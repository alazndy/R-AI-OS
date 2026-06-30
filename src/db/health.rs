use rusqlite::{params, Connection, Result};
// ─── Health cache ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn upsert_health(
    conn: &Connection,
    project_id: i64,
    compliance_grade: &str,
    compliance_score: Option<u8>,
    security_grade: Option<&str>,
    security_score: Option<u8>,
    security_issues: usize,
    security_critical: usize,
    git_dirty: bool,
    has_memory: bool,
    has_sigmap: bool,
    remote_url: Option<&str>,
    refactor_grade: &str,
    refactor_score: u8,
    refactor_high: usize,
) -> Result<()> {
    conn.execute(
        "INSERT INTO health_cache
            (project_id, compliance_grade, compliance_score, security_grade, security_score,
             security_issues, security_critical, git_dirty, has_memory, has_sigmap, remote_url,
             refactor_grade, refactor_score, refactor_high, scanned_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, datetime('now'))
         ON CONFLICT(project_id) DO UPDATE SET
             compliance_grade=excluded.compliance_grade,
             compliance_score=excluded.compliance_score,
             security_grade=COALESCE(excluded.security_grade, security_grade),
             security_score=COALESCE(excluded.security_score, security_score),
             security_issues=excluded.security_issues,
             security_critical=excluded.security_critical,
             git_dirty=excluded.git_dirty,
             has_memory=excluded.has_memory,
             has_sigmap=excluded.has_sigmap,
             remote_url=COALESCE(excluded.remote_url, remote_url),
             refactor_grade=excluded.refactor_grade,
             refactor_score=excluded.refactor_score,
             refactor_high=excluded.refactor_high,
             scanned_at=datetime('now')",
        params![
            project_id,
            compliance_grade,
            compliance_score.map(|s| s as i64),
            security_grade,
            security_score.map(|s| s as i64),
            security_issues as i64,
            security_critical as i64,
            git_dirty as i64,
            has_memory as i64,
            has_sigmap as i64,
            remote_url,
            refactor_grade,
            refactor_score as i64,
            refactor_high as i64,
        ],
    )?;
    Ok(())
}

// ─── Stats query ─────────────────────────────────────────────────────────────

pub struct PortfolioStats {
    pub total: i64,
    pub active: i64,
    pub archived: i64,
    pub dirty: i64,
    pub no_memory: i64,
    pub no_sigmap: i64,
    pub no_github: i64,
    pub avg_compliance: f64,
    pub avg_security: f64,
    pub grade_a: i64,
    pub grade_b: i64,
    pub grade_c: i64,
    pub grade_d: i64,
}

pub fn query_stats(conn: &Connection) -> Result<PortfolioStats> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE status = 'active'",
        [],
        |r| r.get(0),
    )?;
    let archived: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE status IN ('archived','legacy')",
        [],
        |r| r.get(0),
    )?;
    let dirty: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE git_dirty = 1",
        [],
        |r| r.get(0),
    )?;
    let no_memory: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE has_memory = 0",
        [],
        |r| r.get(0),
    )?;
    let no_sigmap: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE has_sigmap = 0",
        [],
        |r| r.get(0),
    )?;
    let no_github: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE github IS NULL",
        [],
        |r| r.get(0),
    )?;
    let avg_compliance: f64 = conn.query_row(
        "SELECT COALESCE(AVG(compliance_score), 0) FROM health_cache",
        [],
        |r| r.get(0),
    )?;
    let avg_security: f64 = conn.query_row(
        "SELECT COALESCE(AVG(security_score), 0) FROM health_cache",
        [],
        |r| r.get(0),
    )?;
    let grade_a: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'A'",
        [],
        |r| r.get(0),
    )?;
    let grade_b: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'B'",
        [],
        |r| r.get(0),
    )?;
    let grade_c: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'C'",
        [],
        |r| r.get(0),
    )?;
    let grade_d: i64 = conn.query_row("SELECT COUNT(*) FROM health_cache WHERE compliance_grade NOT IN ('A','B','C') AND compliance_grade != '-'", [], |r| r.get(0))?;

    Ok(PortfolioStats {
        total,
        active,
        archived,
        dirty,
        no_memory,
        no_sigmap,
        no_github,
        avg_compliance,
        avg_security,
        grade_a,
        grade_b,
        grade_c,
        grade_d,
    })
}

