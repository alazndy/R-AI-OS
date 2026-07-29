//! Control-plane daemon IPC client handle.

use raios_contracts::{Command, FactoryCommand, Problem, Query};
use std::sync::mpsc::Sender;

/// IPC Client handle for sending command and query messages to the daemon.
pub struct Client {
    tx_daemon: Option<Sender<String>>,
}

impl Client {
    /// Creates a new `Client` with an optional daemon message sender.
    pub fn new(tx_daemon: Option<Sender<String>>) -> Self {
        Self { tx_daemon }
    }

    /// Sends a general control-plane command to the daemon.
    pub fn send_command(&self, cmd: Command) -> Result<(), Problem> {
        self.send_serialized(&cmd, "Command")
    }

    /// Ocak (Product Factory) commands have their own transport contract so
    /// they cannot be accidentally accepted by the broader control-plane
    /// dispatcher.
    pub fn send_factory_command(&self, cmd: FactoryCommand) -> Result<(), Problem> {
        self.send_serialized(&cmd, "Ocak command")
    }

    fn send_serialized<T: serde::Serialize>(&self, value: &T, label: &str) -> Result<(), Problem> {
        if let Some(ref tx) = self.tx_daemon {
            let json = serde_json::to_string(value).map_err(|error| {
                Problem::invalid_input(format!("{label} serialization failed: {error}"))
            })?;
            tx.send(json)
                .map_err(|e| Problem::internal(format!("Daemon IPC channel error: {}", e)))?;
            Ok(())
        } else {
            Err(Problem::internal("Daemon IPC disconnected"))
        }
    }

    /// Sends a query request to the daemon.
    pub fn send_query(&self, query: Query) -> Result<(), Problem> {
        if let Some(ref tx) = self.tx_daemon {
            let json = serde_json::to_string(&query).map_err(|e| {
                Problem::invalid_input(format!("Query serialization failed: {}", e))
            })?;
            tx.send(json)
                .map_err(|e| Problem::internal(format!("Daemon IPC channel error: {}", e)))?;
            Ok(())
        } else {
            Err(Problem::internal("Daemon IPC disconnected"))
        }
    }
}
