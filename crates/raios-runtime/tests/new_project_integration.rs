//! End-to-end coverage for `raios_runtime::new_project::create`, exercising
//! the real filesystem + git scaffolding path (no network / gh CLI calls).

use raios_runtime::new_project::{create, NewProjectConfig};

#[test]
fn create_scaffolds_a_project_without_fabricating_a_github_url() {
    let dev_ops = tempfile::tempdir().expect("tempdir");

    let cfg = NewProjectConfig {
        name: "sample-project",
        category: "tools",
        dev_ops: dev_ops.path(),
        github: false,
        no_vault: true,
        vault_path: None,
    };

    let result = create(&cfg);

    let project_dir = dev_ops.path().join("tools").join("sample-project");
    assert_eq!(result.path, project_dir);
    assert!(project_dir.join("code").is_dir());
    assert!(project_dir.join("reference").is_dir());
    assert!(project_dir.join("public").is_dir());
    assert!(project_dir.join(".git").is_dir());

    let memory = std::fs::read_to_string(project_dir.join("memory.md")).expect("memory.md");
    assert!(memory.contains("sample-project"));

    // With github integration off, no GitHub repo was created, so no URL
    // should ever be fabricated from a hardcoded owner — the placeholder
    // written in step 4 must survive untouched.
    let gitrepo = std::fs::read_to_string(project_dir.join("gitrepo.md")).expect("gitrepo.md");
    assert!(gitrepo.contains("**GitHub:** TBD"));
    assert!(result.github_url.is_none());
    assert!(!gitrepo.contains("github.com"));

    let all_steps_have_a_description = result.steps.iter().all(|(desc, _)| !desc.is_empty());
    assert!(all_steps_have_a_description);
}

#[test]
fn create_writes_a_vault_note_when_vault_is_enabled() {
    let dev_ops = tempfile::tempdir().expect("dev_ops tempdir");
    let vault = tempfile::tempdir().expect("vault tempdir");

    let cfg = NewProjectConfig {
        name: "vaulted-project",
        category: "tools",
        dev_ops: dev_ops.path(),
        github: false,
        no_vault: false,
        vault_path: Some(vault.path()),
    };

    let result = create(&cfg);

    let vault_ok = result
        .steps
        .iter()
        .find(|(desc, _)| desc == "Update Obsidian Vault")
        .map(|(_, ok)| *ok)
        .unwrap_or(false);
    assert!(vault_ok, "vault step should succeed: {:?}", result.steps);

    let note_path = vault
        .path()
        .join("Projeler")
        .join("tools")
        .join("vaulted-project.md");
    assert!(note_path.exists(), "expected vault note at {note_path:?}");
}
