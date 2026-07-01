use serde::{Deserialize, Serialize};

mod command;
mod kill;
mod ports;
mod processes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortEntry {
    pub port: u16,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub state: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: Option<f32>,
    pub mem_mb: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillResult {
    pub ok: bool,
    pub message: String,
}

pub use command::{
    command_exists, copy_to_clipboard, launch_in_terminal, open_in_system_editor,
    python_command, resolve_command_path, shell_command,
};
pub use kill::{kill_pid, kill_port};
pub use ports::list_ports;
pub use processes::list_processes;
