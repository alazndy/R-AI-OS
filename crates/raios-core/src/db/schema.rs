use rusqlite::{Connection, Result};

/// Idempotent schema creation and migration. Safe to run on every open.
pub(super) fn migrate(conn: &Connection) -> Result<()> {
    // Idempotent column additions for existing DBs (errors mean column already exists)
    let _ = conn.execute_batch(
        "ALTER TABLE health_cache ADD COLUMN refactor_grade TEXT NOT NULL DEFAULT '-'",
    );
    let _ = conn.execute_batch(
        "UPDATE projects SET status = 'active' WHERE status NOT IN ('production','active','early','legacy','waiting','beklemede','archived')",
    );
    let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN refactor_score INTEGER");
    let _ = conn.execute_batch(
        "ALTER TABLE health_cache ADD COLUMN refactor_high INTEGER NOT NULL DEFAULT 0",
    );
    // task_graph_nodes/swarm_tasks: these columns are already in the
    // CREATE TABLE below for fresh DBs. This ALTER path exists only for
    // upgrading pre-existing DBs created before these columns were added
    // here — GraphStore/SwarmStore used to carry their own duplicate
    // ALTER TABLE for the same purpose (removed; this is now the one
    // place that does it, consistent with every other column addition
    // above).
    let _ = conn.execute_batch(
        "ALTER TABLE task_graph_nodes ADD COLUMN cp_task_id TEXT;
         ALTER TABLE task_graph_nodes ADD COLUMN cp_agent_run_id TEXT;",
    );
    let _ = conn.execute_batch(
        "ALTER TABLE swarm_tasks ADD COLUMN cp_task_id TEXT;
         ALTER TABLE swarm_tasks ADD COLUMN cp_agent_run_id TEXT;
         ALTER TABLE swarm_tasks ADD COLUMN cp_artifact_id TEXT;
         ALTER TABLE swarm_tasks ADD COLUMN cp_approval_id TEXT;",
    );
    let _ = conn.execute_batch("ALTER TABLE mem_items ADD COLUMN layer INTEGER NOT NULL DEFAULT 2");
    let _ = conn.execute_batch(
        "ALTER TABLE mem_items ADD COLUMN provenance TEXT NOT NULL DEFAULT 'observed';
         ALTER TABLE mem_items ADD COLUMN confidence REAL NOT NULL DEFAULT 1.0;
         ALTER TABLE mem_items ADD COLUMN last_used_at TEXT;",
    );
    let _ = conn.execute_batch(
        "ALTER TABLE cp_task_graph_nodes ADD COLUMN node_kind TEXT NOT NULL DEFAULT 'work'",
    );
    // Keep the idempotent column upgrade separate from the index creation.
    // Once the column exists, SQLite reports a duplicate-column error and
    // stops that batch; bundling the CREATE INDEX with it used to leave older
    // databases without the ownership lookup index forever.
    let _ = conn.execute_batch(
        "ALTER TABLE cp_approvals ADD COLUMN owner_subject TEXT NOT NULL DEFAULT 'local_control_owner'",
    );
    let _ = conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_cp_approvals_owner_status
             ON cp_approvals(owner_subject, status)",
    );
    // Factory Phase 3 keeps short, user-authored discovery content in the
    // durable control plane until the artifact store is introduced.
    let _ = conn.execute_batch(
        "ALTER TABLE cp_factory_intake_items ADD COLUMN question_key TEXT NOT NULL DEFAULT '';
         ALTER TABLE cp_factory_intake_items ADD COLUMN response_text TEXT NOT NULL DEFAULT '';
         ALTER TABLE cp_factory_intake_items ADD COLUMN responded_at TEXT;
         ALTER TABLE cp_factory_charter_revisions ADD COLUMN content_text TEXT NOT NULL DEFAULT '';",
    );
    let _ = conn.execute_batch(
        "ALTER TABLE cp_factory_requirement_revisions ADD COLUMN content_text TEXT NOT NULL DEFAULT '';",
    );
    let _ = conn.execute_batch("ALTER TABLE cp_factory_stage_runs ADD COLUMN task_graph_id TEXT;");
    let _ = conn.execute_batch(
        "ALTER TABLE cp_factory_support_items ADD COLUMN resolution_ref TEXT NOT NULL DEFAULT '';",
    );
    let _ = conn.execute_batch(
        "ALTER TABLE cp_factory_evidence_links ADD COLUMN staleness_state TEXT NOT NULL DEFAULT 'current';",
    );
    let _ = conn.execute_batch(
        "ALTER TABLE cp_factory_products ADD COLUMN factory_mode TEXT NOT NULL DEFAULT 'governed';",
    );
    let _ = conn.execute_batch(
        "ALTER TABLE cp_factory_products ADD COLUMN project_path TEXT NOT NULL DEFAULT '';",
    );

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            category    TEXT NOT NULL DEFAULT '',
            path        TEXT UNIQUE NOT NULL,
            github      TEXT,
            status      TEXT NOT NULL DEFAULT 'active',
            stars       INTEGER,
            last_commit TEXT,
            version     TEXT,
            nickname    TEXT,
            updated_at  TEXT DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS health_cache (
            project_id       INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            compliance_grade TEXT NOT NULL DEFAULT '-',
            compliance_score INTEGER,
            security_grade   TEXT,
            security_score   INTEGER,
            security_issues  INTEGER NOT NULL DEFAULT 0,
            security_critical INTEGER NOT NULL DEFAULT 0,
            git_dirty        INTEGER NOT NULL DEFAULT 0,
            has_memory       INTEGER NOT NULL DEFAULT 0,
            has_sigmap       INTEGER NOT NULL DEFAULT 0,
            remote_url       TEXT,
            scanned_at       TEXT DEFAULT (datetime('now')),
            refactor_grade   TEXT NOT NULL DEFAULT '-',
            refactor_score   INTEGER,
            refactor_high    INTEGER NOT NULL DEFAULT 0
        );

        -- COMPAT CACHE: tasks is superseded by cp_tasks (plan_id IS NULL). Do not read for new work.
        CREATE TABLE IF NOT EXISTS tasks (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            text       TEXT NOT NULL,
            completed  INTEGER NOT NULL DEFAULT 0,
            agent      TEXT,
            project    TEXT,
            created_at TEXT DEFAULT (datetime('now'))
        );

        -- Created unconditionally regardless of --features cortex.
        -- SQLite has no overhead for an empty table, and conditional schema
        -- migration would require versioned DDL. The cortex feature flag gates
        -- the code that writes here, not the schema itself.
        CREATE TABLE IF NOT EXISTS cortex_chunks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            path        TEXT    NOT NULL,
            mtime_secs  INTEGER NOT NULL,
            start_line  INTEGER NOT NULL,
            chunk_text  TEXT    NOT NULL,
            embedding   BLOB    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cortex_path ON cortex_chunks(path);
    ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS bm25_files (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            path       TEXT UNIQUE NOT NULL,
            mtime_secs INTEGER NOT NULL,
            doc_length INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS bm25_postings (
            token   TEXT    NOT NULL,
            file_id INTEGER NOT NULL REFERENCES bm25_files(id) ON DELETE CASCADE,
            line_no INTEGER NOT NULL,
            snippet TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_bm25_token ON bm25_postings(token);
        -- The token index cannot serve ON DELETE CASCADE lookups by file_id.
        -- Without this, a scoped reindex repeatedly scans the whole postings
        -- table while removing stale files.
        CREATE INDEX IF NOT EXISTS idx_bm25_postings_file ON bm25_postings(file_id);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sessions (
            id          TEXT PRIMARY KEY,
            agent       TEXT NOT NULL,
            project     TEXT,
            started_at  TEXT NOT NULL,
            ended_at    TEXT,
            summary     TEXT
        );

        CREATE TABLE IF NOT EXISTS session_events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            event_type  TEXT NOT NULL,
            data        TEXT NOT NULL DEFAULT '',
            timestamp   TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_se_session ON session_events(session_id);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS instinct_candidates (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            project_name TEXT NOT NULL,
            command      TEXT NOT NULL,
            outcome      TEXT NOT NULL,
            suggestion   TEXT NOT NULL,
            status       TEXT NOT NULL DEFAULT 'pending',
            created_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- COMPAT CACHE: task_graphs is rebuilt from cp_task_graphs; not source of truth.
        CREATE TABLE IF NOT EXISTS task_graphs (
            id          TEXT PRIMARY KEY,
            goal        TEXT NOT NULL,
            agent       TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'pending',
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            completed_at TEXT
        );

        -- COMPAT CACHE: task_graph_nodes is rebuilt from cp_task_graph_nodes + cp_tasks; not source of truth.
        CREATE TABLE IF NOT EXISTS task_graph_nodes (
            id          TEXT NOT NULL,
            graph_id    TEXT NOT NULL REFERENCES task_graphs(id) ON DELETE CASCADE,
            description TEXT NOT NULL,
            shell_cmd   TEXT NOT NULL,
            deps        TEXT NOT NULL DEFAULT '[]',
            status      TEXT NOT NULL DEFAULT 'pending',
            cp_task_id  TEXT,
            cp_agent_run_id TEXT,
            factory_job_id TEXT,
            result      TEXT,
            error       TEXT,
            PRIMARY KEY (graph_id, id)
        );
        CREATE INDEX IF NOT EXISTS idx_tgn_graph ON task_graph_nodes(graph_id);

        CREATE TABLE IF NOT EXISTS swarm_tasks (
            id            TEXT PRIMARY KEY,
            project_name  TEXT NOT NULL,
            project_path  TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            branch_name   TEXT NOT NULL,
            description   TEXT NOT NULL,
            agent         TEXT NOT NULL,
            status        TEXT NOT NULL DEFAULT 'initializing',
            created_at    TEXT NOT NULL DEFAULT (datetime('now')),
            completed_at  TEXT,
            cp_task_id      TEXT,
            cp_agent_run_id TEXT,
            cp_artifact_id  TEXT,
            cp_approval_id  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_swarm_status ON swarm_tasks(status);

        CREATE TABLE IF NOT EXISTS cp_tasks (
            id TEXT PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
            plan_id TEXT,
            parent_task_id TEXT REFERENCES cp_tasks(id) ON DELETE SET NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 50,
            status TEXT NOT NULL,
            assignee_kind TEXT,
            assignee_id TEXT,
            acceptance_criteria TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_tasks_project ON cp_tasks(project_id);
        CREATE INDEX IF NOT EXISTS idx_cp_tasks_status ON cp_tasks(status);

        CREATE TABLE IF NOT EXISTS cp_agent_runs (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
            provider TEXT NOT NULL,
            agent_name TEXT NOT NULL,
            run_contract_id TEXT NOT NULL,
            attempt INTEGER NOT NULL DEFAULT 1,
            status TEXT NOT NULL,
            started_at TEXT,
            ended_at TEXT,
            exit_reason TEXT,
            summary TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cp_runs_task ON cp_agent_runs(task_id);
        CREATE INDEX IF NOT EXISTS idx_cp_runs_status ON cp_agent_runs(status);
        CREATE INDEX IF NOT EXISTS idx_cp_runs_agent ON cp_agent_runs(agent_name);

        -- Explicit, wrapper-owned user notes. These deliberately remain
        -- separate from legacy `sessions`/`session_events`: wrapper runs are
        -- authenticated by a live cp_agent_runs row and a project boundary.
        CREATE TABLE IF NOT EXISTS cp_wrapper_events (
            id           TEXT PRIMARY KEY,
            agent_run_id TEXT NOT NULL REFERENCES cp_agent_runs(id) ON DELETE CASCADE,
            project_id   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            event_kind   TEXT NOT NULL CHECK(event_kind IN ('memory_note')),
            content      TEXT NOT NULL,
            created_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_wrapper_events_run
            ON cp_wrapper_events(agent_run_id, created_at);

        CREATE TABLE IF NOT EXISTS cp_artifacts (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            agent_run_id TEXT NOT NULL REFERENCES cp_agent_runs(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            status TEXT NOT NULL,
            path TEXT,
            content_ref TEXT,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_artifacts_task ON cp_artifacts(task_id);
        CREATE INDEX IF NOT EXISTS idx_cp_artifacts_status ON cp_artifacts(status);

        CREATE TABLE IF NOT EXISTS cp_approvals (
            id TEXT PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
            task_id TEXT REFERENCES cp_tasks(id) ON DELETE SET NULL,
            agent_run_id TEXT REFERENCES cp_agent_runs(id) ON DELETE SET NULL,
            artifact_id TEXT REFERENCES cp_artifacts(id) ON DELETE SET NULL,
            approval_type TEXT NOT NULL,
            reason TEXT NOT NULL,
            status TEXT NOT NULL,
            owner_subject TEXT NOT NULL DEFAULT 'local_control_owner',
            requested_at TEXT NOT NULL,
            resolved_at TEXT,
            resolved_by TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cp_approvals_status ON cp_approvals(status);
        CREATE INDEX IF NOT EXISTS idx_cp_approvals_owner_status ON cp_approvals(owner_subject, status);

        CREATE TABLE IF NOT EXISTS cp_budget_ledger (
            id TEXT PRIMARY KEY,
            scope_kind TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            provider TEXT,
            metric TEXT NOT NULL,
            limit_value REAL,
            used_value REAL,
            remaining_value REAL,
            reset_at TEXT,
            confidence TEXT NOT NULL,
            source TEXT NOT NULL,
            observed_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_budget_scope ON cp_budget_ledger(scope_kind, scope_id);

        CREATE TABLE IF NOT EXISTS cp_task_edges (
            graph_id TEXT NOT NULL REFERENCES task_graphs(id) ON DELETE CASCADE,
            task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            depends_on_task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            edge_kind TEXT NOT NULL DEFAULT 'blocks',
            created_at TEXT NOT NULL,
            PRIMARY KEY (graph_id, task_id, depends_on_task_id)
        );
        CREATE INDEX IF NOT EXISTS idx_cp_task_edges_graph ON cp_task_edges(graph_id);
        CREATE INDEX IF NOT EXISTS idx_cp_task_edges_task ON cp_task_edges(task_id);

        CREATE TABLE IF NOT EXISTS cp_task_graph_nodes (
            graph_id TEXT NOT NULL REFERENCES task_graphs(id) ON DELETE CASCADE,
            node_id TEXT NOT NULL,
            task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            agent_run_id TEXT NOT NULL REFERENCES cp_agent_runs(id) ON DELETE CASCADE,
            shell_cmd TEXT NOT NULL,
            node_kind TEXT NOT NULL DEFAULT 'work',
            created_at TEXT NOT NULL,
            PRIMARY KEY (graph_id, node_id),
            UNIQUE (graph_id, task_id)
        );
        CREATE INDEX IF NOT EXISTS idx_cp_task_graph_nodes_graph ON cp_task_graph_nodes(graph_id);
        CREATE INDEX IF NOT EXISTS idx_cp_task_graph_nodes_task ON cp_task_graph_nodes(task_id);

        CREATE TABLE IF NOT EXISTS cp_task_graphs (
            graph_id TEXT PRIMARY KEY REFERENCES task_graphs(id) ON DELETE CASCADE,
            goal TEXT NOT NULL,
            agent TEXT NOT NULL,
            created_at TEXT NOT NULL,
            completed_at TEXT
        );

        CREATE TABLE IF NOT EXISTS cp_task_list_items (
            task_id       TEXT PRIMARY KEY REFERENCES cp_tasks(id) ON DELETE CASCADE,
            source_kind   TEXT NOT NULL DEFAULT 'markdown',
            source_path   TEXT NOT NULL DEFAULT '',
            display_order INTEGER NOT NULL DEFAULT 0,
            project_name  TEXT,
            created_at    TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_cp_task_list_items_order ON cp_task_list_items(display_order);

        CREATE TABLE IF NOT EXISTS cp_run_contracts (
            id                   TEXT PRIMARY KEY,
            task_id              TEXT REFERENCES cp_tasks(id) ON DELETE SET NULL,
            workspace_root       TEXT NOT NULL DEFAULT '',
            allowed_paths_json   TEXT NOT NULL DEFAULT '[]',
            blocked_paths_json   TEXT NOT NULL DEFAULT '[]',
            allowed_tools_json   TEXT NOT NULL DEFAULT '[]',
            network_policy_json  TEXT NOT NULL DEFAULT '{}',
            token_budget         INTEGER,
            time_budget_secs     INTEGER,
            cpu_budget_pct       REAL,
            memory_budget_mb     INTEGER,
            expected_artifacts_json TEXT NOT NULL DEFAULT '[]',
            success_criteria_json   TEXT NOT NULL DEFAULT '{}',
            escalation_policy_json  TEXT NOT NULL DEFAULT '{}',
            created_at           TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_cp_run_contracts_task ON cp_run_contracts(task_id);

        CREATE TABLE IF NOT EXISTS cp_provider_capabilities (
            provider                      TEXT PRIMARY KEY,
            supports_tool_calling         INTEGER NOT NULL DEFAULT 0,
            supports_patch_diff           INTEGER NOT NULL DEFAULT 0,
            supports_long_running         INTEGER NOT NULL DEFAULT 0,
            supports_streaming            INTEGER NOT NULL DEFAULT 0,
            supports_exact_quota_visibility INTEGER NOT NULL DEFAULT 0,
            updated_at                    TEXT NOT NULL DEFAULT (datetime('now'))
        );
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS audit_log (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp  TEXT    NOT NULL DEFAULT (datetime('now','utc')),
            event_type TEXT    NOT NULL,
            actor      TEXT    NOT NULL DEFAULT 'raios',
            data       TEXT    NOT NULL DEFAULT '',
            prev_hash  TEXT    NOT NULL DEFAULT '',
            hash       TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_log(timestamp);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS tool_traces (
            id              TEXT PRIMARY KEY,
            project         TEXT NOT NULL,
            agent           TEXT NOT NULL,
            command         TEXT NOT NULL,
            context         TEXT NOT NULL DEFAULT '',
            outcome         TEXT NOT NULL DEFAULT '',
            error_summary   TEXT NOT NULL DEFAULT '',
            fix_summary     TEXT NOT NULL DEFAULT '',
            tags_json       TEXT NOT NULL DEFAULT '[]',
            success         INTEGER NOT NULL DEFAULT 0,
            confidence      REAL NOT NULL DEFAULT 0.5,
            related_task_id TEXT,
            content_hash    TEXT NOT NULL UNIQUE,
            redacted        INTEGER NOT NULL DEFAULT 0,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_tool_traces_project_created_at
            ON tool_traces(project, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_tool_traces_success ON tool_traces(success);
        CREATE INDEX IF NOT EXISTS idx_tool_traces_content_hash ON tool_traces(content_hash);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS cp_scheduled_jobs (
            id               TEXT PRIMARY KEY,
            title            TEXT NOT NULL,
            agent            TEXT NOT NULL,
            task_description TEXT NOT NULL,
            project_id       TEXT,
            interval_secs    INTEGER NOT NULL,
            status           TEXT NOT NULL DEFAULT 'active',
            last_run_at      TEXT,
            next_run_at      TEXT NOT NULL,
            created_at       TEXT NOT NULL,
            run_count        INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_cp_scheduled_jobs_status   ON cp_scheduled_jobs(status);
        CREATE INDEX IF NOT EXISTS idx_cp_scheduled_jobs_next_run ON cp_scheduled_jobs(next_run_at);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS agent_doctor_runs (
            agent        TEXT PRIMARY KEY,
            tier_reached TEXT NOT NULL,
            notes_json   TEXT NOT NULL DEFAULT '[]',
            checked_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS cp_logs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            ts          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            sender      TEXT    NOT NULL,
            content     TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_logs_ts ON cp_logs(id DESC);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS mem_items (
            id          TEXT PRIMARY KEY,
            project_key TEXT NOT NULL,
            item_type   TEXT NOT NULL CHECK(item_type IN ('user','feedback','project','reference')),
            slug        TEXT NOT NULL,
            title       TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            body        TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
            session_id  TEXT,
            layer       INTEGER NOT NULL DEFAULT 2,
            provenance  TEXT NOT NULL DEFAULT 'observed',
            confidence  REAL NOT NULL DEFAULT 1.0,
            last_used_at TEXT,
            UNIQUE(project_key, slug)
        );
        CREATE INDEX IF NOT EXISTS idx_mem_items_project ON mem_items(project_key);
        CREATE INDEX IF NOT EXISTS idx_mem_items_type    ON mem_items(project_key, item_type);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS mem_nodes (
            id          TEXT PRIMARY KEY,
            project_key TEXT NOT NULL,
            kind        TEXT NOT NULL CHECK(kind IN ('l0_raw','revision')),
            source      TEXT NOT NULL DEFAULT '',
            content     TEXT NOT NULL,
            session_id  TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_mem_nodes_project ON mem_nodes(project_key, kind);
        CREATE INDEX IF NOT EXISTS idx_mem_nodes_dedup ON mem_nodes(project_key, kind, content);

        CREATE TABLE IF NOT EXISTS mem_lineage (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            child_kind  TEXT NOT NULL CHECK(child_kind IN ('item','node')),
            child_id    TEXT NOT NULL,
            parent_kind TEXT NOT NULL CHECK(parent_kind IN ('item','node')),
            parent_id   TEXT NOT NULL,
            relation    TEXT NOT NULL DEFAULT 'derived_from',
            created_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(child_kind, child_id, parent_kind, parent_id, relation)
        );
        CREATE INDEX IF NOT EXISTS idx_mem_lineage_child  ON mem_lineage(child_kind, child_id);
        CREATE INDEX IF NOT EXISTS idx_mem_lineage_parent ON mem_lineage(parent_kind, parent_id);
        ",
    )?;

    // Product Factory skeleton. These tables establish canonical ownership and
    // revision boundaries only. No migration copies, index rebuilds, or product
    // lifecycle mutations run from this schema phase.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS cp_workspaces (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            owner_subject TEXT NOT NULL,
            storage_root TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_cp_workspaces_owner ON cp_workspaces(owner_subject);

        CREATE TABLE IF NOT EXISTS cp_plans (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES cp_workspaces(id) ON DELETE CASCADE,
            product_id TEXT,
            title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'planned',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_cp_plans_workspace ON cp_plans(workspace_id);
        CREATE INDEX IF NOT EXISTS idx_cp_plans_product ON cp_plans(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_products (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES cp_workspaces(id) ON DELETE CASCADE,
            owner_subject TEXT NOT NULL,
            title TEXT NOT NULL,
            factory_mode TEXT NOT NULL DEFAULT 'governed' CHECK(factory_mode IN ('quick', 'governed')),
            project_path TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'draft',
            current_charter_revision_id TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_factory_products_workspace ON cp_factory_products(workspace_id);
        CREATE INDEX IF NOT EXISTS idx_factory_products_owner ON cp_factory_products(owner_subject);

        CREATE TABLE IF NOT EXISTS cp_factory_intake_sessions (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'open',
            started_by TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            closed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_factory_intake_product ON cp_factory_intake_sessions(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_intake_items (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES cp_factory_intake_sessions(id) ON DELETE CASCADE,
            item_kind TEXT NOT NULL,
            question_key TEXT NOT NULL DEFAULT '',
            prompt_ref TEXT,
            response_ref TEXT,
            response_text TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'open',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            responded_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_factory_intake_items_session ON cp_factory_intake_items(session_id);

        CREATE TABLE IF NOT EXISTS cp_factory_charter_revisions (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            revision INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'proposed',
            content_ref TEXT NOT NULL,
            content_text TEXT NOT NULL DEFAULT '',
            created_by TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(product_id, revision)
        );
        CREATE INDEX IF NOT EXISTS idx_factory_charter_product ON cp_factory_charter_revisions(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_requirements (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            stable_key TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'proposed',
            current_revision INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(product_id, stable_key)
        );
        CREATE INDEX IF NOT EXISTS idx_factory_requirements_product ON cp_factory_requirements(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_requirement_revisions (
            id TEXT PRIMARY KEY,
            requirement_id TEXT NOT NULL REFERENCES cp_factory_requirements(id) ON DELETE CASCADE,
            revision INTEGER NOT NULL,
            content_ref TEXT NOT NULL,
            content_text TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'proposed',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(requirement_id, revision)
        );

        CREATE TABLE IF NOT EXISTS cp_factory_requirement_links (
            id TEXT PRIMARY KEY,
            requirement_id TEXT NOT NULL REFERENCES cp_factory_requirements(id) ON DELETE CASCADE,
            target_kind TEXT NOT NULL,
            target_id TEXT NOT NULL,
            relation_kind TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(requirement_id, target_kind, target_id, relation_kind)
        );
        CREATE INDEX IF NOT EXISTS idx_factory_requirement_links_target ON cp_factory_requirement_links(target_kind, target_id);

        CREATE TABLE IF NOT EXISTS cp_factory_decisions (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'proposed',
            rationale_ref TEXT,
            supersedes_id TEXT REFERENCES cp_factory_decisions(id) ON DELETE SET NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_factory_decisions_product ON cp_factory_decisions(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_cycles (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            plan_id TEXT NOT NULL REFERENCES cp_plans(id) ON DELETE RESTRICT,
            status TEXT NOT NULL DEFAULT 'planned',
            current_stage TEXT NOT NULL DEFAULT 'discover',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            completed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_factory_cycles_product ON cp_factory_cycles(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_stage_runs (
            id TEXT PRIMARY KEY,
            cycle_id TEXT NOT NULL REFERENCES cp_factory_cycles(id) ON DELETE CASCADE,
            stage TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            task_graph_id TEXT REFERENCES task_graphs(id) ON DELETE SET NULL,
            started_at TEXT,
            completed_at TEXT,
            UNIQUE(cycle_id, stage)
        );

        CREATE TABLE IF NOT EXISTS cp_factory_change_requests (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            requested_by TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'proposed',
            summary TEXT NOT NULL,
            source_revision_ref TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            resolved_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_factory_changes_product ON cp_factory_change_requests(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_impact_assessments (
            id TEXT PRIMARY KEY,
            change_request_id TEXT NOT NULL REFERENCES cp_factory_change_requests(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'pending',
            affected_count INTEGER NOT NULL DEFAULT 0,
            evidence_ref TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            accepted_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_factory_impacts_change ON cp_factory_impact_assessments(change_request_id);

        CREATE TABLE IF NOT EXISTS cp_factory_impact_targets (
            id TEXT PRIMARY KEY,
            assessment_id TEXT NOT NULL REFERENCES cp_factory_impact_assessments(id) ON DELETE CASCADE,
            target_kind TEXT NOT NULL,
            target_id TEXT NOT NULL,
            relation_kind TEXT NOT NULL,
            staleness_state TEXT NOT NULL DEFAULT 'current',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(assessment_id, target_kind, target_id, relation_kind)
        );
        CREATE INDEX IF NOT EXISTS idx_factory_impact_targets_assessment ON cp_factory_impact_targets(assessment_id);

        CREATE TABLE IF NOT EXISTS cp_factory_evidence_links (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            subject_kind TEXT NOT NULL,
            subject_id TEXT NOT NULL,
            artifact_id TEXT REFERENCES cp_artifacts(id) ON DELETE SET NULL,
            content_ref TEXT,
            storage_class TEXT NOT NULL,
            staleness_state TEXT NOT NULL DEFAULT 'current',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_factory_evidence_subject ON cp_factory_evidence_links(subject_kind, subject_id);

        CREATE TABLE IF NOT EXISTS cp_factory_evidence_dependencies (
            id TEXT PRIMARY KEY,
            evidence_id TEXT NOT NULL REFERENCES cp_factory_evidence_links(id) ON DELETE CASCADE,
            target_kind TEXT NOT NULL,
            target_id TEXT NOT NULL,
            relation_kind TEXT NOT NULL DEFAULT 'verifies',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(evidence_id, target_kind, target_id, relation_kind)
        );
        CREATE INDEX IF NOT EXISTS idx_factory_evidence_dependencies_target
            ON cp_factory_evidence_dependencies(target_kind, target_id);

        CREATE TABLE IF NOT EXISTS cp_factory_quality_profiles (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            required INTEGER NOT NULL DEFAULT 1,
            definition_ref TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );

        CREATE TABLE IF NOT EXISTS cp_factory_quality_checks (
            id TEXT PRIMARY KEY,
            profile_id TEXT NOT NULL REFERENCES cp_factory_quality_profiles(id) ON DELETE CASCADE,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'pending',
            evidence_ref TEXT,
            checked_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_factory_quality_checks_product ON cp_factory_quality_checks(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_releases (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'draft',
            build_ref TEXT NOT NULL,
            approval_id TEXT REFERENCES cp_approvals(id) ON DELETE SET NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            released_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_factory_releases_product ON cp_factory_releases(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_release_channels (
            id TEXT PRIMARY KEY,
            release_id TEXT NOT NULL REFERENCES cp_factory_releases(id) ON DELETE CASCADE,
            channel_kind TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'draft',
            external_ref TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(release_id, channel_kind)
        );

        CREATE TABLE IF NOT EXISTS cp_factory_support_items (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            source_kind TEXT NOT NULL,
            external_ref TEXT,
            status TEXT NOT NULL DEFAULT 'new',
            summary_ref TEXT,
            resolution_ref TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            resolved_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_factory_support_product ON cp_factory_support_items(product_id);

        CREATE TABLE IF NOT EXISTS cp_factory_support_links (
            id TEXT PRIMARY KEY,
            support_item_id TEXT NOT NULL REFERENCES cp_factory_support_items(id) ON DELETE CASCADE,
            target_kind TEXT NOT NULL,
            target_id TEXT NOT NULL,
            relation_kind TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(support_item_id, target_kind, target_id, relation_kind)
        );

        CREATE TABLE IF NOT EXISTS cp_factory_automation_policies (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            maintenance_mode TEXT NOT NULL DEFAULT 'observe',
            policy_ref TEXT,
            enabled INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );

        CREATE TABLE IF NOT EXISTS cp_factory_integrations (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES cp_workspaces(id) ON DELETE CASCADE,
            integration_kind TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'disabled',
            secret_lease_ref TEXT,
            configuration_ref TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(workspace_id, integration_kind)
        );

        CREATE TABLE IF NOT EXISTS cp_factory_events (
            id TEXT PRIMARY KEY,
            product_id TEXT NOT NULL REFERENCES cp_factory_products(id) ON DELETE CASCADE,
            event_kind TEXT NOT NULL,
            subject_kind TEXT NOT NULL,
            subject_id TEXT NOT NULL,
            actor_subject TEXT NOT NULL,
            metadata_ref TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_factory_events_product_created ON cp_factory_events(product_id, created_at DESC);
        ",
    )?;

    Ok(())
}
