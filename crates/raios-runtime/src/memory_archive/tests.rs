use super::*;
use std::fs;
use tempfile::TempDir;

/// Policy with tiny budgets so a handful of short entries triggers archiving.
fn tight(change_log_bytes: usize, instinct_bytes: usize) -> ArchivePolicy {
    ArchivePolicy {
        change_log_bytes,
        instinct_bytes,
        min_entries: 2,
        trigger_bytes: 0,
    }
}

fn entry(date: &str, n: usize) -> String {
    format!("- [{date}] [Agent]: entry number {n} {}\n", "x".repeat(30))
}

/// A realistic memory.md: guiding sections, a newest-first Change Log, Instincts.
fn memory(change_log: &[String], instincts: &[String]) -> String {
    let mut s = String::from(
        "# Project Memory: demo\n\n## Context\n- keep this\n\n## Current Focus\n- and this\n\n## Change Log & Agent Trail\n\n",
    );
    change_log.iter().for_each(|e| s.push_str(e));
    s.push_str("\n## Instincts\n");
    instincts.iter().for_each(|e| s.push_str(e));
    s
}

fn newest_first(count: usize) -> Vec<String> {
    // 2026-10-<count> down to 2026-10-01: newest on top.
    (0..count)
        .map(|i| entry(&format!("2026-10-{:02}", count - i), count - i))
        .collect()
}

fn instincts(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            format!(
                "- Situation: rule {i}. Response: do thing {i} {}\n",
                "y".repeat(30)
            )
        })
        .collect()
}

fn non_blank_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

// ─── planning ────────────────────────────────────────────────────────────────

#[test]
fn newest_entries_stay_and_oldest_move_in_a_newest_first_log() {
    let content = memory(&newest_first(10), &instincts(1));
    let plan = plan(&content, &tight(300, 100_000));

    assert!(!plan.change_log.is_empty());
    // The newest entry is kept, the oldest one moved.
    assert!(plan.new_content.contains("entry number 10 "));
    assert!(!plan.new_content.contains("entry number 1 x"));
    assert!(plan
        .change_log
        .iter()
        .any(|e| e.contains("entry number 1 ")));
    // Moved entries keep file order: newest of the moved first.
    let first_moved = &plan.change_log[0];
    let last_moved = plan.change_log.last().unwrap();
    assert!(first_moved.contains("2026-10-0") || first_moved.contains("2026-10-"));
    assert!(
        first_moved > last_moved,
        "moved entries stay in file order (newest first)"
    );
}

#[test]
fn an_oldest_first_log_is_detected_from_its_dates() {
    // Dates ascending: the newest entry is at the bottom.
    let oldest_first: Vec<String> = newest_first(8).into_iter().rev().collect();
    let content = memory(&oldest_first, &instincts(1));
    let plan = plan(&content, &tight(250, 100_000));

    assert!(
        plan.new_content.contains("entry number 8 "),
        "newest (bottom) must stay"
    );
    assert!(
        !plan.new_content.contains("entry number 1 x"),
        "oldest (top) must move"
    );
    assert!(plan
        .change_log
        .iter()
        .any(|e| e.contains("entry number 1 ")));
}

#[test]
fn a_log_whose_order_cannot_be_determined_is_left_alone() {
    let undated: Vec<String> = (0..10)
        .map(|i| format!("- did thing {i} {}\n", "z".repeat(40)))
        .collect();
    let content = memory(&undated, &[]);
    let plan = plan(&content, &tight(50, 50));
    assert!(plan.change_log.is_empty());
    assert_eq!(plan.new_content, content);

    // All entries on one day: also not enough to tell which end is newest.
    let same_day: Vec<String> = (0..10).map(|i| entry("2026-10-05", i)).collect();
    let content = memory(&same_day, &[]);
    assert_eq!(super::plan(&content, &tight(50, 50)).new_content, content);
}

#[test]
fn instincts_are_archived_oldest_last() {
    let content = memory(&newest_first(2), &instincts(10));
    let plan = plan(&content, &tight(100_000, 250));

    assert!(
        plan.new_content.contains("rule 0."),
        "newest instinct (top) stays"
    );
    assert!(
        !plan.new_content.contains("rule 9."),
        "oldest instinct (bottom) moves"
    );
    assert!(plan.instincts.iter().any(|e| e.contains("rule 9.")));
    assert!(plan.change_log.is_empty());
}

