use std::path::Path;

pub(super) fn cmd_version_info(project: Option<String>, dev_ops: &Path, json: bool) {
    let path = super::resolve_project_path(project, dev_ops);
    match crate::core::version::info(&path) {
        None => eprintln!("✗ No version file found (Cargo.toml / package.json / pyproject.toml)"),
        Some(v) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                println!("Version:  {} ({})", v.current, v.project_type);
                println!("File:     {}", v.version_file);
                match &v.last_tag {
                    Some(t) => println!("Last tag: {}  ({} commits since)", t, v.commits_since_tag),
                    None => println!("Last tag: (none)"),
                }
                if v.commits_since_tag > 0 {
                    println!("\nChanges since {}:", v.last_tag.as_deref().unwrap_or("beginning"));
                    println!("{}", crate::core::version::changelog(&path));
                }
            }
        }
    }
}

pub(super) fn cmd_version_bump(level: &str, project: Option<String>, changelog: bool, tag: bool, dev_ops: &Path, json: bool) {
    let bump_type = match crate::core::version::BumpType::parse(level) {
        Some(b) => b,
        None => {
            eprintln!("✗ Invalid bump level '{}' — use: patch | minor | major", level);
            return;
        }
    };
    let path = super::resolve_project_path(project, dev_ops);
    let r = crate::core::version::bump(&path, &bump_type, changelog, tag);

    if json {
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
        return;
    }
    if r.ok {
        println!("✓ {} → {}  ({})", r.old_version, r.new_version, r.version_file);
        if changelog { println!("✓ CHANGELOG.md updated"); }
        if tag { println!("✓ Git tag v{} created", r.new_version); }
        if !r.changelog_entry.is_empty() { println!("\n{}", r.changelog_entry); }
    } else {
        eprintln!("✗ {}", r.message);
    }
}
