//! Renders and writes an Obsidian-compatible vault of raios project notes.
//!
//! See docs/superpowers/specs/2026-07-31-obsidian-vault-sync-design.md.

use raios_core::entities::EntityProject;

fn yaml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn render_project_note(
    project: &EntityProject,
    memory_content: Option<&str>,
    synced_at: &str,
) -> String {
    let github = project.github.as_deref().unwrap_or("");
    let last_commit = project.last_commit.as_deref().unwrap_or("");
    let version = project.version.as_deref().unwrap_or("");
    let body = memory_content.unwrap_or("_memory.md not found_");

    format!(
        "---\n\
         tags: [proje, \"kategori/{category}\", \"durum/{status}\"]\n\
         category: {category_q}\n\
         status: {status_q}\n\
         local_path: {local_path_q}\n\
         github: {github_q}\n\
         last_commit: {last_commit_q}\n\
         version: {version_q}\n\
         synced: {synced_q}\n\
         ---\n\
         # {name}\n\
         \n\
         ← [[{category}-MOC|{category} projeleri]]\n\
         \n\
         {body}\n",
        category = project.category,
        status = project.status,
        category_q = yaml_quote(&project.category),
        status_q = yaml_quote(&project.status),
        local_path_q = yaml_quote(&project.local_path.to_string_lossy()),
        github_q = yaml_quote(github),
        last_commit_q = yaml_quote(last_commit),
        version_q = yaml_quote(version),
        synced_q = yaml_quote(synced_at),
        name = project.name,
        body = body,
    )
}

fn render_moc(category: &str, entries: &[(String, String)]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = format!(
        "---\ntags: [moc, \"kategori/{category}\"]\n---\n# {category} projeleri\n\n"
    );
    for (name, status) in &sorted {
        out.push_str(&format!("- [[{name}]] — {status}\n"));
    }
    out
}

fn render_atlas(projects: &[EntityProject]) -> String {
    use std::collections::BTreeMap;

    let mut by_category: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
    for p in projects {
        *by_category.entry(p.category.as_str()).or_insert(0) += 1;
        *by_status.entry(p.status.as_str()).or_insert(0) += 1;
    }

    let mut out = format!("# Proje Atlası\n\nToplam: {} proje\n\n## Kategoriler\n", projects.len());
    for (category, count) in &by_category {
        out.push_str(&format!("- [[{category}-MOC|{category}]] — {count} proje\n"));
    }
    out.push_str("\n## Durum\n");
    for (status, count) in &by_status {
        out.push_str(&format!("- {status}: {count}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_project() -> EntityProject {
        EntityProject {
            name: "sample-project".to_string(),
            category: "ai".to_string(),
            local_path: PathBuf::from("/home/alaz/dev/ai/sample-project"),
            github: Some("https://github.com/alazndy/sample-project".to_string()),
            status: "active".to_string(),
            stars: None,
            last_commit: Some("2026-07-30".to_string()),
            version: Some("1.2.0".to_string()),
            version_nickname: None,
        }
    }

    #[test]
    fn render_project_note_includes_frontmatter_and_memory_content() {
        let project = sample_project();
        let note = render_project_note(
            &project,
            Some("# Project Memory: sample-project\n\nsome content"),
            "2026-07-31T22:00:00",
        );

        assert!(note.starts_with("---\n"));
        assert!(note.contains("tags: [proje, \"kategori/ai\", \"durum/active\"]"));
        assert!(note.contains("category: \"ai\""));
        assert!(note.contains("status: \"active\""));
        assert!(note.contains("local_path: \"/home/alaz/dev/ai/sample-project\""));
        assert!(note.contains("github: \"https://github.com/alazndy/sample-project\""));
        assert!(note.contains("last_commit: \"2026-07-30\""));
        assert!(note.contains("version: \"1.2.0\""));
        assert!(note.contains("synced: \"2026-07-31T22:00:00\""));
        assert!(note.contains("# sample-project"));
        assert!(note.contains("[[ai-MOC|ai projeleri]]"));
        assert!(note.contains("some content"));
    }

    #[test]
    fn render_project_note_handles_missing_memory_md() {
        let project = sample_project();
        let note = render_project_note(&project, None, "2026-07-31T22:00:00");
        assert!(note.contains("_memory.md not found_"));
    }

    #[test]
    fn render_project_note_handles_none_fields() {
        let mut project = sample_project();
        project.github = None;
        project.last_commit = None;
        project.version = None;
        let note = render_project_note(&project, Some("x"), "2026-07-31T22:00:00");
        assert!(note.contains("github: \"\""));
        assert!(note.contains("last_commit: \"\""));
        assert!(note.contains("version: \"\""));
    }

    #[test]
    fn render_moc_lists_entries_sorted_by_name() {
        let entries = vec![
            ("zeta".to_string(), "active".to_string()),
            ("alpha".to_string(), "beklemede".to_string()),
        ];
        let moc = render_moc("ai", &entries);
        assert!(moc.contains("tags: [moc, \"kategori/ai\"]"));
        assert!(moc.contains("# ai projeleri"));
        let alpha_pos = moc.find("[[alpha]] — beklemede").expect("alpha entry");
        let zeta_pos = moc.find("[[zeta]] — active").expect("zeta entry");
        assert!(alpha_pos < zeta_pos, "entries must be sorted by name");
    }

    #[test]
    fn render_atlas_counts_by_category_and_status() {
        let mut p2 = sample_project();
        p2.name = "other-project".to_string();
        p2.category = "web".to_string();
        p2.status = "beklemede".to_string();
        let projects = vec![sample_project(), p2];

        let atlas = render_atlas(&projects);
        assert!(atlas.contains("Toplam: 2 proje"));
        assert!(atlas.contains("[[ai-MOC|ai]] — 1 proje"));
        assert!(atlas.contains("[[web-MOC|web]] — 1 proje"));
        assert!(atlas.contains("active: 1"));
        assert!(atlas.contains("beklemede: 1"));
    }

    #[test]
    fn yaml_quote_escapes_backslash_and_quote() {
        assert_eq!(yaml_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
