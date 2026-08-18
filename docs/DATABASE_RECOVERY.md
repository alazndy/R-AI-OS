# workspace.db Maintenance and Recovery

R-AI-OS stores its durable control plane, curated memory, search indexes, and
audit ledger in `~/.config/raios/workspace.db`. Treat the file and its `-wal`
and `-shm` companions as one live database while `aiosd` is running.

## Routine checks

```bash
raios db check
raios db check --full
raios verify-chain
```

The default check uses SQLite `quick_check`; `--full` uses the much slower
`integrity_check`. Audit-chain verification is separate because it validates
application-level hash linkage rather than SQLite page structure.

## WAL maintenance

```bash
raios db checkpoint
raios db checkpoint --truncate
```

The default passive checkpoint does not wait for readers or writers.
`--truncate` requests a complete checkpoint and a zero-length WAL. A busy
result is an error, not a claim that maintenance completed. Checkpointing does
not delete logical data and is not a substitute for retention policies.

## Online snapshots

```bash
raios db backup
raios db backup --keep 5
sha256sum -c ~/.config/raios/backups/<snapshot>.db.sha256
```

The backup command:

- uses SQLite's online backup API while `aiosd` remains available;
- checks the live source and completed snapshot;
- creates `~/.config/raios/backups` as mode 0700 and snapshot/checksum files as
  mode 0600 on Unix;
- records a SHA-256 sidecar and fsyncs durable files before reporting success;
- retains three snapshots by default, with an enforced range of 1 through 10;
- prunes only regular files matching its own `workspace-*.db` naming contract
  and their checksum sidecars.

Snapshots contain the canonical database state, including committed WAL
content captured by the online backup API. They do not need a copied `-wal` or
`-shm` file.

## Offline restore

Restore changes shared state for every R-AI-OS surface. It requires explicit
owner approval and must never run while `aiosd`, the tray, or another process
has the database open.

1. Stop writers and confirm the daemon is down:

   ```bash
   systemctl --user stop raios-tray.service aiosd.service
   ss -ltnp
   ```

2. Verify the selected snapshot before touching the live files:

   ```bash
   cd ~/.config/raios/backups
   sha256sum -c <snapshot>.db.sha256
   sqlite3 -readonly <snapshot>.db "PRAGMA integrity_check;"
   ```

3. Preserve the current database and both companions under one timestamp.
   Do not delete them; they are the rollback set:

   ```bash
   stamp="$(date -u +%Y%m%dT%H%M%SZ)"
   cd ~/.config/raios
   mv workspace.db "workspace.db.failed-$stamp"
   test ! -e workspace.db-wal || mv workspace.db-wal "workspace.db-wal.failed-$stamp"
   test ! -e workspace.db-shm || mv workspace.db-shm "workspace.db-shm.failed-$stamp"
   ```

4. Install the verified snapshot without reusing old WAL/SHM companions:

   ```bash
   install -m 600 "backups/<snapshot>.db" workspace.db
   sync
   ```

5. Start the daemon, then validate both SQLite and the audit ledger:

   ```bash
   systemctl --user start aiosd.service raios-tray.service
   raios db check --full
   raios verify-chain
   ```

If either validation fails, stop the services again, move the restored file
aside, and move the timestamped rollback set back to its original names. Never
mix WAL/SHM files from one database generation with another.
