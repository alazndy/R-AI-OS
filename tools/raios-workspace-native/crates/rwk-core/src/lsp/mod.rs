//! A Language Server Protocol client: framing, a fixed catalog of servers, URI mapping and a
//! child-process client. Toolkit-free and thread-based; the editor turns its answers into
//! diagnostics, hovers and completions.

pub mod client;
pub mod edits;
pub mod framing;
pub mod manager;
pub mod problems;
pub mod servers;
pub mod uri;

pub use client::{LspClient, ServerEvent};
pub use lsp_types;
pub use manager::{
    default_launcher, LanguageServers, Launcher, ManagerEvent, Reply, Requester, ServerStatus,
};
pub use problems::{summary, Counts, Problem, Problems, Severity};
pub use servers::{
    find_program, parse_user_servers, set_user_servers, spec_for_path, spec_named, ServerSpec,
    SERVERS,
};
