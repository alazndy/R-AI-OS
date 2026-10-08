//! The daily ANKA refresh pair. Units are never checked in: they are
//! generated from live values at install time (the `hub.rs` convention), so
//! the absolute binary path, `UMask`, schedule, and limits always describe
//! the binary and machine that install them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

const SERVICE_UNIT: &str = "raios-anka-index.service";
const TIMER_UNIT: &str = "raios-anka-index.timer";
/// Local-time daily schedule. `Persistent=true` catches up a missed run on the
/// next activation instead of silently skipping a day, and the randomized
/// delay spreads load without letting it grow past "small".
const ON_CALENDAR: &str = "*-*-* 04:00:00";
const RANDOMIZED_DELAY: &str = "15min";
/// Phase 4 measured 11.16s / ~115 MiB; these bounds leave ~25x headroom and
/// stop a pathological run instead of letting it burn unattended. For
/// `Type=oneshot` the operative time knob is `TimeoutStartSec` (a completed
/// oneshot is no longer running, so `RuntimeMaxSec` would never bite).
const TIME_LIMIT: &str = "300s";
const MEMORY_LIMIT: &str = "512M";

/// Pure generator — pinned by tests, not by a checked-in file. `binary` must
/// be the absolute path of the tested build (`current_exe()` at install).
fn service_unit(binary: &Path) -> String {
    format!(
        "[Unit]
Description=R-AI-OS ANKA transcript index refresh (policy-filtered, read-only sources)

[Service]
Type=oneshot
UMask=0077
ExecStart={binary} anka index
TimeoutStartSec={time_limit}
MemoryMax={memory_limit}
StandardOutput=journal
StandardError=journal
",
        binary = binary.display(),
        time_limit = TIME_LIMIT,
        memory_limit = MEMORY_LIMIT,
    )
}

/// Pure generator for the schedule half of the pair.
fn timer_unit() -> String {
    format!(
        "[Unit]
Description=Daily RAIOS ANKA transcript index refresh

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

/// `systemctl --user …` with captured stderr, so a failed rollout step names
/// its own cause instead of printing a bare exit code.
fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("could not run `systemctl --user`")?;
    if !output.status.success() {
        bail!(
            "`systemctl --user {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn user_unit_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not resolve the home directory")?;
    let dir = home.join(".config/systemd/user");
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    Ok(dir)
}

/// Install and enable the daily refresh timer.
///
/// Fail-closed gate: the timer is only scheduled once the privacy policy is
/// initialized — a missing/privacy error keeps the timer uninstalled, so no
/// unattended index run can ever start without consent.
pub(super) fn install() -> Result<serde_json::Value> {
    let status = raios_runtime::anka::status()?;
    let dto = serde_json::to_value(raios_runtime::anka::status_dto(status))?;
    if dto["policy"]["initialized"].as_bool() != Some(true) {
        bail!(
            "ANKA policy is not initialized — run `raios anka policy-init --home <keep|exclude>` \
             before installing the refresh timer"
        );
    }

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
    }))
}

/// Rollback: stop and remove the timer pair. The privacy policy, the
/// tombstones, and the cache are retained by construction — this function
/// never touches them, and no systemctl failure can change that. Disable and
/// reload are best-effort (the unit files may not exist yet); their outcome is
/// reported rather than swallowed.
pub(super) fn uninstall() -> Result<serde_json::Value> {
    let disable = run_systemctl(&["disable", "--now", TIMER_UNIT]);

    let unit_dir = dirs::home_dir()
        .context("could not resolve the home directory")?
        .join(".config/systemd/user");
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
        "retained_by_design": ["privacy policy", "tombstones", "ANKA cache"],
    }))
}

/// `Ok(true)` — deleted now; `Ok(false)` — was already absent (uninstall stays
/// idempotent); an error propagates as a rollback failure the operator must
/// see.
fn remove_unit(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("could not remove {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_unit_pins_the_absolute_binary_umask_and_measured_bounds() {
        let content = service_unit(Path::new("/home/alaz/.local/bin/raios"));
        assert!(
            content.contains("ExecStart=/home/alaz/.local/bin/raios anka index"),
            "the unit must point at an absolute binary path, not a PATH lookup"
        );
        assert!(content.contains("Type=oneshot"));
        assert!(content.contains("UMask=0077"));
        assert!(content.contains("TimeoutStartSec=300s"));
        assert!(content.contains("MemoryMax=512M"));
        assert!(
            content.contains("StandardError=journal"),
            "failures must land in the journal for status/journalctl visibility"
        );
    }

    #[test]
    fn timer_unit_schedules_daily_with_persistent_catchup_and_randomized_delay() {
        let content = timer_unit();
        assert!(content.contains("OnCalendar=*-*-* 04:00:00"));
        assert!(content.contains("Persistent=true"));
        assert!(content.contains("RandomizedDelaySec=15min"));
        assert!(content.contains("Unit=raios-anka-index.service"));
        assert!(content.contains("WantedBy=timers.target"));
    }

    #[test]
    fn remove_unit_reports_absent_units_without_failing() {
        let missing = std::env::temp_dir().join("raios-anka-missing-unit.service");
        let _ = fs::remove_file(&missing);
        assert!(!remove_unit(&missing).expect("absence is not a failure"));

        let present = std::env::temp_dir().join("raios-anka-present-unit.service");
        fs::write(&present, "[Unit]\n").expect("fixture write");
        assert!(remove_unit(&present).expect("deletion works"));
        assert!(!remove_unit(&present).expect("already gone is not a failure"));
    }
}
