use std::path::{Path, PathBuf};
use std::process::Command;
use chrono::Local;

pub struct NewProjectConfig<'a> {
    pub name: &'a str,
    pub category: &'a str,
    pub dev_ops: &'a Path,
    pub github: bool,
    pub no_vault: bool,
}

pub struct NewProjectResult {
    pub path: PathBuf,
    pub github_url: Option<String>,
    pub steps: Vec<(String, bool)>, // (description, success)
}

pub fn create(cfg: &NewProjectConfig) -> NewProjectResult {
    let project_dir = cfg.dev_ops.join(cfg.category).join(cfg.name);
    let mut steps = Vec::new();
    let today = Local::now().format("%Y-%m-%d").to_string();

    // 1. Create folder structure
    let dirs_to_create = ["", "code", "reference", "public"];
    let mut dir_ok = true;
    for sub in &dirs_to_create {
        let target = if sub.is_empty() { project_dir.clone() } else { project_dir.join(sub) };
        if let Err(e) = std::fs::create_dir_all(&target) {
            steps.push((format!("create dir {}: {}", target.display(), e), false));
            dir_ok = false;
        }
    }
    steps.push(("Create folder structure".into(), dir_ok));

    // 2. Write memory.md
    let memory_content = format!(r#"# {name} Memory

## Son Durum
- Tarih: {today}
- Aktif agent: Claude Code

## Claude
### Yaptıkları
- Proje oluşturuldu (raios new)
### Yapacakları
- [ ] —
### Notlar
- —

## Gemini
### Yaptıkları
- —
### Yapacakları
- —
### Notlar
- —

## Antigravity
### Yaptıkları
- —
### Yapacakları
- —
### Notlar
- —

## Plan
### Tamamlananlar
- [x] Proje iskelet kurulumu
### Devam Edenler
- [ ] —
### Sıradakiler
- [ ] —

## Karar Günlüğü
| Tarih | Agent | Karar | Neden |
|-------|-------|-------|-------|
| {today} | Claude | Proje oluşturuldu | raios new komutu |
"#, name = cfg.name, today = today);

    let mem_ok = std::fs::write(project_dir.join("memory.md"), memory_content).is_ok();
    steps.push(("Write memory.md".into(), mem_ok));

    // 3. Write README.md
    let readme_content = format!(r#"# {name}

> Created: {today}

## Overview

TODO: Describe the project.

## Structure

```
{name}/
├── code/        # Source code
├── reference/   # Docs, specs, research
└── public/      # Exported / distributable files
```

## Usage

TODO

## Stack

TODO
"#, name = cfg.name, today = today);

    let readme_ok = std::fs::write(project_dir.join("README.md"), readme_content).is_ok();
    steps.push(("Write README.md".into(), readme_ok));

    // 4. Write gitrepo.md
    let gitrepo_content = format!(
        "# {}\n\n- **GitHub:** TBD\n- **Created:** {}\n- **Category:** {}\n",
        cfg.name, today, cfg.category
    );
    let gitrepo_ok = std::fs::write(project_dir.join("gitrepo.md"), gitrepo_content).is_ok();
    steps.push(("Write gitrepo.md".into(), gitrepo_ok));

    // 4b. Write .raios.yaml manifest
    let manifest = format!(
        "name: \"{}\"\ncategory: \"{}\"\nstack: unknown\ngithub: null\nstatus: active\n",
        cfg.name, cfg.category
    );
    let manifest_ok = std::fs::write(project_dir.join(".raios.yaml"), manifest).is_ok();
    steps.push(("Write .raios.yaml".into(), manifest_ok));

    // 5. git init
    let git_ok = Command::new("git")
        .arg("init")
        .current_dir(&project_dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    steps.push(("git init".into(), git_ok));

    // 6. Initial commit
    let _ = Command::new("git").args(["add", "-A"]).current_dir(&project_dir).output();
    let init_commit_ok = Command::new("git")
        .args(["commit", "-m", "chore: initial scaffold (raios new)"])
        .current_dir(&project_dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    steps.push(("Initial commit".into(), init_commit_ok));

    // 7. GitHub repo creation (optional)
    let mut github_url: Option<String> = None;
    if cfg.github {
        let slug = cfg.name.to_lowercase().replace(' ', "-");
        let gh_out = Command::new("gh")
            .args(["repo", "create", &slug, "--private", "--source=.", "--push", "--remote=origin"])
            .current_dir(&project_dir)
            .output();

        let gh_ok = gh_out.as_ref().map(|o| o.status.success()).unwrap_or(false);
        if gh_ok {
            github_url = Some(format!("https://github.com/alazndy/{}", slug));
            let updated_gitrepo = format!(
                "# {}\n\n- **GitHub:** {}\n- **Created:** {}\n- **Category:** {}\n",
                cfg.name,
                github_url.as_deref().unwrap_or(""),
                today,
                cfg.category
            );
            let _ = std::fs::write(project_dir.join("gitrepo.md"), updated_gitrepo);
        }
        steps.push(("Create GitHub repo".into(), gh_ok));
    }

    // 8. Add to entities.json
    let entities_ok = add_to_entities(cfg.dev_ops, cfg.name, cfg.category, &project_dir, github_url.as_deref());
    steps.push(("Update entities.json".into(), entities_ok));

    // 9. Update Vault Proje Atlası (unless --no-vault)
    if !cfg.no_vault {
        let vault_ok = update_vault_atlas(cfg.dev_ops, cfg.name, cfg.category, &project_dir, github_url.as_deref(), &today);
        steps.push(("Update Vault Proje Atlası".into(), vault_ok));
    }

    NewProjectResult { path: project_dir, github_url, steps }
}

fn add_to_entities(dev_ops: &Path, name: &str, category: &str, path: &Path, github: Option<&str>) -> bool {
    let mut projects = crate::entities::load_entities(dev_ops);
    if projects.iter().any(|p| p.local_path == path) {
        return true;
    }
    projects.push(crate::entities::EntityProject {
        name: name.to_string(),
        category: category.to_string(),
        local_path: path.to_path_buf(),
        github: github.map(str::to_string),
        status: "active".to_string(),
        stars: None,
        last_commit: None,
        version: None,
        version_nickname: None,
    });
    crate::entities::save_entities(dev_ops, projects).is_ok()
}

fn update_vault_atlas(
    dev_ops: &Path,
    name: &str,
    category: &str,
    _path: &Path,
    github: Option<&str>,
    today: &str,
) -> bool {
    let atlas_candidates = [
        PathBuf::from(r"C:\Users\turha\Documents\Obsidian Vaults\Vault101\Projeler\Proje Atlası.md"),
        dev_ops.join("..").join("..").join("Documents").join("Obsidian Vaults").join("Vault101").join("Projeler").join("Proje Atlası.md"),
    ];

    for atlas_path in &atlas_candidates {
        if !atlas_path.exists() { continue; }

        let Ok(mut content) = std::fs::read_to_string(atlas_path) else { continue };

        let github_display = github.unwrap_or("—");
        let entry = format!(
            "\n| {} | {} | active | — | {} | {} |",
            name, category, github_display, today
        );

        // Append before last line or at end
        if let Some(pos) = content.rfind('\n') {
            content.insert_str(pos, &entry);
        } else {
            content.push_str(&entry);
        }

        return std::fs::write(atlas_path, content).is_ok();
    }
    false
}
