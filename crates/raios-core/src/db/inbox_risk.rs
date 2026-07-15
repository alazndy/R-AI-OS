//! Risk scoring for the approval inbox.
//!
//! Adds `risk_score` / `risk_label` / `file_impact` / `agent_success_rate` /
//! `suggested_action` on top of the existing `ApprovalInboxRow` read model
//! (`control_plane.rs`), without changing it — every existing consumer of
//! `cp_query_pending_approvals` keeps working unchanged; `tool_get_inbox`,
//! `handle_inbox`, and the TUI Inbox panel switch to the scored variant.
//!
//! The score is a deterministic, hand-weighted heuristic — not a model, not
//! machine-learned. Weights are visible and documented below so the "why"
//! behind any score is always inspectable (same "policy simulator" ethos as
//! `raios policy simulate`: explain the decision, don't just state it).

use rusqlite::{Connection, Result};

use super::agent_stats::cp_agent_stats_for;
use super::control_plane::ApprovalInboxRow;
use super::wf_handoff::HandoffReport;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FileImpact {
    pub files_changed: Option<u32>,
    /// Insertions + deletions for a `handover`'s `git diff --stat`; for a
    /// `file_write` this is a rough line-count delta between old/new content
    /// (not a real diff — counting lines added/removed exactly would need a
    /// full diff algorithm, out of scope here), clearly not a precise number.
    pub lines_changed: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScoredApproval {
    #[serde(flatten)]
    pub approval: ApprovalInboxRow,
    pub risk_score: u8,
    pub risk_label: &'static str,
    pub file_impact: Option<FileImpact>,
    pub agent_success_rate: Option<f64>,
    pub suggested_action: &'static str,
    /// Structured `HandoffReport`, parsed from the artifact's `metadata_json`
    /// when this is a `handover` approval filed with `--report`. `None` for
    /// non-handover approvals and legacy free-text handoffs.
    pub handoff_report: Option<HandoffReport>,
}

/// Parses the last summary line of `git diff --stat` output, e.g.
/// `" 3 files changed, 42 insertions(+), 7 deletions(-)"`. Missing
/// insertions/deletions phrases (e.g. a deletion-only diff) are handled;
/// returns `FileImpact { None, None }` if nothing matches (never panics on
/// unexpected formats — this is best-effort parsing of tool output, not a
/// trusted input format).
pub fn parse_diff_stat(stat: &str) -> FileImpact {
    let files_changed = capture_number(stat, r"(\d+)\s+files?\s+changed");
    let insertions = capture_number(stat, r"(\d+)\s+insertions?\(\+\)").unwrap_or(0);
    let deletions = capture_number(stat, r"(\d+)\s+deletions?\(-\)").unwrap_or(0);
    let lines_changed = if insertions + deletions > 0 { Some(insertions + deletions) } else { None };
    FileImpact { files_changed, lines_changed }
}

fn capture_number(text: &str, pattern: &str) -> Option<u32> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(text)?.get(1)?.as_str().parse().ok()
}

/// Rough line-count delta between a file_write approval's proposed old/new
/// content. Not a real diff — just `|new_lines - old_lines|` plus a floor of
/// 1 for any nonzero byte-level change, so a same-line-count edit still
/// registers as *some* impact instead of silently reporting zero.
fn file_write_impact(original_content: Option<&str>, new_content: &str) -> FileImpact {
    let old_lines = original_content.map(|c| c.lines().count()).unwrap_or(0);
    let new_lines = new_content.lines().count();
    let delta = old_lines.abs_diff(new_lines) as u32;
    let lines_changed = if delta == 0 && Some(new_content) != original_content { 1 } else { delta };
    FileImpact { files_changed: Some(1), lines_changed: Some(lines_changed) }
}

