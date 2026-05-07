use notify::{Watcher, RecursiveMode, Config, Event};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use anyhow::Result;

pub struct FileMonitor {
    pub project_path: PathBuf,
}

impl FileMonitor {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            project_path: path.as_ref().to_path_buf(),
        }
    }

    /// Starts a watcher and returns a receiver for file change events.
    /// Implements debouncing internally or via the consumer.
    pub fn watch(&self) -> Result<Receiver<Result<Event, notify::Error>>> {
        let (tx, rx) = channel();

        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;

        watcher.watch(&self.project_path, RecursiveMode::Recursive)?;
        
        // Note: In a real implementation, the watcher needs to be kept alive.
        // For aiosd integration, we'll store this in the daemon's state.
        
        Ok(rx)
    }
}