#[test]
fn at_least_min_entries_are_kept_even_when_over_budget() {
    let content = memory(&newest_first(10), &[]);
    let plan = plan(&content, &tight(1, 1));
    let kept = plan.new_content.matches("[Agent]: entry number").count();
    assert_eq!(kept, 2, "min_entries");
    assert_eq!(plan.change_log.len(), 8);
}

#[test]
fn continuation_lines_travel_with_their_entry() {
    let mut log = newest_first(6);
    // Give the oldest entry a multi-line body with a nested bullet.
    let last = log.pop().unwrap();
    log.push(format!(
        "{last}  - nested detail of the oldest\n  a wrapped line\n"
    ));
    let content = memory(&log, &[]);

    let plan = plan(&content, &tight(150, 100_000));
    let oldest = plan.change_log.last().unwrap();
    assert!(oldest.contains("nested detail of the oldest"));
    assert!(oldest.contains("a wrapped line"));
    assert!(!plan.new_content.contains("nested detail"));
}

#[test]
fn other_sections_and_spacing_are_preserved_byte_for_byte() {
    let content = memory(&newest_first(10), &instincts(3));
    let plan = plan(&content, &tight(300, 100_000));

    assert!(plan.new_content.starts_with(
        "# Project Memory: demo\n\n## Context\n- keep this\n\n## Current Focus\n- and this\n\n## Change Log & Agent Trail\n\n"
    ));
    // The Instincts section is untouched and still separated by a blank line.
    assert!(plan
        .new_content
        .ends_with(&format!("\n## Instincts\n{}", instincts(3).concat())));
}

#[test]
fn nothing_is_lost_for_any_budget() {
    let content = memory(&newest_first(25), &instincts(12));
    for log_budget in [0, 100, 400, 1000, 100_000] {
        for instinct_budget in [0, 200, 100_000] {
            let plan = plan(&content, &tight(log_budget, instinct_budget));
            let moved: Vec<&String> = plan
                .change_log
                .iter()
                .chain(plan.instincts.iter())
                .collect();
            assert!(
                nothing_lost(&content, &plan.new_content, &moved),
                "lost lines at budgets {log_budget}/{instinct_budget}"
            );
            // And independently of the helper: line multiset check.
            let mut rebuilt = non_blank_lines(&plan.new_content);
            moved
                .iter()
                .for_each(|m| rebuilt.extend(non_blank_lines(m)));
            let mut original = non_blank_lines(&content);
            original.sort();
            rebuilt.sort();
            assert_eq!(original, rebuilt, "budgets {log_budget}/{instinct_budget}");
        }
    }
}

#[test]
fn crlf_line_endings_are_preserved() {
    let content = memory(&newest_first(8), &[]).replace('\n', "\r\n");
    let plan = plan(&content, &tight(250, 100_000));
    assert!(!plan.change_log.is_empty());
    assert!(
        !plan.new_content.replace("\r\n", "").contains('\n'),
        "no bare LF introduced"
    );
    assert!(plan.new_content.contains("## Current Focus\r\n"));
}

#[test]
fn a_file_without_log_sections_is_unchanged() {
    let content = "# Title\n\n## Context\n- only this\n";
    assert_eq!(plan(content, &tight(0, 0)).new_content, content);
}

#[test]
fn misfiled_change_log_lines_in_instincts_are_counted() {
    let mut inst = instincts(2);
    inst.push(entry("2026-09-01", 99));
    let content = memory(&newest_first(2), &inst);
    assert_eq!(plan(&content, &tight(100_000, 100_000)).misfiled, 1);
}

// ─── applying to disk ────────────────────────────────────────────────────────

fn project_with(content: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(MEMORY_FILE), content).unwrap();
    dir
}

#[test]
fn archive_memory_moves_old_entries_to_the_archive_file() {
    let content = memory(&newest_first(12), &instincts(6));
    let dir = project_with(&content);

    let report = archive_memory(dir.path(), &tight(400, 300), false).unwrap();
    assert!(report.applied);
    assert!(report.change_log_moved > 0 && report.instincts_moved > 0);
    assert!(report.bytes_after < report.bytes_before);

    let memory_now = fs::read_to_string(dir.path().join(MEMORY_FILE)).unwrap();
    let archive = fs::read_to_string(dir.path().join(ARCHIVE_FILE)).unwrap();
    assert_eq!(memory_now.len(), report.bytes_after);
    assert!(archive.starts_with("# Memory Archive:"));
    assert!(archive.contains("— Change Log ("));
    assert!(archive.contains("— Instincts ("));

    // Every original non-blank line is in memory.md or the archive.
    let mut have = non_blank_lines(&memory_now);
    have.extend(non_blank_lines(&archive));
    for line in non_blank_lines(&content) {
        assert!(have.contains(&line), "lost: {line}");
    }
    // No leftover temp file.
    assert!(!dir
        .path()
        .join(format!(".{MEMORY_FILE}.archive-tmp"))
        .exists());
}

