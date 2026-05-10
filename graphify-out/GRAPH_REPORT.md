# Graph Report - .  (2026-05-07)

## Corpus Check
- 74 files · ~67,472 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 665 nodes · 1162 edges · 55 communities (52 shown, 3 thin omitted)
- Extraction: 78% EXTRACTED · 22% INFERRED · 0% AMBIGUOUS · INFERRED: 260 edges (avg confidence: 0.8)
- Token cost: 37,000 input · 6,000 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Daemon State & IPC|Daemon State & IPC]]
- [[_COMMUNITY_CLI & Entry Points|CLI & Entry Points]]
- [[_COMMUNITY_Agent Concepts & Memory|Agent Concepts & Memory]]
- [[_COMMUNITY_Agent Config & Discovery|Agent Config & Discovery]]
- [[_COMMUNITY_App Event Handler|App Event Handler]]
- [[_COMMUNITY_Cortex Chunker|Cortex Chunker]]
- [[_COMMUNITY_Agent Runner & Hybrid Search|Agent Runner & Hybrid Search]]
- [[_COMMUNITY_Setup Wizard|Setup Wizard]]
- [[_COMMUNITY_Daemon Workers & Infra|Daemon Workers & Infra]]
- [[_COMMUNITY_TUI Editor|TUI Editor]]
- [[_COMMUNITY_OWASP Security Scanner|OWASP Security Scanner]]
- [[_COMMUNITY_Daemon Server Workers|Daemon Server Workers]]
- [[_COMMUNITY_App State Machine|App State Machine]]
- [[_COMMUNITY_MemPalace Builder|MemPalace Builder]]
- [[_COMMUNITY_Compliance Checker|Compliance Checker]]
- [[_COMMUNITY_Module Activity|Module: Activity]]
- [[_COMMUNITY_Module AiAuditReport|Module: AiAuditReport]]
- [[_COMMUNITY_Module Apphandle_bg_msg()|Module: App::handle_bg_msg()]]
- [[_COMMUNITY_Module Appexecute_command|Module: App::execute_command]]
- [[_COMMUNITY_Module AgentInfo|Module: AgentInfo]]
- [[_COMMUNITY_Module check_graphify()|Module: check_graphify()]]
- [[_COMMUNITY_Module ProjectIndex|Module: ProjectIndex]]
- [[_COMMUNITY_Module connect_daemon()|Module: connect_daemon()]]
- [[_COMMUNITY_Module McpServer|Module: McpServer]]
- [[_COMMUNITY_Module AgentProcess|Module: AgentProcess]]
- [[_COMMUNITY_Module exec_initialize()|Module: exec_initialize()]]
- [[_COMMUNITY_Module main()|Module: main()]]
- [[_COMMUNITY_Module aiosd â€” AI OS daem|Module: aiosd â€” AI OS daem]]
- [[_COMMUNITY_Module Requirement struct|Module: Requirement struct]]
- [[_COMMUNITY_Module PALETTE_ITEMS comman|Module: PALETTE_ITEMS comman]]

## God Nodes (most connected - your core abstractions)
1. `App` - 24 edges
2. `App` - 23 edges
3. `render()` - 18 edges
4. `run()` - 17 edges
5. `check_project()` - 15 edges
6. `McpServer` - 15 edges
7. `MASTER.md â€” R-AI-OS agent constitution file` - 15 edges
8. `Server (daemon/server.rs)` - 15 edges
9. `render_content_body()` - 14 edges
10. `check_file()` - 11 edges

## Surprising Connections (you probably didn't know these)
- `R-AI-OS README (product description)` --describes--> `Server (daemon/server.rs)`  [INFERRED]
  README.md → src/daemon/server.rs
- `Cargo compile errors log (v0.5.0 refactor artifacts)` --recorded_compile_errors_for--> `DaemonState (daemon/state.rs)`  [INFERRED]
  errors.txt → src/daemon/state.rs
- `Hybrid Search architecture (BM25 + fastembed semantic)` --is_exposed_via--> `VectorSearch IPC Command (hybrid BM25+semantic)`  [INFERRED]
  memory.md → src/daemon/server.rs
- `Antigravity â€” AI agent (antigravity CLI)` --implemented--> `Auto-Spawn Daemon (ensure_daemon_running in ipc.rs)`  [EXTRACTED]
  C:/Users/turha/Desktop/Dev_Ops_New/07_DevTools_&_Productivity/CLI_Tools/R-AI-OS/src/agent_runner.rs → memory.md
