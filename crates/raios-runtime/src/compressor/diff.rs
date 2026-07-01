//! Unified diff distillation.
//!
//! Retains all added/removed lines and up to `MAX_CONTEXT` surrounding context
//! lines per change block. Caps hunk output at `MAX_HUNK_LINES` to prevent
//! runaway large diffs from flooding agent context.

const MAX_CONTEXT: usize = 2;
const MAX_HUNK_LINES: usize = 150;

/// Distil a unified diff, keeping signal lines and trimming excess context.
///
/// Returns the compressed diff. If the input is already compact (≤ the output
/// size) the caller should fall back to the original.
pub fn distil(diff: &str) -> String {
    let mut out: Vec<String> = Vec::with_capacity(diff.lines().count() / 2);
    let mut in_hunk = false;
    let mut context_after: usize = 0;
    let mut pending: Vec<String> = Vec::new();
    let mut hunk_lines: usize = 0;
    let mut omitted: usize = 0;

    for line in diff.lines() {
        // ── File header ───────────────────────────────────────────────────────
        if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("Binary files")
            || line.starts_with("new file mode")
            || line.starts_with("deleted file mode")
        {
            flush_omitted(&mut out, &mut omitted);
            flush_pending(&mut out, &mut pending);
            out.push(line.to_string());
            in_hunk = false;
            hunk_lines = 0;
            continue;
        }

        // ── Hunk header ───────────────────────────────────────────────────────
        if line.starts_with("@@") {
            flush_omitted(&mut out, &mut omitted);
            flush_pending(&mut out, &mut pending);
            out.push(line.to_string());
            in_hunk = true;
            context_after = 0;
            hunk_lines = 0;
            continue;
        }

        if !in_hunk {
            out.push(line.to_string());
            continue;
        }

        // ── Hard cap per hunk ─────────────────────────────────────────────────
        if hunk_lines >= MAX_HUNK_LINES {
            omitted += 1;
            continue;
        }

        let is_add = line.starts_with('+');
        let is_del = line.starts_with('-');

        if is_add || is_del {
            // Emit buffered pre-change context then the change itself
            flush_pending(&mut out, &mut pending);
            out.push(line.to_string());
            context_after = MAX_CONTEXT;
            hunk_lines += 1;
        } else if line.starts_with(' ') {
            if context_after > 0 {
                // Post-change context
                out.push(line.to_string());
                context_after -= 1;
                hunk_lines += 1;
            } else {
                // Pre-change context: keep a sliding window of MAX_CONTEXT lines
                pending.push(line.to_string());
                if pending.len() > MAX_CONTEXT {
                    pending.remove(0);
                    omitted += 1;
                }
            }
        } else {
            // Unexpected line inside hunk (e.g. "No newline at end of file")
            out.push(line.to_string());
            hunk_lines += 1;
        }
    }

    flush_omitted(&mut out, &mut omitted);

    out.join("\n")
}

fn flush_pending(out: &mut Vec<String>, pending: &mut Vec<String>) {
    out.append(pending);
}

fn flush_omitted(out: &mut Vec<String>, omitted: &mut usize) {
    if *omitted > 0 {
        out.push(format!("  … {} context lines omitted", *omitted));
        *omitted = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_diff_returns_empty() {
        assert_eq!(distil(""), "");
    }

    #[test]
    fn header_lines_are_preserved() {
        let diff =
            "diff --git a/foo.rs b/foo.rs\nindex abc..def 100644\n--- a/foo.rs\n+++ b/foo.rs";
        let result = distil(diff);
        assert!(result.contains("diff --git"), "got: {result}");
        assert!(result.contains("--- a/foo.rs"), "got: {result}");
        assert!(result.contains("+++ b/foo.rs"), "got: {result}");
    }

    #[test]
    fn added_and_removed_lines_kept() {
        let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -1,4 +1,4 @@\n context\n-old line\n+new line\n context";
        let result = distil(diff);
        assert!(result.contains("-old line"), "got: {result}");
        assert!(result.contains("+new line"), "got: {result}");
    }

    #[test]
    fn excess_context_is_omitted() {
        // Build a hunk with 10 context lines before a change
        let mut diff =
            "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -1,12 +1,12 @@\n".to_string();
        for _ in 0..10 {
            diff.push_str(" context line\n");
        }
        diff.push_str("-removed\n+added\n");
        let result = distil(&diff);
        // Should only keep MAX_CONTEXT (2) lines before the change
        let context_count = result
            .lines()
            .filter(|l| l.trim() == "context line")
            .count();
        assert!(
            context_count <= MAX_CONTEXT * 2 + 1,
            "too many context lines ({context_count}), got: {result}"
        );
        assert!(
            result.contains("omitted"),
            "should note omitted lines, got: {result}"
        );
    }

    #[test]
    fn hunk_cap_is_enforced() {
        let mut diff =
            "diff --git a/big.rs b/big.rs\n--- a/big.rs\n+++ b/big.rs\n@@ -1,200 +1,200 @@\n"
                .to_string();
        for i in 0..200 {
            diff.push_str(&format!("-old {i}\n+new {i}\n"));
        }
        let result = distil(&diff);
        let change_lines = result
            .lines()
            .filter(|l| l.starts_with('+') || l.starts_with('-'))
            .count();
        assert!(
            change_lines <= MAX_HUNK_LINES + 4,
            "cap not enforced, got {change_lines} change lines"
        );
        assert!(result.contains("omitted"), "should note omitted lines");
    }
}
