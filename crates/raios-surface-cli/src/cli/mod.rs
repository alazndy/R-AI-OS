mod agent_stats;
mod agent_wrapper;
mod audit;
mod cron;
mod dev;
mod ext;
mod git;
mod handoff;
mod health;
mod hub;
mod instinct;
mod mem;
mod new;
mod policy;
mod refactor;
mod reflect;
mod search;
mod security;
mod session;
mod swarm;
mod task_update;
mod trace;
mod version;
mod workspace;
use self::mem::cmd_mem;
use self::session::cmd_sessions;
use self::task_update::cmd_task_update;
pub use self::task_update::run_refactor_flag;
use self::trace::cmd_trace;

use raios_core::config::Config;
use std::path::{Path, PathBuf};

mod action_types;
mod args;
pub use action_types::*;
pub use args::*;

// ─── Config helper ────────────────────────────────────────────────────────────

fn load_cfg() -> Config {
    if let Some(cfg) = Config::load() {
        return cfg;
    }
    Config::from_detect_result(Config::auto_detect())
}

pub(crate) fn resolve_project_path(project: Option<String>, dev_ops: &Path) -> PathBuf {
    match project {
        None => std::env::current_dir().unwrap_or_else(|_| dev_ops.to_path_buf()),
        Some(ref p) => {
            let direct = Path::new(p);
            if direct.exists() {
                return direct.to_path_buf();
            }
            if let Ok(conn) = raios_core::db::open_db() {
                if let Ok(projects) = raios_core::db::load_all_projects(&conn) {
                    if let Some(found) = projects
                        .iter()
                        .find(|pr| pr.name.to_lowercase().contains(&p.to_lowercase()))
                    {
                        return PathBuf::from(&found.path);
                    }
                }
            }
            direct.to_path_buf()
        }
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run(cli: Cli) {
    let cfg = load_cfg();
    let cmd = cli.command.expect("Subcommand missing");
    match cmd {
        Commands::Rules { name } => workspace::cmd_rules(name, &cfg.master_md_path, cli.json),
        Commands::Memory {
            project,
            query,
            top,
        } => workspace::cmd_memory(project, query, top, &cfg.dev_ops_path, cli.json),
        Commands::Mempalace => workspace::cmd_mempalace(&cfg.dev_ops_path, cli.json),
        Commands::Projects => workspace::cmd_projects(&cfg.dev_ops_path, cli.json),
        Commands::Agents => workspace::cmd_agents(cli.json),
        Commands::View { name } => workspace::cmd_view(name, &cfg.master_md_path, cli.json),
        Commands::Discover => workspace::cmd_discover(&cfg.dev_ops_path, cli.json),
        Commands::Health { project } => health::cmd_health(project, &cfg.dev_ops_path, cli.json),
        Commands::Version => println!("raios v{}", env!("CARGO_PKG_VERSION")),
        Commands::McpServer => {
            if let Err(e) = raios_surface_mcp::mcp_server::run_stdio() {
                eprintln!("MCP server error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Run {
            agent,
            project,
            timeout,
            extra,
        } => {
            if let Err(e) = raios_runtime::agent_runner::run_agent(&agent, project, timeout, extra)
            {
                eprintln!("Agent Runner Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Commit {
            project,
            message,
            push,
            dry_run,
        } => health::cmd_commit(project, message, push, dry_run, &cfg.dev_ops_path, cli.json),
        Commands::Stats => health::cmd_stats(&cfg.dev_ops_path, cli.json),
        Commands::Search {
            query,
            top_k,
            reindex,
        } => search::cmd_search(&query, top_k, reindex, &cfg.dev_ops_path, cli.json),
        Commands::License { project } => {
            security::cmd_license(project, &cfg.dev_ops_path, cli.json)
        }
        Commands::Audit { url, threshold } => {
            let exit = audit::cmd_audit(&url, threshold, cli.json);
            std::process::exit(exit);
        }
        Commands::Security {
            target,
            full,
            watch,
        } => security::cmd_security(target, full, watch, &cfg.dev_ops_path, cli.json),
        Commands::Refactor {
            target,
            high_lines,
            medium_lines,
            high_unwrap,
            medium_unwrap,
            high_nesting,
            medium_nesting,
            ext_config,
        } => refactor::cmd_refactor(
            target,
            &cfg.dev_ops_path,
            cli.json,
            high_lines,
            medium_lines,
            high_unwrap,
            medium_unwrap,
            high_nesting,
            medium_nesting,
            ext_config,
        ),
        Commands::New {
            name,
            category,
            github,
            no_vault,
        } => new::cmd_new(
            &name,
            &category,
            github,
            no_vault,
            &cfg.dev_ops_path,
            cli.json,
        ),
        Commands::Task {
            description,
            project,
            agent,
        } => new::cmd_task(&description, project, agent),
        Commands::Handoff {
            to,
            status,
            msg,
            project,
        } => {
            let project_path = resolve_project_path(project, &cfg.dev_ops_path);
            handoff::cmd_handoff(to, status, msg, &project_path, cli.json);
        }
        Commands::Bootstrap => new::cmd_bootstrap(),
        Commands::VersionBump {
            level,
            project,
            changelog,
            tag,
        } => {
            version::cmd_version_bump(&level, project, changelog, tag, &cfg.dev_ops_path, cli.json)
        }
        Commands::VersionInfo { project } => {
            version::cmd_version_info(project, &cfg.dev_ops_path, cli.json)
        }
        Commands::Disk { project } => dev::cmd_disk(project, &cfg.dev_ops_path, cli.json),
        Commands::Clean {
            project,
            dry_run,
            all,
        } => dev::cmd_clean(project, dry_run, all, &cfg.dev_ops_path, cli.json),
        Commands::Ps { procs, top } => dev::cmd_ps(procs, top, cli.json),
        Commands::Usage => dev::cmd_usage(cli.json),
        Commands::KillPort { port } => dev::cmd_kill_port(port, cli.json),
        Commands::Env { project, all } => dev::cmd_env(project, all, &cfg.dev_ops_path, cli.json),
        Commands::Deps {
            project,
            audit,
            all,
        } => dev::cmd_deps(project, audit, all, &cfg.dev_ops_path, cli.json),
        Commands::Build {
            project,
            release,
            check,
        } => dev::cmd_build(project, release, check, &cfg.dev_ops_path, cli.json),
        Commands::Test {
            project,
            all,
            instrumented,
        } => dev::cmd_test(project, all, instrumented, &cfg.dev_ops_path, cli.json),
        Commands::Git { cmd } => git::cmd_git(cmd, &cfg.dev_ops_path, cli.json),
        Commands::Instinct { cmd } => instinct::cmd_instinct(cmd, &cfg.dev_ops_path, cli.json),
        Commands::Ci { project } => dev::cmd_ci(project, &cfg.dev_ops_path, cli.json),
        Commands::CortexIndex { force } => {
            search::cmd_cortex_index(force, &cfg.dev_ops_path, cli.json)
        }
        Commands::Swarm { action } => swarm::cmd_swarm(action, cli.json),
        Commands::Route { query } => swarm::cmd_route(&query, cli.json),
        Commands::Evolve { action } => swarm::cmd_evolve(action, cli.json),
        Commands::VerifyChain { last } => security::cmd_verify_chain(last, cli.json),
        Commands::RateStatus => security::cmd_rate_status(cli.json),
        Commands::PinReset => security::cmd_pin_reset(cli.json),
        Commands::PinStatus => security::cmd_pin_status(cli.json),
        Commands::Quarantine { action } => security::cmd_quarantine(action, cli.json),
        Commands::Secret { action } => security::cmd_secret(action, cli.json),
        Commands::TaskUpdate { id, status } => cmd_task_update(&id, &status, cli.json),
        Commands::Cron { action } => cron::cmd_cron(action, cli.json),
        Commands::AgentWrapper { action } => {
            let a = match action {
                AgentWrapperCmd::Install { agents } => {
                    agent_wrapper::AgentWrapperAction::Install { agents }
                }
                AgentWrapperCmd::Remove { agents } => {
                    agent_wrapper::AgentWrapperAction::Remove { agents }
                }
                AgentWrapperCmd::Status => agent_wrapper::AgentWrapperAction::Status,
            };
            agent_wrapper::cmd_agent_wrapper(a, cli.json);
        }
        Commands::Sessions { agent, top } => cmd_sessions(agent.as_deref(), top, cli.json),
        Commands::AgentStats { agent } => agent_stats::cmd_agent_stats(agent, cli.json),
        Commands::MemoryGen { project } => {
            raios_runtime::session_memory::cmd_memory_gen(project.as_deref(), cli.json);
        }
        Commands::Mem { action } => cmd_mem(action, cli.json),
        Commands::Trace { action } => cmd_trace(action, cli.json),
        Commands::Policy { action } => policy::cmd_policy(action, cli.json),
        Commands::Hub { action } => match action {
            HubAction::Start => hub::cmd_start(cli.json),
            HubAction::Stop => hub::cmd_stop(cli.json),
            HubAction::Status => hub::cmd_status(cli.json),
            HubAction::Install { enable } => hub::cmd_install(enable, cli.json),
            HubAction::Logs { lines } => hub::cmd_logs(lines),
            HubAction::ApiKey { action } => match action {
                HubApiKeyAction::Generate { force } => hub::cmd_api_key_generate(force),
                HubApiKeyAction::Show => hub::cmd_api_key_show(),
            },
        },
        Commands::Reflect => reflect::cmd_reflect(&cfg.dev_ops_path, cli.json),
        Commands::PreFlight { project } => {
            let ok = raios_runtime::cli::preflight::cmd_preflight(project, &cfg.dev_ops_path);
            if !ok {
                std::process::exit(1);
            }
        }
        Commands::Ext { name, args } => ext::cmd_ext(&name, &args, &cfg.dev_ops_path, cli.json),
    }
}
