use super::DbCmd;

pub fn cmd_db(command: DbCmd, json: bool) {
    let result = match command {
        DbCmd::Check { full } => run_check(full, json),
        DbCmd::Backup { keep } => run_backup(keep, json),
        DbCmd::Checkpoint { truncate } => run_checkpoint(truncate, json),
    };

    if let Err(error) = result {
        if json {
            println!(
                "{}",
                serde_json::json!({ "ok": false, "error": error.to_string() })
            );
        } else {
            eprintln!("Database operation failed: {error:#}");
        }
        std::process::exit(1);
    }
}

fn run_check(full: bool, json: bool) -> anyhow::Result<()> {
    let report = raios_core::db::check_workspace_database(full)?;
    if !report.healthy {
        anyhow::bail!(
            "SQLite {} check failed: {}",
            report.check_mode,
            report.integrity_messages.join("; ")
        );
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("workspace.db maintenance check");
        println!("  Path: {}", report.database_path.display());
        println!(
            "  Integrity ({}): {}",
            report.check_mode,
            report.integrity_messages.join("; ")
        );
        println!("  Database: {}", human_bytes(report.database_bytes));
        println!("  WAL: {}", human_bytes(report.wal_bytes));
        println!("  SHM: {}", human_bytes(report.shm_bytes));
        println!("  Journal: {}", report.journal_mode);
        println!("  Free pages: {}", report.freelist_pages);
        println!("  Managed snapshots: {}", report.snapshot_count);
    }
    Ok(())
}

fn run_backup(keep: usize, json: bool) -> anyhow::Result<()> {
    let report = raios_core::db::backup_workspace_database(keep)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("workspace.db snapshot complete");
        println!("  Snapshot: {}", report.snapshot_path.display());
        println!("  Checksum: {}", report.checksum_path.display());
        println!("  SHA-256: {}", report.sha256);
        println!("  Size: {}", human_bytes(report.snapshot_bytes));
        println!(
            "  Integrity ({}): {}",
            report.check_mode,
            report.integrity_messages.join("; ")
        );
        println!("  Retained snapshots: {}", report.retained_snapshots);
        if !report.pruned_snapshots.is_empty() {
            println!("  Pruned snapshots: {}", report.pruned_snapshots.len());
        }
    }
    Ok(())
}

fn run_checkpoint(truncate: bool, json: bool) -> anyhow::Result<()> {
    let report = raios_core::db::checkpoint_workspace_database(truncate)?;
    if report.busy != 0 {
        anyhow::bail!(
            "checkpoint could not process every frame because the database is busy \
             (WAL frames: {}, checkpointed: {})",
            report.wal_frames,
            report.checkpointed_frames
        );
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("workspace.db WAL checkpoint complete");
        println!("  Mode: {}", report.mode);
        println!("  Busy readers/writers: {}", report.busy);
        println!("  WAL frames: {}", report.wal_frames);
        println!("  Checkpointed frames: {}", report.checkpointed_frames);
        println!(
            "  WAL size: {} -> {}",
            human_bytes(report.wal_bytes_before),
            human_bytes(report.wal_bytes_after)
        );
    }
    Ok(())
}

fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.2} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}