/// Base risk by approval type, then adjusted by file impact and the
/// requesting agent's historical success rate. Scale 0-100, clamped.
///
/// Weights (documented so "why is this High" is always answerable):
/// - Base: handover=15, file_write=30, merge=40, tool_quarantine=50,
///   budget_override=55, network_exception=60, other=35.
/// - `+2` per file touched (capped at 20 files -> +40).
/// - `+1` per 10 lines changed (capped at 500 lines -> +50).
/// - `+30 * (1 - success_rate)` — an agent with a poor track record raises
///   risk on its pending approvals; a spotless record does not itself lower
///   the base score below the type's floor (a clean history doesn't excuse
///   a `network_exception`, it just avoids compounding the risk further).
pub fn score_approval(
    approval_type: &str,
    file_impact: Option<&FileImpact>,
    agent_success_rate: Option<f64>,
) -> (u8, &'static str, &'static str) {
    let base: i32 = match approval_type {
        "handover" => 15,
        "file_write" => 30,
        "merge" => 40,
        "tool_quarantine" => 50,
        "budget_override" => 55,
        "network_exception" => 60,
        _ => 35,
    };

    let mut score = base;
    if let Some(fi) = file_impact {
        if let Some(files) = fi.files_changed {
            score += (files.min(20) * 2) as i32;
        }
        if let Some(lines) = fi.lines_changed {
            score += (lines.min(500) / 10) as i32;
        }
    }
    if let Some(rate) = agent_success_rate {
        score += (30.0 * (1.0 - rate)).round() as i32;
    }

    let score = score.clamp(0, 100) as u8;
    let label = match score {
        0..=24 => "low",
        25..=49 => "medium",
        50..=74 => "high",
        _ => "critical",
    };
    let suggested_action = match label {
        "low" => "approve",
        "medium" => "review",
        _ => "reject or tighten scope",
    };
    (score, label, suggested_action)
}

/// `cp_query_pending_approvals` enriched with risk score, file impact, and
/// the requesting agent's historical success rate.
pub fn cp_query_pending_approvals_scored(conn: &Connection) -> Result<Vec<ScoredApproval>> {
    let mut stmt = conn.prepare(
        "SELECT ap.id, ap.approval_type, ap.reason, ap.status,
                ap.task_id, t.title, ap.requested_at,
                art.metadata_json, ar2.agent_name
         FROM cp_approvals ap
         LEFT JOIN cp_tasks t ON t.id = ap.task_id
         LEFT JOIN cp_artifacts art ON art.id = ap.artifact_id
         LEFT JOIN cp_agent_runs ar2 ON ar2.id = ap.agent_run_id
         WHERE ap.status = 'pending'
         ORDER BY ap.requested_at DESC",
    )?;

    struct Raw {
        approval: ApprovalInboxRow,
        metadata_json: Option<String>,
        agent_name: Option<String>,
    }

    let rows: Vec<Raw> = stmt
        .query_map([], |row| {
            Ok(Raw {
                approval: ApprovalInboxRow {
                    id: row.get(0)?,
                    approval_type: row.get(1)?,
                    reason: row.get(2)?,
                    status: row.get(3)?,
                    task_id: row.get(4)?,
                    task_title: row.get(5)?,
                    requested_at: row.get(6)?,
                },
                metadata_json: row.get(7)?,
                agent_name: row.get(8)?,
            })
        })?
        .collect::<Result<_>>()?;

    let mut out = Vec::with_capacity(rows.len());
    for raw in rows {
        let metadata_value = raw
            .metadata_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());

        let file_impact = metadata_value
            .as_ref()
            .and_then(|meta| derive_file_impact(&raw.approval.approval_type, meta));

        let handoff_report: Option<HandoffReport> = metadata_value
            .as_ref()
            .and_then(|meta| meta.get("report"))
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok());

        let agent_success_rate = match &raw.agent_name {
            Some(name) => cp_agent_stats_for(conn, name)?.and_then(|s| s.success_rate),
            None => None,
        };

        let (risk_score, risk_label, suggested_action) =
            score_approval(&raw.approval.approval_type, file_impact.as_ref(), agent_success_rate);

        out.push(ScoredApproval {
            approval: raw.approval,
            risk_score,
            risk_label,
            file_impact,
            agent_success_rate,
            suggested_action,
            handoff_report,
        });
    }
    Ok(out)
}

