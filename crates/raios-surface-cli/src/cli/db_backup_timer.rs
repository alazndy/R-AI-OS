//! The daily `workspace.db` backup pair, plus the passive WAL checkpoint that
//! follows each successful run. Units are never checked in: they are generated
//! from live values at install time (the `hub.rs` convention), so the absolute
//! binary path, retention, schedule, and limits always describe the binary and
//! machine that install them.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::systemd_user::{quote_exec_path, remove_unit, run_systemctl, user_unit_dir};

const SERVICE_UNIT: &str = "raios-db-backup.service";
const TIMER_UNIT: &str = "raios-db-backup.timer";
/// Local-time daily schedule, half an hour before the ANKA refresh: the
/// snapshot describes the database as it was before the daily index writes.
/// `Persistent=true` catches up a missed run on the next activation instead of
/// silently skipping a day, and the randomized delay spreads load without
/// letting it grow past "small".
const ON_CALENDAR: &str = "*-*-* 03:30:00";
const RANDOMIZED_DELAY: &str = "15min";
/// A snapshot copies the whole database through SQLite's online backup API and
/// then reads it back for the integrity check, so the ceiling scales with the
/// database (1.7 GiB at the time of the durability report) and a slow disk.
/// Thirty minutes is generous for a daily 03:30 run; the limit exists to stop
/// a wedged run, not to bound a healthy one. For `Type=oneshot` the operative
/// time knob is `TimeoutStartSec` (a completed oneshot is no longer running,
/// so `RuntimeMaxSec` would never bite).
const TIME_LIMIT: &str = "1800s";
const MEMORY_LIMIT: &str = "512M";

/// Pure generator — pinned by tests, not by a checked-in file. `binary` must
/// be the absolute path of the tested build (`current_exe()` at install).
///
/// Retention is the CLI's own default constant, so an installed unit and a
/// manual `raios db backup` prune to the same set.
///
/// The checkpoint is an `ExecStartPost=` line prefixed with `-`: it runs only
/// after a successful backup, and a busy database (the live daemon's readers
/// can hold the checkpoint horizon) is logged to the journal instead of
/// failing the backup run that already succeeded. It is the passive form — no
/// `--truncate`.
fn service_unit(binary: &Path) -> String {
    let binary = quote_exec_path(binary);
    format!(
        "[Unit]
Description=R-AI-OS workspace.db online backup

[Service]
Type=oneshot
UMask=0077
ExecStart={binary} db backup --keep {retention}
ExecStartPost=-{binary} db checkpoint
TimeoutStartSec={time_limit}
MemoryMax={memory_limit}
StandardOutput=journal
StandardError=journal
",
        retention = raios_core::db::DEFAULT_SNAPSHOT_RETENTION,
        time_limit = TIME_LIMIT,
        memory_limit = MEMORY_LIMIT,
    )
}

/// Pure generator for the schedule half of the pair.
fn timer_unit() -> String {
    format!(
        "[Unit]
Description=Daily RAIOS workspace.db backup

[Timer]
OnCalendar={on_calendar}
Persistent=true
RandomizedDelaySec={delay}
Unit={service}

[Install]
WantedBy=timers.target
",
        on_calendar = ON_CALENDAR,
        delay = RANDOMIZED_DELAY,
        service = SERVICE_UNIT,
    )
}

/// Install and enable the daily backup timer.
///
/// Idempotent by construction: the same generated text is rewritten and
/// `enable --now` is a no-op for an enabled timer, so re-running is safe.
/// Nothing is backed up here — the first run happens on the schedule, or when
/// the operator asks for one with `raios db backup`.
pub(super) fn install() -> Result<serde_json::Value> {
    let binary = std::env::current_exe().context("could not resolve the running binary path")?;
    let unit_dir = user_unit_dir()?;
    let service_path = unit_dir.join(SERVICE_UNIT);
    let timer_path = unit_dir.join(TIMER_UNIT);

    fs::write(&service_path, service_unit(&binary))
        .with_context(|| format!("could not write {}", service_path.display()))?;
    fs::write(&timer_path, timer_unit())
        .with_context(|| format!("could not write {}", timer_path.display()))?;

    run_systemctl(&["daemon-reload"]).context("units were written but could not be reloaded")?;
    run_systemctl(&["enable", "--now", TIMER_UNIT])
        .with_context(|| format!("units were written but {TIMER_UNIT} could not be enabled"))?;

    Ok(serde_json::json!({
        "binary": binary.display().to_string(),
        "service": service_path.display().to_string(),
        "timer": timer_path.display().to_string(),
        "enabled": true,
        "on_calendar": ON_CALENDAR,
        "retention": raios_core::db::DEFAULT_SNAPSHOT_RETENTION,
        "checkpoint": "passive, after each successful backup",
    }))
}

