//! Behavior of the project capability: listing, reading and saving files inside one project.

use std::fs;
use std::path::PathBuf;

use rwk_core::documents::{Document, Revision, MAX_FILE_BYTES};
use rwk_core::error::ErrorCode;
use rwk_core::project::ProjectRoot;
use tempfile::TempDir;

fn project() -> (TempDir, ProjectRoot) {
    let directory = tempfile::tempdir().unwrap();
    let root = ProjectRoot::open(directory.path().to_path_buf()).unwrap();
    (directory, root)
}

fn paths(entries: &[rwk_core::project::FileEntry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.path.as_str()).collect()
}

fn leftovers(directory: &TempDir) -> Vec<String> {
    fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".raios-save-"))
        .collect()
}

#[test]
fn opening_a_directory_canonicalizes_it_and_issues_an_opaque_id() {
    let directory = tempfile::tempdir().unwrap();
    let first = ProjectRoot::open(directory.path().to_path_buf()).unwrap();
    let second = ProjectRoot::open(directory.path().to_path_buf()).unwrap();

    let canonical = directory.path().canonicalize().unwrap();
    assert_eq!(first.info().root, canonical);
    assert_eq!(
        first.info().name,
        canonical.file_name().unwrap().to_string_lossy()
    );
    assert!(!first.info().id.as_str().is_empty());
    assert!(!first.info().id.as_str().contains('/'));
    assert_ne!(first.info().id, second.info().id);
}

#[test]
fn opening_something_that_is_not_a_directory_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    let file = directory.path().join("file.txt");
    fs::write(&file, "x").unwrap();

    let error = ProjectRoot::open(missing).err().unwrap();
    assert_eq!(error.code, ErrorCode::NotFound);
    let error = ProjectRoot::open(file).err().unwrap();
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn listing_puts_directories_first_sorts_case_insensitively_and_skips_noise() {
    let (directory, root) = project();
    for name in ["b.txt", "A.txt"] {
        fs::write(directory.path().join(name), "x").unwrap();
    }
    for name in [
        "src",
        "Zed",
        ".git",
        "node_modules",
        "target",
        ".venv",
        "venv",
    ] {
        fs::create_dir(directory.path().join(name)).unwrap();
    }
    fs::write(directory.path().join("src").join("main.rs"), "fn main() {}").unwrap();

    let top = root.list("").unwrap();
    assert_eq!(paths(&top), ["src", "Zed", "A.txt", "b.txt"]);
    assert!(top[0].is_directory && top[1].is_directory);
    assert!(!top[2].is_directory);

    let nested = root.list("src").unwrap();
    assert_eq!(paths(&nested), ["src/main.rs"]);
    assert_eq!(nested[0].name, "main.rs");
}