#[test]
fn a_second_run_has_nothing_left_to_move() {
    let dir = project_with(&memory(&newest_first(12), &instincts(6)));
    archive_memory(dir.path(), &tight(400, 300), false).unwrap();
    let archive_after_first = fs::read_to_string(dir.path().join(ARCHIVE_FILE)).unwrap();

    let again = archive_memory(dir.path(), &tight(400, 300), false).unwrap();
    assert_eq!(again.moved(), 0);
    assert!(!again.applied);
    assert_eq!(
        fs::read_to_string(dir.path().join(ARCHIVE_FILE)).unwrap(),
        archive_after_first,
        "archive must not be rewritten when there is nothing to move"
    );
}

#[test]
fn archiving_appends_to_an_existing_archive_without_touching_it() {
    let dir = project_with(&memory(&newest_first(12), &[]));
    fs::write(
        dir.path().join(ARCHIVE_FILE),
        "# Memory Archive: demo\n\nolder text\n",
    )
    .unwrap();
    archive_memory(dir.path(), &tight(400, 100_000), false).unwrap();
    let archive = fs::read_to_string(dir.path().join(ARCHIVE_FILE)).unwrap();
    assert!(archive.starts_with("# Memory Archive: demo\n\nolder text\n"));
    assert_eq!(
        archive.matches("# Memory Archive").count(),
        1,
        "header written once"
    );
}

#[test]
fn dry_run_reports_but_writes_nothing() {
    let content = memory(&newest_first(12), &instincts(6));
    let dir = project_with(&content);
    let report = archive_memory(dir.path(), &tight(400, 300), true).unwrap();
    assert!(report.moved() > 0);
    assert!(!report.applied);
    assert_eq!(
        fs::read_to_string(dir.path().join(MEMORY_FILE)).unwrap(),
        content
    );
    assert!(!dir.path().join(ARCHIVE_FILE).exists());
}

#[test]
fn entries_already_in_the_archive_are_not_duplicated_after_an_interrupted_run() {
    // Simulates a crash after the archive was written but before memory.md was.
    let content = memory(&newest_first(12), &[]);
    let dir = project_with(&content);
    let first = archive_memory(dir.path(), &tight(400, 100_000), false).unwrap();
    assert!(first.applied);
    let archive_after_first = fs::read_to_string(dir.path().join(ARCHIVE_FILE)).unwrap();
    // Roll memory.md back as if the rewrite never happened.
    fs::write(dir.path().join(MEMORY_FILE), &content).unwrap();

    let retry = archive_memory(dir.path(), &tight(400, 100_000), false).unwrap();
    assert!(retry.applied, "memory.md is shrunk on the retry");
    let archive = fs::read_to_string(dir.path().join(ARCHIVE_FILE)).unwrap();
    let marker = "entry number 1 ";
    assert_eq!(
        archive.matches(marker).count(),
        archive_after_first.matches(marker).count()
    );
}

#[test]
fn memory_md_changed_during_archiving_is_left_untouched_and_retried_later() {
    let content = memory(&newest_first(12), &[]);
    let dir = project_with(&content);
    let path = dir.path().join(MEMORY_FILE);
    let concurrent = "- [2026-11-01] [Agent]: written while archiving\n";

    // An agent appends to memory.md after we read it but before we replace it.
    let report = archive_memory_with(dir.path(), &tight(400, 100_000), false, || {
        let mut now = fs::read_to_string(&path).unwrap();
        now.push_str(concurrent);
        fs::write(&path, now).unwrap();
    })
    .unwrap();

    assert!(!report.applied);
    assert!(report
        .skipped_reason
        .as_deref()
        .unwrap()
        .contains("changed while archiving"));
    // The concurrent write is intact, and memory.md was not replaced.
    let now = fs::read_to_string(&path).unwrap();
    assert!(
        now.ends_with(concurrent),
        "a concurrent write must never be lost"
    );
    assert_eq!(now, format!("{content}{concurrent}"));
    // The entries went to the archive first, so nothing is missing anywhere.
    assert!(fs::read_to_string(dir.path().join(ARCHIVE_FILE))
        .unwrap()
        .contains("entry number 1 "));

    // The next run succeeds, without duplicating what is already archived.
    let retry = archive_memory(dir.path(), &tight(400, 100_000), false).unwrap();
    assert!(retry.applied);
    let archive = fs::read_to_string(dir.path().join(ARCHIVE_FILE)).unwrap();
    assert_eq!(archive.matches("entry number 1 ").count(), 1);
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("written while archiving"));
}

