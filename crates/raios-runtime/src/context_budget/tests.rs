use super::*;
use std::fs;
use std::time::Duration;
use tempfile::TempDir;

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn set_age(path: &Path, secs_ago: u64) {
    let when = SystemTime::now() - Duration::from_secs(secs_ago);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

// ─── measure ─────────────────────────────────────────────────────────────────

#[test]
fn sums_source_files_recursively() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("a.txt"), 5);
    write(&tmp.path().join("sub/b.txt"), 10);
    let m = measure(tmp.path(), 1000);
    assert_eq!(
        m,
        Measured {
            bytes: 15,
            exceeded: false
        }
    );
}

#[test]
fn empty_dir_and_plain_file_measure_as_zero() {
    let tmp = TempDir::new().unwrap();
    assert_eq!(measure(tmp.path(), 10).bytes, 0);
    let file = tmp.path().join("solo.txt");
    write(&file, 50);
    // Not a directory: nothing to walk.
    assert_eq!(
        measure(&file, 10),
        Measured {
            bytes: 0,
            exceeded: false
        }
    );
}

#[test]
fn build_output_dependencies_vcs_and_hidden_dirs_are_not_counted() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("src/main.rs"), 100);
    write(&tmp.path().join("target/debug/huge.bin"), 1_000_000);
    write(&tmp.path().join("node_modules/pkg/index.js"), 1_000_000);
    write(&tmp.path().join(".git/objects/pack/p"), 1_000_000);
    write(&tmp.path().join(".venv/lib/x.py"), 1_000_000);
    write(&tmp.path().join("dist/bundle.js"), 1_000_000);
    write(&tmp.path().join("sub/__pycache__/m.pyc"), 1_000_000);
    let m = measure(tmp.path(), 10_000);
    assert_eq!(
        m,
        Measured {
            bytes: 100,
            exceeded: false
        }
    );
}

#[test]
fn reports_exceeded_and_stops_early() {
    let tmp = TempDir::new().unwrap();
    for i in 0..20 {
        write(&tmp.path().join(format!("f{i}.txt")), 100);
    }
    let m = measure(tmp.path(), 250);
    assert!(m.exceeded);
    // Stopped soon after crossing the limit instead of summing all 2000 bytes.
    assert!(m.bytes > 250 && m.bytes < 2000, "{m:?}");
}

#[test]
fn exactly_at_the_limit_is_not_exceeded() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("a.txt"), 100);
    assert!(!measure(tmp.path(), 100).exceeded);
    assert!(measure(tmp.path(), 99).exceeded);
}

#[cfg(unix)]
#[test]
fn symlinks_are_neither_followed_nor_counted() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("real.txt"), 7);
    // A directory link back to the root: followed, this recurses forever.
    std::os::unix::fs::symlink(tmp.path(), tmp.path().join("loop")).unwrap();
    // A file link to a large file outside the tree.
    let outside = TempDir::new().unwrap();
    write(&outside.path().join("big.bin"), 1_000_000);
    std::os::unix::fs::symlink(outside.path().join("big.bin"), tmp.path().join("link.bin"))
        .unwrap();
    assert_eq!(
        measure(tmp.path(), 10_000),
        Measured {
            bytes: 7,
            exceeded: false
        }
    );
}

// ─── sigmap freshness ────────────────────────────────────────────────────────

#[test]
fn missing_sigmap_is_not_fresh() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("src/lib.rs"), 10);
    assert!(!sigmap_is_fresh(tmp.path()));
}

#[test]
fn sigmap_newer_than_every_source_is_fresh() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("src/lib.rs"), 10);
    write(&tmp.path().join(SIGMAP_FILE), 10);
    set_age(&tmp.path().join("src/lib.rs"), 100);
    set_age(&tmp.path().join(SIGMAP_FILE), 10);
    assert!(sigmap_is_fresh(tmp.path()));
}

#[test]
fn a_source_edited_after_sigmap_makes_it_stale() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("src/lib.rs"), 10);
    write(&tmp.path().join(SIGMAP_FILE), 10);
    set_age(&tmp.path().join(SIGMAP_FILE), 100);
    set_age(&tmp.path().join("src/lib.rs"), 10);
    assert!(!sigmap_is_fresh(tmp.path()));
}

#[test]
fn markdown_and_ignored_dirs_do_not_make_sigmap_stale() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("src/lib.rs"), 10);
    write(&tmp.path().join(SIGMAP_FILE), 10);
    write(&tmp.path().join("memory.md"), 10);
    write(&tmp.path().join("target/debug/out.rs"), 10);
    write(&tmp.path().join(".git/HEAD"), 10);
    set_age(&tmp.path().join("src/lib.rs"), 100);
    set_age(&tmp.path().join(SIGMAP_FILE), 50);
    // Everything below is newer than the map but must not count as a source edit.
    set_age(&tmp.path().join("memory.md"), 1);
    set_age(&tmp.path().join("target/debug/out.rs"), 1);
    set_age(&tmp.path().join(".git/HEAD"), 1);
    assert!(sigmap_is_fresh(tmp.path()));
}
