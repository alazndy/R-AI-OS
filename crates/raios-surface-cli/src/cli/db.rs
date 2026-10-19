use super::DbCmd;

pub fn cmd_db(command: DbCmd, json: bool) {
    let result = match command {
        DbCmd::RepairOrphanContracts { apply } => run_repair_orphan_contracts(apply, json),
        DbCmd::ReconcileSessions {
            apply,
            include_legacy,
        } => run_reconcile_sessions(apply, include_legacy, json),
        DbCmd::PruneSearch { apply } => run_prune_search(apply, json),
        DbCmd::NormalizeBm25 { apply } => {
            raios_runtime::search::bm25_storage::normalize_bm25(&raios_core::db::db_path(),apply).and_then(|report| {
                if json { println!("{}",serde_json::to_string_pretty(&report)?); }
                else {
                    println!("BM25 normalization: {} legacy postings, {} distinct lines",report.legacy_postings,report.distinct_legacy_lines);
                    println!("Estimated snippet payload savings: {}",human_bytes(report.estimated_payload_savings));
                    println!("Converted postings: {}",report.converted_postings);
                    if !apply { println!("Read-only preview. Back up and update CLI/daemon before --apply. Compaction is a separate operation."); }
                }
                Ok(())
            })
        }
        DbCmd::Check { full } => run_check(full, json),
        DbCmd::Backup { keep } => run_backup(keep, json),
        DbCmd::Checkpoint { truncate } => run_checkpoint(truncate, json),
        DbCmd::BackupTimerInstall => run_backup_timer_install(json),
        DbCmd::BackupTimerUninstall => run_backup_timer_uninstall(json),
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

fn run_reconcile_sessions(apply: bool, include_legacy: bool, json: bool) -> anyhow::Result<()> {
    let report = raios_runtime::wrapper_liveness::reconcile_sessions(
        &raios_core::db::db_path(),
        apply,
        include_legacy,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Wrapper recovery: {} candidates, {} closed, {} live/unverified",
            report.candidates.len(),
            report.closed,
            report.kept_alive_or_unverified
        );
        if !apply {
            println!("Read-only preview. Review candidates and back up before using --apply.");
        }
    }
    Ok(())
}

fn run_repair_orphan_contracts(apply: bool, json: bool) -> anyhow::Result<()> {
    let report = raios_core::db::repair_orphan_contracts(&raios_core::db::db_path(), apply)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Contract recovery: {} candidates, {} detached",
            report.candidates.len(),
            report.detached
        );
        if !apply {
            println!("Read-only preview. Review and back up before using --apply.");
        }
    }
    Ok(())
}

fn run_prune_search(apply: bool, json: bool) -> anyhow::Result<()> {
    let report = raios_runtime::search::cache_maintenance::prune_search_cache(
        &raios_core::db::db_path(),
        apply,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Search cache maintenance ({})", report.scanned_at_utc);
        println!("  Mode: {}", if apply { "apply" } else { "dry-run" });
        println!("  BM25: {:?}", report.bm25);
        println!("  Trigram: {:?}", report.trigram);
        if !apply {
            println!("  No rows deleted. Use --apply after reviewing and backing up.");
        }
    }
    Ok(())
}

fn run_check(full: bool, json: bool) -> anyhow::Result<()> {
    let report = raios_core::db::check_workspace_database(full)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        if !report.healthy {
            std::process::exit(1);
        }
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
        println!(
            "  Foreign-key violations: {}",
            report.foreign_key_violations.len()
        );
    }
    if !report.healthy {
        anyhow::bail!(
            "SQLite {} check: {}; foreign-key violations: {}",
            report.check_mode,
            report.integrity_messages.join("; "),
            report.foreign_key_violations.len()
        );
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

fn run_backup_timer_install(json: bool) -> anyhow::Result<()> {
    let report = super::db_backup_timer::install()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("workspace.db backup timer installed and enabled");
        println!("  Service: {}", report["service"].as_str().unwrap_or("?"));
        println!("  Timer: {}", report["timer"].as_str().unwrap_or("?"));
        println!(
            "  Schedule: {} local, persistent catch-up, randomized by 15 min",
            report["on_calendar"].as_str().unwrap_or("?")
        );
        println!("  Retention: {} snapshots", report["retention"]);
        println!("  WAL: passive checkpoint after each successful backup");
        println!();
        println!("No backup has run yet; `raios db backup` takes one now.");
    }
    Ok(())
}

fn run_backup_timer_uninstall(json: bool) -> anyhow::Result<()> {
    let report = super::db_backup_timer::uninstall()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("workspace.db backup timer removed");
        println!(
            "  Disabled: {}{}",
            report["disabled"],
            report["disable_detail"]
                .as_str()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default()
        );
        println!("  Service removed: {}", report["service_removed"]);
        println!("  Timer removed: {}", report["timer_removed"]);
        println!("  Retained: workspace.db, managed snapshots, SHA-256 sidecars");
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