#[test]
fn the_nothing_lost_guard_rejects_a_plan_that_drops_a_line() {
    let original = "## A\n- keep\n- drop me\n";
    let moved: Vec<&String> = Vec::new();
    assert!(nothing_lost(original, "## A\n- keep\n- drop me\n", &moved));
    assert!(!nothing_lost(original, "## A\n- keep\n", &moved));
    // A moved entry covers the missing line.
    let covered = "- drop me\n".to_string();
    assert!(nothing_lost(original, "## A\n- keep\n", &[&covered]));
    // Duplicated lines must be duplicated in the result too.
    assert!(!nothing_lost("- x\n- x\n", "- x\n", &moved));
}

#[test]
fn archive_if_oversized_respects_the_trigger() {
    let dir = project_with(&memory(&newest_first(12), &[]));
    let big_trigger = ArchivePolicy {
        trigger_bytes: 1_000_000,
        ..tight(400, 100_000)
    };
    assert!(archive_if_oversized(dir.path(), &big_trigger)
        .unwrap()
        .is_none());
    assert!(!dir.path().join(ARCHIVE_FILE).exists());

    let small_trigger = ArchivePolicy {
        trigger_bytes: 100,
        ..tight(400, 100_000)
    };
    let report = archive_if_oversized(dir.path(), &small_trigger)
        .unwrap()
        .unwrap();
    assert!(report.applied);

    let empty = TempDir::new().unwrap();
    assert!(archive_if_oversized(empty.path(), &small_trigger)
        .unwrap()
        .is_none());
}

// ─── inserting a Change Log entry ────────────────────────────────────────────

#[test]
fn new_entry_goes_to_the_top_of_a_newest_first_log_not_the_end_of_the_file() {
    let content = memory(&newest_first(4), &instincts(2));
    let updated = insert_change_log_entry(&content, "- [2026-11-01] [Claude Kaira]: new work");

    let log_start = updated.find("## Change Log").unwrap();
    let instincts_start = updated.find("## Instincts").unwrap();
    let new_at = updated.find("new work").unwrap();
    assert!(
        log_start < new_at && new_at < instincts_start,
        "must land inside the Change Log"
    );
    // Directly before the previously-newest entry.
    assert!(updated.find("new work").unwrap() < updated.find("entry number 4 ").unwrap());
    // Nothing else changed.
    assert_eq!(
        updated.replace("- [2026-11-01] [Claude Kaira]: new work\n", ""),
        content
    );
}

#[test]
fn new_entry_goes_to_the_bottom_of_an_oldest_first_log() {
    let oldest_first: Vec<String> = newest_first(4).into_iter().rev().collect();
    let content = memory(&oldest_first, &instincts(1));
    let updated = insert_change_log_entry(&content, "- [2026-11-01] [Claude Kaira]: new work");

    let new_at = updated.find("new work").unwrap();
    assert!(new_at > updated.find("entry number 4 ").unwrap());
    assert!(new_at < updated.find("## Instincts").unwrap());
}

#[test]
fn a_missing_change_log_section_is_created() {
    let content = "# T\n\n## Context\n- a\n";
    let updated = insert_change_log_entry(content, "- [2026-11-01] [X]: first");
    assert!(updated.ends_with("## Change Log\n- [2026-11-01] [X]: first\n"));
    assert!(updated.starts_with("# T\n\n## Context\n- a\n"));
}

#[test]
fn inserting_into_an_empty_change_log_section_works() {
    let content = "## Context\n- a\n\n## Change Log\n\n## Instincts\n- r\n";
    let updated = insert_change_log_entry(content, "- [2026-11-01] [X]: first");
    let log = updated.find("## Change Log").unwrap();
    let new_at = updated.find("first").unwrap();
    assert!(log < new_at && new_at < updated.find("## Instincts").unwrap());
}
