//! A Language Server Protocol client: framing, a fixed catalog of servers, URI mapping and a
//! child-process client. Toolkit-free and thread-based; the editor turns its answers into
//! diagnostics, hovers and completions.

pub mod client;
pub mod framing;
pub mod servers;
pub mod uri;

pub use client::{LspClient, ServerEvent};
pub use servers::{find_program, spec_for_path, ServerSpec, SERVERS};