/// Rollback: stop and remove the timer pair. The database, its snapshots, and
/// their checksums are retained by construction — this function never touches
/// them, and no systemctl failure can change that. Disable and reload are
/// best-effort (the unit files may not exist yet); their outcome is reported
/// rather than swallowed.
pub(super) fn uninstall() -> Result<serde_json::Value> {
    let disable = run_systemctl(&["disable", "--now", TIMER_UNIT]);

    let unit_dir = user_unit_dir()?;
    // Attempt both removals before propagating, so one failure never strands
    // the other unit file in place.
    let removed_service = remove_unit(&unit_dir.join(SERVICE_UNIT));
    let removed_timer = remove_unit(&unit_dir.join(TIMER_UNIT));
    let reload = run_systemctl(&["daemon-reload"]);

    let service_removed = removed_service?;
    let timer_removed = removed_timer?;

    Ok(serde_json::json!({
        "disabled": disable.is_ok(),
        "disable_detail": disable.err().map(|error| error.to_string()),
        "service_removed": service_removed,
        "timer_removed": timer_removed,
        "reload_ok": reload.is_ok(),
        "retained_by_design": ["workspace.db", "managed snapshots", "SHA-256 sidecars"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_unit_pins_the_absolute_binary_umask_and_bounded_limits() {
        let content = service_unit(Path::new("/home/alaz/.local/bin/raios"));
        assert!(
            content.contains("ExecStart=\"/home/alaz/.local/bin/raios\" db backup --keep 3"),
            "the unit must point at an absolute binary path, quoted as one token"
        );
        assert!(content.contains("Type=oneshot"));
        assert!(content.contains("UMask=0077"));
        assert!(content.contains("TimeoutStartSec=1800s"));
        assert!(content.contains("MemoryMax=512M"));
        assert!(
            content.contains("StandardError=journal"),
            "failures must land in the journal for status/journalctl visibility"
        );
    }

    /// The retention baked into the unit must be the same constant a manual
    /// `raios db backup` defaults to; otherwise an installed timer would prune
    /// a different set than the documented command.
    #[test]
    fn the_generated_retention_matches_the_cli_default() {
        let content = service_unit(Path::new("/home/alaz/.local/bin/raios"));
        let expected = format!(
            "db backup --keep {}",
            raios_core::db::DEFAULT_SNAPSHOT_RETENTION
        );
        assert!(
            content.contains(&expected),
            "the unit's --keep must come from DEFAULT_SNAPSHOT_RETENTION"
        );
    }

    /// The checkpoint is the passive form, runs only after a successful
    /// backup, and a busy database must not mark that successful backup failed.
    #[test]
    fn the_checkpoint_follows_the_backup_passively_and_tolerates_busy() {
        let content = service_unit(Path::new("/home/alaz/.local/bin/raios"));
        let line = content
            .lines()
            .find(|line| line.starts_with("ExecStartPost="))
            .expect("the unit must checkpoint the WAL after the backup");
        assert_eq!(
            line, "ExecStartPost=-\"/home/alaz/.local/bin/raios\" db checkpoint",
            "the `-` prefix keeps a busy checkpoint from failing the backup, \
             and the absence of --truncate keeps it passive"
        );
        assert!(!line.contains("--truncate"));
    }

    #[test]
    fn timer_unit_schedules_daily_with_persistent_catchup_and_randomized_delay() {
        let content = timer_unit();
        assert!(content.contains("OnCalendar=*-*-* 03:30:00"));
        assert!(content.contains("Persistent=true"));
        assert!(content.contains("RandomizedDelaySec=15min"));
        assert!(content.contains("Unit=raios-db-backup.service"));
        assert!(content.contains("WantedBy=timers.target"));
    }

    /// A timer whose `Unit=` and service file name drift apart schedules
    /// nothing; both halves come from the same constants here.
    #[test]
    fn the_timer_activates_the_generated_service() {
        let service = service_unit(Path::new("/home/alaz/.local/bin/raios"));
        assert!(!service.is_empty());
        assert!(timer_unit().contains(&format!("Unit={SERVICE_UNIT}")));
        assert!(SERVICE_UNIT.ends_with(".service"));
        assert!(TIMER_UNIT.ends_with(".timer"));
    }

    /// Paths the unit parser would otherwise split or expand must survive
    /// verbatim in both exec lines.
    #[test]
    fn exec_lines_quote_paths_that_the_unit_parser_would_otherwise_split() {
        let content = service_unit(Path::new("/home/John Smith/raios 100%"));
        assert!(
            content.contains("ExecStart=\"/home/John Smith/raios 100%%\" db backup"),
            "the binary path must be one quoted token with specifiers escaped"
        );
        assert!(
            content.contains("ExecStartPost=-\"/home/John Smith/raios 100%%\" db checkpoint"),
            "the checkpoint line uses the same escaped token"
        );
    }
}