- `Antigravity â€” AI agent (antigravity CLI)` --implemented--> `NotebookLM export (Python automation, source packaging)`  [EXTRACTED]
  C:/Users/turha/Desktop/Dev_Ops_New/07_DevTools_&_Productivity/CLI_Tools/R-AI-OS/src/agent_runner.rs → memory.md

## Communities (55 total, 3 thin omitted)

### Community 0 - "Daemon State & IPC"
Cohesion: 0.06
Nodes (63): DaemonState, FileChangeApproval, crate::indexer::SearchResult, center_rect(), render_boot(), render_bouncing_alert(), render_command_palette(), render_file_changed_badge() (+55 more)

### Community 1 - "CLI & Entry Points"
Cohesion: 0.06
Nodes (60): main(), Cli, cmd_agents(), cmd_commit(), cmd_discover(), cmd_health(), cmd_memory(), cmd_mempalace() (+52 more)

### Community 2 - "Agent Concepts & Memory"
Cohesion: 0.05
Nodes (48): Antigravity â€” AI agent (antigravity CLI), Claude Code agent, Gemini CLI agent, AppState enum (TUI routing), Architectural Memory option (cortex vectral function/rule links), Auto-Spawn Daemon (ensure_daemon_running in ipc.rs), TUI colour palette (GREEN/CYAN/AMBER/RED/PANEL_BG), Handover Limit / Bouncing Guard (+40 more)

### Community 3 - "Agent Config & Discovery"
Cohesion: 0.06
Nodes (46): Claude Code â€” AI agent (claude CLI), Cursor â€” AI agent (cursor IDE), Gemini CLI â€” AI agent (gemini CLI), AgentRuleGroup â€” struct grouping rule/config files per AI agent, check_discovery_v2.py â€” project directory scanner v2, check_discovery_v3.py â€” project scanner with extended markers, check_discovery_v4.py â€” project scanner with pruning control, ComplianceReport â€” struct with score, violations, file_type (+38 more)

### Community 4 - "App Event Handler"
Cohesion: 0.1
Nodes (15): simple_diff(), App, append_memo(), launch_agent(), load_file_content(), build_prompt(), copy_to_clipboard(), dispatch_to_agent() (+7 more)

### Community 5 - "Cortex Chunker"
Cohesion: 0.08
Nodes (28): Chunk, chunk_code(), chunk_file(), chunk_markdown(), chunk_sliding_window(), flush_chunk(), Cortex struct, bow_embed() (+20 more)

### Community 6 - "Agent Runner & Hybrid Search"
Cohesion: 0.11
Nodes (13): run_agent(), fuse(), HybridResult, ResultSource, McpServer, RpcError, RpcRequest, RpcResponse (+5 more)

### Community 7 - "Setup Wizard"
Cohesion: 0.17
Nodes (18): AgentStatus, antigravity_md_template(), claude_md_template(), detect_agents(), exec_antigravity(), exec_claude(), exec_gemini(), exec_initialize() (+10 more)

### Community 8 - "Daemon Workers & Infra"
Cohesion: 0.12
Nodes (26): tokio broadcast channel (IPC pub-sub bus), Death Timer (agent timeout kill), Diff Inbox / Async File Change Approval Queue, notify file watcher (workspace change detection), Git status scan (branch + dirty detection per project), GitHub Sync (stars/last_commit via gh api), IPC TCP Port 42069, IPC Token Authentication (+18 more)

### Community 9 - "TUI Editor"
Cohesion: 0.12
Nodes (9): char_to_byte(), Editor, ChunkMeta, EmbPoint, PersistedStore, store_path(), vec_to_array(), VectorEngine (+1 more)

### Community 10 - "OWASP Security Scanner"
Cohesion: 0.13
Nodes (14): check_env_in_git(), detect_project_type(), parse_audit_issues(), parse_cargo_audit(), parse_npm_audit(), parse_pip_audit(), Pattern, ProjectType (+6 more)

### Community 11 - "Daemon Server Workers"
Cohesion: 0.13
Nodes (11): is_indexable(), start_cortex_worker(), start_git_worker(), start_health_worker(), Server, Config, DetectResult, find_dev_ops() (+3 more)

### Community 12 - "App State Machine"
Cohesion: 0.12
Nodes (6): App, filtered_palette(), PaletteItem, AppState enum, PortfolioStats, Task struct

