use raios_contracts::{
    ExploreSnapshot, GovernSnapshot, NowSnapshot, SnapshotEnvelope, WorkSnapshot,
};

use crate::app::route::Route;

#[derive(Debug, Clone)]
pub struct Store {
    pub current_route: Route,
    pub snapshot: SnapshotEnvelope,
    pub daemon_connected: bool,
    pub daemon_address: String,
    pub right_panel_focus: bool,
    pub cursor: usize,
    pub sub_cursor: usize,
    pub search_input: String,
    pub command_mode: bool,
    pub command_buf: String,
    pub help_open: bool,
    pub logs: Vec<String>,
    pub last_error: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            current_route: Route::Now,
            snapshot: SnapshotEnvelope {
                sequence: 0,
                timestamp: String::new(),
                now: NowSnapshot::default(),
                work: WorkSnapshot::default(),
                explore: ExploreSnapshot::default(),
                govern: GovernSnapshot::default(),
            },
            daemon_connected: false,
            daemon_address: "127.0.0.1:42071".into(),
            right_panel_focus: false,
            cursor: 0,
            sub_cursor: 0,
            search_input: String::new(),
            command_mode: false,
            command_buf: String::new(),
            help_open: false,
            logs: Vec::new(),
            last_error: None,
        }
    }
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_snapshot(&mut self, env: SnapshotEnvelope) {
        self.snapshot = env;
    }

    pub fn add_log(&mut self, log: impl Into<String>) {
        self.logs.push(log.into());
        if self.logs.len() > 500 {
            self.logs.remove(0);
        }
    }
}
