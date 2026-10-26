//! One language server per language for the open project, started when the first file of that
//! language is opened. All of it runs on a manager thread, so starting a slow server (or
//! waiting for one that crashed) never blocks the window; the window talks to it through a
//! handle and reads its events from a channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::client::{LspClient, ServerEvent};
use super::servers::{find_program, spec_for_path, ServerSpec};
use super::uri::{path_to_uri, uri_to_relative};
use crate::error::Result;

/// Starts a server for a catalog entry in a project root.
pub type Launcher =
    Arc<dyn Fn(&ServerSpec, &Path) -> Result<(LspClient, Receiver<ServerEvent>)> + Send + Sync>;

/// The launcher the application uses: the catalog entry's program from `PATH`.
pub fn default_launcher() -> Launcher {
    Arc::new(|spec, root| match find_program(spec) {
        Some(program) => LspClient::start(spec, &program, root),
        None => Err(crate::error::AppError::new(
            crate::error::ErrorCode::NotFound,
            format!("{} is not installed", spec.program),
        )),
    })
}

/// How a server is doing, for the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// The server is being started.
    Starting,
    /// The server answered its handshake.
    Ready,
    /// The server could not be started (usually: not installed). Files of its language simply
    /// have no language features.
    Unavailable(String),
    /// The server died and could not be brought back.
    Crashed,
}

/// What the manager reports.
#[derive(Debug, Clone, PartialEq)]
pub enum ManagerEvent {
    /// The diagnostics of one project file changed (empty: cleared).
    Diagnostics {
        /// Project-relative path.
        path: String,
        /// The current diagnostics.
        diagnostics: Vec<lsp_types::Diagnostic>,
    },
    /// A server's state changed.
    Status {
        /// The catalog name (`rust`, `typescript`, …).
        server: &'static str,
        /// Its state.
        status: ServerStatus,
    },
}

enum Message {
    Open {
        path: String,
        text: String,
    },
    Change {
        path: String,
        text: String,
    },
    Close {
        path: String,
    },
    FromServer {
        server: &'static str,
        generation: u64,
        event: ServerEvent,
    },
    Shutdown,
}

struct Document {
    language: &'static str,
    server: &'static str,
    version: i32,
    text: String,
}

struct Running {
    client: LspClient,
    generation: u64,
}

#[derive(Default)]
struct ServerSlot {
    running: Option<Running>,
    /// The server could not start (or kept crashing); it is not tried again this session.
    gave_up: bool,
    restarts: u32,
}