#[test]
fn listing_rejects_bad_paths_and_reports_missing_directories() {
    let (_directory, root) = project();
    for bad in ["../outside", "/etc", "a//b", "a/"] {
        let error = root.list(bad).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{bad:?}");
    }
    assert_eq!(root.list("missing").unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn a_directory_with_too_many_entries_is_refused() {
    let (directory, root) = project();
    for index in 0..2001 {
        fs::write(directory.path().join(format!("f{index}.txt")), "").unwrap();
    }
    assert_eq!(root.list("").unwrap_err().code, ErrorCode::InvalidInput);
}

#[cfg(unix)]
#[test]
fn symlinks_are_never_listed_and_never_followed() {
    let (directory, root) = project();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "top secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), directory.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        directory.path().join("link.txt"),
    )
    .unwrap();
    fs::write(directory.path().join("real.txt"), "ok").unwrap();

    assert_eq!(paths(&root.list("").unwrap()), ["real.txt"]);
    assert_eq!(
        root.list("escape").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        root.read_document("escape/secret.txt").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        root.read_document("link.txt").unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn reading_returns_the_content_and_its_sha256_revision() {
    let (directory, root) = project();
    fs::write(directory.path().join("a.txt"), "hello").unwrap();

    let document = root.read_document("a.txt").unwrap();
    assert_eq!(document.content, "hello");
    assert_eq!(document.path, "a.txt");
    assert_eq!(&document.project_id, &root.info().id);
    assert_eq!(document.revision, Revision::of(b"hello"));
}

#[test]
fn reading_refuses_binary_oversized_directory_and_traversal_targets() {
    let (directory, root) = project();
    fs::write(directory.path().join("binary.bin"), [0xff, 0xfe, 0x00]).unwrap();
    fs::write(
        directory.path().join("big.txt"),
        vec![b'a'; MAX_FILE_BYTES + 1],
    )
    .unwrap();
    fs::write(
        directory.path().join("limit.txt"),
        vec![b'a'; MAX_FILE_BYTES],
    )
    .unwrap();
    fs::create_dir(directory.path().join("folder")).unwrap();

    for refused in ["binary.bin", "big.txt", "folder", "../outside", ""] {
        let error = root.read_document(refused).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{refused:?}");
    }
    assert_eq!(
        root.read_document("missing.txt").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        root.read_document("limit.txt").unwrap().content.len(),
        MAX_FILE_BYTES
    );
}

#[test]
fn saving_replaces_the_content_and_returns_the_new_revision() {
    let (directory, root) = project();
    fs::write(directory.path().join("a.txt"), "one").unwrap();

    let document = root.read_document("a.txt").unwrap();
    let saved = root
        .save_document(Document {
            content: "two".into(),
            ..document
        })
        .unwrap();

    assert_eq!(
        fs::read_to_string(directory.path().join("a.txt")).unwrap(),
        "two"
    );
    assert_eq!(saved.revision, Revision::of(b"two"));
    // The returned document is a valid base for the next save.
    let again = root
        .save_document(Document {
            content: "three".into(),
            ..saved
        })
        .unwrap();
    assert_eq!(again.revision, Revision::of(b"three"));
    assert!(leftovers(&directory).is_empty());
}

#[cfg(unix)]
#[test]
fn saving_keeps_the_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (directory, root) = project();
    let path = directory.path().join("script.sh");
    fs::write(&path, "echo 1").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();

    let document = root.read_document("script.sh").unwrap();
    root.save_document(Document {
        content: "echo 2".into(),
        ..document
    })
    .unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o750);
}

#[test]
fn saving_a_file_that_changed_on_disk_is_a_conflict_and_keeps_the_disk_version() {
    let (directory, root) = project();
    let path = directory.path().join("a.txt");
    fs::write(&path, "original").unwrap();
    let document = root.read_document("a.txt").unwrap();

    fs::write(&path, "changed elsewhere").unwrap();
    let error = root
        .save_document(Document {
            content: "my edit".into(),
            ..document
        })
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(fs::read_to_string(&path).unwrap(), "changed elsewhere");
    assert!(leftovers(&directory).is_empty());
}

#[test]
fn saving_rejects_foreign_documents_oversized_content_bad_paths_and_missing_files() {
    let (directory, root) = project();
    fs::write(directory.path().join("a.txt"), "x").unwrap();
    let (_other_directory, other) = project();
    let foreign = other.info().id.clone();
    let document = root.read_document("a.txt").unwrap();

    let mut cases: Vec<(Document, ErrorCode)> = vec![
        (
            Document {
                project_id: foreign,
                ..document.clone()
            },
            ErrorCode::InvalidInput,
        ),
        (
            Document {
                content: "x".repeat(MAX_FILE_BYTES + 1),
                ..document.clone()
            },
            ErrorCode::InvalidInput,
        ),
        (
            Document {
                path: "../outside".into(),
                ..document.clone()
            },
            ErrorCode::InvalidInput,
        ),
        (
            Document {
                path: "missing.txt".into(),
                ..document.clone()
            },
            ErrorCode::NotFound,
        ),
    ];
    for (candidate, expected) in cases.drain(..) {
        let error = root.save_document(candidate).unwrap_err();
        assert_eq!(error.code, expected);
    }
    assert_eq!(
        fs::read_to_string(directory.path().join("a.txt")).unwrap(),
        "x"
    );
}

#[cfg(unix)]
#[test]
fn saving_never_writes_through_a_link_that_leaves_the_project() {
    let (directory, root) = project();
    let outside = tempfile::tempdir().unwrap();
    let target: PathBuf = outside.path().join("victim.txt");
    fs::write(&target, "untouched").unwrap();
    std::os::unix::fs::symlink(&target, directory.path().join("link.txt")).unwrap();

    let forged = Document {
        project_id: root.info().id.clone(),
        path: "link.txt".into(),
        content: "overwritten".into(),
        revision: Revision::of(b"untouched"),
    };
    assert!(root.save_document(forged).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "untouched");
}