### Community 13 - "MemPalace Builder"
Cohesion: 0.23
Nodes (15): build(), extract_date(), extract_status(), extract_version(), extract_version_nickname(), find_memory_file(), is_project_root(), is_skip_dir() (+7 more)

### Community 14 - "Compliance Checker"
Cohesion: 0.2
Nodes (9): check_file(), check_package_json(), check_python(), check_rust(), check_secrets(), check_typescript(), ComplianceReport, FileType (+1 more)

### Community 15 - "Module: Activity"
Cohesion: 0.15
Nodes (8): Activity, AppState, BgMsg, LogEntry, PortfolioStats, RuleCategory, SetupField, SortMode

### Community 16 - "Module: AiAuditReport"
Cohesion: 0.27
Nodes (11): AiAuditReport, check_antigravity(), check_cursor(), check_lm_studio(), check_npm_tool(), check_ollama(), scan_env_keys(), scan_local_models() (+3 more)

### Community 17 - "Module: App::handle_bg_msg()"
Cohesion: 0.22
Nodes (7): App::handle_bg_msg(), BgMsg enum, graphify.py script, find_graphify_script(), mempalace::build(), MemProject, MemRoom

### Community 18 - "Module: App::execute_command"
Cohesion: 0.25
Nodes (7): App::execute_command(), aiosd TCP 127.0.0.1:42069, tool_handover(), safe_write(), sync_universe(), AiAuditReport, scan_system()

### Community 19 - "Module: AgentInfo"
Cohesion: 0.32
Nodes (6): AgentInfo, discover_agents(), discover_skills(), open_in_editor(), scan_dir_for_skills(), SkillInfo

### Community 20 - "Module: check_graphify()"
Cohesion: 0.29
Nodes (7): check_project(), check_project_with_security(), ProjectHealth, security::scan_project(), SecurityIssue, SecurityReport, Severity enum

### Community 21 - "Module: ProjectIndex"
Cohesion: 0.43
Nodes (3): ProjectIndex, SearchResult, tokenize()

### Community 22 - "Module: connect_daemon()"
Cohesion: 0.38
Nodes (4): connect_daemon(), dispatch_event(), ensure_daemon_running(), system_rules()

### Community 23 - "Module: McpServer"
Cohesion: 0.33
Nodes (6): McpServer, run_stdio() MCP entry, new_project::create(), NewProjectConfig, load_tasks(), parse_task_line()

### Community 25 - "Module: exec_initialize()"
Cohesion: 0.5
Nodes (3): exec_initialize(), exec_workspace(), WizardStep enum

### Community 26 - "Module: main()"
Cohesion: 0.83
Nodes (3): main(), run_app(), run_tui()

### Community 27 - "Module: aiosd â€” AI OS daem"
Cohesion: 0.67
Nodes (3): aiosd â€” AI OS daemon (TCP server on port 42069), test_connection.py â€” Python E2E test for aiosd TCP connection, test_daemon_search.rs â€” Rust E2E test for aiosd Search command

## Knowledge Gaps
- **100 isolated node(s):** `Cli`, `Commands`, `Violation`, `FileType`, `DetectResult` (+95 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **3 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `App` connect `App State Machine` to `Cortex Chunker`, `TUI Editor`, `Module: App::handle_bg_msg()`, `Module: App::execute_command`, `Module: check_graphify()`, `Module: connect_daemon()`, `Module: main()`?**
  _High betweenness centrality (0.105) - this node is a cross-community bridge._
- **Why does `ProjectIndex` connect `Cortex Chunker` to `App State Machine`?**
  _High betweenness centrality (0.047) - this node is a cross-community bridge._
- **Are the 17 inferred relationships involving `render()` (e.g. with `run_app()` and `render_boot()`) actually correct?**
  _`render()` has 17 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `run()` (e.g. with `run_stdio()` and `run_agent()`) actually correct?**
  _`run()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 10 inferred relationships involving `check_project()` (e.g. with `cmd_health()` and `cmd_stats()`) actually correct?**
  _`check_project()` has 10 INFERRED edges - model-reasoned connections that need verification._
- **What connects `Cli`, `Commands`, `Violation` to the rest of the system?**
  _100 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Daemon State & IPC` be split into smaller, more focused modules?**
  _Cohesion score 0.06 - nodes in this community are weakly interconnected._