/// The window's handle on the manager thread.
pub struct LanguageServers {
    sender: Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl LanguageServers {
    /// Starts the manager for `root`. Events arrive on the returned receiver.
    pub fn new(root: PathBuf, launcher: Launcher) -> (Self, Receiver<ManagerEvent>) {
        let (sender, inbox) = mpsc::channel();
        let (outbox, events) = mpsc::channel();
        let own = sender.clone();
        let thread = std::thread::Builder::new()
            .name("lsp-manager".into())
            .spawn(move || {
                Manager {
                    root,
                    launcher,
                    own,
                    outbox,
                    slots: HashMap::new(),
                    documents: HashMap::new(),
                    next_generation: 1,
                }
                .run(&inbox);
            })
            .ok();
        (Self { sender, thread }, events)
    }

    /// A project file was opened with this text.
    pub fn open(&self, path: &str, text: &str) {
        let _ = self.sender.send(Message::Open {
            path: path.to_owned(),
            text: text.to_owned(),
        });
    }

    /// The text of an open file changed.
    pub fn change(&self, path: &str, text: &str) {
        let _ = self.sender.send(Message::Change {
            path: path.to_owned(),
            text: text.to_owned(),
        });
    }

    /// A project file was closed.
    pub fn close(&self, path: &str) {
        let _ = self.sender.send(Message::Close {
            path: path.to_owned(),
        });
    }

    /// Stops every server and waits for the manager thread. Slow: call it off the UI thread.
    pub fn shutdown(mut self) {
        let _ = self.sender.send(Message::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for LanguageServers {
    fn drop(&mut self) {
        // Dropped without `shutdown`: the thread stops the servers and ends on its own.
        let _ = self.sender.send(Message::Shutdown);
    }
}

struct Manager {
    root: PathBuf,
    launcher: Launcher,
    own: Sender<Message>,
    outbox: Sender<ManagerEvent>,
    slots: HashMap<&'static str, ServerSlot>,
    documents: HashMap<String, Document>,
    next_generation: u64,
}

/// Keeps only the last of consecutive changes to the same file, so typing faster than the
/// server reads does not queue a full text per keystroke.
fn coalesce(batch: Vec<Message>) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::with_capacity(batch.len());
    for message in batch {
        if let (Message::Change { path, .. }, Some(Message::Change { path: last, .. })) =
            (&message, out.last())
        {
            if path == last {
                out.pop();
            }
        }
        out.push(message);
    }
    out
}

impl Manager {
    fn run(mut self, inbox: &Receiver<Message>) {
        while let Ok(first) = inbox.recv() {
            let mut batch = vec![first];
            batch.extend(inbox.try_iter());
            for message in coalesce(batch) {
                if matches!(message, Message::Shutdown) {
                    self.stop_all();
                    return;
                }
                self.handle(message);
            }
        }
        self.stop_all();
    }

    fn stop_all(&mut self) {
        for slot in self.slots.values_mut() {
            if let Some(running) = slot.running.take() {
                running.client.shutdown();
            }
        }
    }

    fn say(&self, event: ManagerEvent) {
        let _ = self.outbox.send(event);
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Open { path, text } => self.open(path, text),
            Message::Change { path, text } => {
                let Some(document) = self.documents.get_mut(&path) else {
                    return;
                };
                document.version += 1;
                document.text = text;
                let (server, version) = (document.server, document.version);
                let uri = path_to_uri(&self.root, &path);
                let text = document.text.clone();
                if let Some(running) = self.slots.get(server).and_then(|s| s.running.as_ref()) {
                    let _ = running.client.did_change(&uri, version, &text);
                }
            }
            Message::Close { path } => {
                let Some(document) = self.documents.remove(&path) else {
                    return;
                };
                if let Some(running) = self
                    .slots
                    .get(document.server)
                    .and_then(|s| s.running.as_ref())
                {
                    let _ = running.client.did_close(&path_to_uri(&self.root, &path));
                }
                self.say(ManagerEvent::Diagnostics {
                    path,
                    diagnostics: Vec::new(),
                });
            }
            Message::FromServer {
                server,
                generation,
                event,
            } => self.on_server_event(server, generation, event),
            Message::Shutdown => {}
        }
    }

    fn open(&mut self, path: String, text: String) {
        let Some((spec, language)) = spec_for_path(&path) else {
            return;
        };
        self.documents.insert(
            path.clone(),
            Document {
                language,
                server: spec.name,
                version: 1,
                text: text.clone(),
            },
        );
        if self
            .slots
            .get(spec.name)
            .is_none_or(|s| s.running.is_none())
        {
            if self.slots.get(spec.name).is_some_and(|s| s.gave_up) {
                return;
            }
            // Starting opens every document of the language, this one included.
            self.start(spec);
            return;
        }
        if let Some(running) = self.slots.get(spec.name).and_then(|s| s.running.as_ref()) {
            let _ = running
                .client
                .did_open(&path_to_uri(&self.root, &path), language, 1, &text);
        }
    }

    fn start(&mut self, spec: &'static ServerSpec) {
        self.say(ManagerEvent::Status {
            server: spec.name,
            status: ServerStatus::Starting,
        });
        match (self.launcher)(spec, &self.root) {
            Ok((client, receiver)) => {
                let generation = self.next_generation;
                self.next_generation += 1;
                let own = self.own.clone();
                let name = spec.name;
                let forwarded = std::thread::Builder::new()
                    .name("lsp-forward".into())
                    .spawn(move || {
                        for event in receiver {
                            let exited = event == ServerEvent::Exited;
                            let _ = own.send(Message::FromServer {
                                server: name,
                                generation,
                                event,
                            });
                            if exited {
                                break;
                            }
                        }
                    });
                if forwarded.is_err() {
                    client.shutdown();
                    self.give_up(spec.name, "could not start a thread".into());
                    return;
                }
                for (path, document) in self.documents.iter().filter(|(_, d)| d.server == spec.name)
                {
                    let _ = client.did_open(
                        &path_to_uri(&self.root, path),
                        document.language,
                        document.version,
                        &document.text,
                    );
                }
                self.slots.entry(spec.name).or_default().running =
                    Some(Running { client, generation });
                self.say(ManagerEvent::Status {
                    server: spec.name,
                    status: ServerStatus::Ready,
                });
            }
            Err(error) => self.give_up(spec.name, error.message),
        }
    }

    fn give_up(&mut self, server: &'static str, reason: String) {
        let slot = self.slots.entry(server).or_default();
        slot.running = None;
        slot.gave_up = true;
        self.say(ManagerEvent::Status {
            server,
            status: ServerStatus::Unavailable(reason),
        });
    }

    fn on_server_event(&mut self, server: &'static str, generation: u64, event: ServerEvent) {
        // Events of a server that was already replaced belong to nobody.
        let current = self
            .slots
            .get(server)
            .and_then(|s| s.running.as_ref())
            .is_some_and(|running| running.generation == generation);
        if !current {
            return;
        }
        match event {
            ServerEvent::Diagnostics {
                uri, diagnostics, ..
            } => {
                let Some(path) = uri_to_relative(&self.root, &uri) else {
                    return;
                };
                // Diagnostics for a file that is not open are not shown.
                if self
                    .documents
                    .get(&path)
                    .is_some_and(|d| d.server == server)
                {
                    self.say(ManagerEvent::Diagnostics { path, diagnostics });
                }
            }
            ServerEvent::Exited => {
                let slot = self.slots.entry(server).or_default();
                slot.running = None;
                slot.restarts += 1;
                let spec = super::servers::SERVERS.iter().find(|s| s.name == server);
                match spec {
                    Some(spec) if slot.restarts <= 1 => self.start(spec),
                    _ => {
                        slot.gave_up = true;
                        self.say(ManagerEvent::Status {
                            server,
                            status: ServerStatus::Crashed,
                        });
                    }
                }
            }
        }
    }
}