fn derive_file_impact(approval_type: &str, meta: &serde_json::Value) -> Option<FileImpact> {
    match approval_type {
        "handover" => meta["diff_stat"].as_str().map(parse_diff_stat),
        "file_write" => {
            let new_content = meta["new_content"].as_str()?;
            let original_content = meta["original_content"].as_str();
            Some(file_write_impact(original_content, new_content))
        }
        _ => None,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diff_stat_reads_files_insertions_deletions() {
        let fi = parse_diff_stat(" src/db.rs | 12 ++++++++----\n 1 file changed, 8 insertions(+), 4 deletions(-)");
        assert_eq!(fi.files_changed, Some(1));
        assert_eq!(fi.lines_changed, Some(12));
    }

    #[test]
    fn parse_diff_stat_handles_insertion_only_diff() {
        let fi = parse_diff_stat("2 files changed, 30 insertions(+)");
        assert_eq!(fi.files_changed, Some(2));
        assert_eq!(fi.lines_changed, Some(30));
    }

    #[test]
    fn parse_diff_stat_empty_string_is_none() {
        let fi = parse_diff_stat("");
        assert_eq!(fi.files_changed, None);
        assert_eq!(fi.lines_changed, None);
    }

    #[test]
    fn file_write_impact_new_file_counts_all_lines() {
        let fi = file_write_impact(None, "line1\nline2\nline3");
        assert_eq!(fi.files_changed, Some(1));
        assert_eq!(fi.lines_changed, Some(3));
    }

    #[test]
    fn file_write_impact_same_line_count_edit_still_registers() {
        let fi = file_write_impact(Some("a\nb\nc"), "a\nX\nc");
        assert_eq!(fi.lines_changed, Some(1), "same line count but content changed must not report 0");
    }

    #[test]
    fn file_write_impact_identical_content_is_zero() {
        let fi = file_write_impact(Some("a\nb"), "a\nb");
        assert_eq!(fi.lines_changed, Some(0));
    }

    #[test]
    fn score_approval_handover_is_lower_base_than_network_exception() {
        let (handover_score, ..) = score_approval("handover", None, None);
        let (network_score, ..) = score_approval("network_exception", None, None);
        assert!(handover_score < network_score);
    }

    #[test]
    fn score_approval_scales_with_file_impact() {
        let small = FileImpact { files_changed: Some(1), lines_changed: Some(5) };
        let large = FileImpact { files_changed: Some(15), lines_changed: Some(400) };
        let (small_score, ..) = score_approval("file_write", Some(&small), None);
        let (large_score, ..) = score_approval("file_write", Some(&large), None);
        assert!(large_score > small_score);
    }

    #[test]
    fn score_approval_poor_agent_history_raises_risk() {
        let (good_score, ..) = score_approval("file_write", None, Some(1.0));
        let (bad_score, ..) = score_approval("file_write", None, Some(0.0));
        assert!(bad_score > good_score);
    }

    #[test]
    fn score_approval_labels_and_actions_are_consistent() {
        let (score, label, action) = score_approval("network_exception", None, Some(0.0));
        assert!(score >= 50);
        assert!(label == "high" || label == "critical");
        assert_eq!(action, "reject or tighten scope");

        let (score, label, action) = score_approval("handover", None, Some(1.0));
        assert!(score < 25, "score={score}");
        assert_eq!(label, "low");
        assert_eq!(action, "approve");
    }

    #[test]
    fn score_approval_never_exceeds_bounds() {
        let extreme = FileImpact { files_changed: Some(u32::MAX), lines_changed: Some(u32::MAX) };
        let (score, ..) = score_approval("network_exception", Some(&extreme), Some(0.0));
        assert_eq!(score, 100);
    }
}
