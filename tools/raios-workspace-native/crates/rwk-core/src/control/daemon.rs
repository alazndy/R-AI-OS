//! HTTP client for the daemon's `/api/v1/control/query` and `/api/v1/control/command` routes.
//!
//! Every failure mode fails closed: an unreachable daemon, a rejected token, an HTTP error, a
//! daemon `Problem` and a malformed response all return an error, never default data.

use std::path::{Path, PathBuf};
use std::time::Duration;

use raios_contracts::{
    Command, CommandPolicyDto, GovernSnapshot, HandoffReceiptDto, Problem, Query, WorkSnapshot,
};
use serde_json::Value;

use super::{
    ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest, PolicyAction,
    QueuedHandoff, RegistrationOutcome,
};
use crate::error::{AppError, ErrorCode, Result};

const QUERY_PATH: &str = "/api/v1/control/query";
const COMMAND_PATH: &str = "/api/v1/control/command";
/// Longest a single request may take before it is treated as an unreachable daemon.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Supplies the loopback session bearer token the daemon's auth middleware validates.
/// The production implementation lives in the application crate, which reads the daemon's
/// session token file, so `rwk-core` does not depend on `raios-core`.
pub trait TokenSource: Send + Sync {
    /// Returns a currently valid bearer token.
    fn bearer_token(&self) -> Result<String>;
}

/// Blocking HTTP client for the daemon. Call it from a background executor, never from the
/// UI thread.
pub struct DaemonClient {
    base_url: String,
    tokens: Box<dyn TokenSource>,
}

impl DaemonClient {
    /// Default loopback port of the daemon's HTTP API (`http_port` in `raios-policy.toml`).
    pub const DEFAULT_PORT: u16 = 42071;

    /// Creates a client for `base_url`, for example `http://127.0.0.1:42071`.
    pub fn new(base_url: impl Into<String>, tokens: Box<dyn TokenSource>) -> Self {
        Self {
            base_url: base_url.into(),
            tokens,
        }
    }

    /// The base URL requests are sent to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn query(&self, query: &Query) -> Result<Value> {
        self.post(QUERY_PATH, &serialize(query)?)
    }

    fn command(&self, command: &Command) -> Result<Value> {
        self.post(COMMAND_PATH, &serialize(command)?)
    }

    /// Posts one JSON body and returns the response object once its `status` is `ok`.
    fn post(&self, path: &str, body: &str) -> Result<Value> {
        let token = self.tokens.bearer_token()?;
        let response = ureq::post(&format!("{}{path}", self.base_url))
            .timeout(REQUEST_TIMEOUT)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/json")
            .send_string(body);
        let text = match response {
            Ok(response) => response
                .into_string()
                .map_err(|error| daemon_error(format!("could not read the response: {error}")))?,
            Err(ureq::Error::Status(401 | 403, _)) => {
                return Err(AppError::new(
                    ErrorCode::Unauthorized,
                    "The daemon rejected the session token.",
                ))
            }
            Err(ureq::Error::Status(code, _)) => {
                return Err(daemon_error(format!("the daemon answered HTTP {code}")))
            }
            Err(ureq::Error::Transport(error)) => {
                return Err(AppError::new(
                    ErrorCode::DaemonUnreachable,
                    format!("cannot reach the daemon: {error}"),
                ))
            }
        };
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| daemon_error("the daemon sent a response that is not JSON"))?;
        match value.get("status").and_then(Value::as_str) {
            Some("ok") => Ok(value),
            Some("error") => Err(problem_error(&value)),
            _ => Err(daemon_error("the daemon response has no status")),
        }
    }
}

fn serialize<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|error| {
        AppError::new(
            ErrorCode::Internal,
            format!("could not encode the request: {error}"),
        )
    })
}

fn daemon_error(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Daemon, message)
}

/// Converts the daemon's `{"status":"error","problem":{...}}` into a stable error.
fn problem_error(value: &Value) -> AppError {
    match value
        .get("problem")
        .cloned()
        .and_then(|problem| serde_json::from_value::<Problem>(problem).ok())
    {
        Some(problem) => {
            AppError::new(ErrorCode::from_problem_code(&problem.code), problem.message)
        }
        None => daemon_error("the daemon reported an error without a problem"),
    }
}

/// Takes the named field out of a response object and decodes it.
fn take<T: serde::de::DeserializeOwned>(mut response: Value, field: &str) -> Result<T> {
    let part = response
        .get_mut(field)
        .map(Value::take)
        .ok_or_else(|| daemon_error(format!("the daemon response lacks '{field}'")))?;
    serde_json::from_value(part)
        .map_err(|error| daemon_error(format!("unexpected shape of '{field}': {error}")))
}

fn text_field(value: &Value, field: &str) -> Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| daemon_error(format!("the daemon result lacks '{field}'")))
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        AppError::new(
            ErrorCode::InvalidInput,
            "The project path is not valid UTF-8.",
        )
    })
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}

impl ControlPlane for DaemonClient {
    fn work_snapshot(&self) -> Result<WorkSnapshot> {
        take(self.query(&Query::GetWorkSnapshot)?, "work")
    }

    fn govern_snapshot(&self) -> Result<GovernSnapshot> {
        take(self.query(&Query::GetGovernSnapshot)?, "govern")
    }

    fn command_policy(&self, command_name: &str) -> Result<PolicyAction> {
        let policy: CommandPolicyDto = take(
            self.query(&Query::GetCommandPolicy {
                command_name: command_name.to_owned(),
            })?,
            "command_policy",
        )?;
        PolicyAction::parse(&policy.action)
    }

    fn register_project(
        &self,
        project_root: &Path,
        key: &IdempotencyKey,
    ) -> Result<RegistrationOutcome> {
        let response = self.command(&Command::RegisterProject {
            project_path: path_text(project_root)?,
            idempotency_key: key.as_str().to_owned(),
        })?;
        let result: Value = take(response, "result")?;
        Ok(RegistrationOutcome {
            project_path: PathBuf::from(text_field(&result, "project_path")?),
            already_registered: text_field(&result, "status")? == "already_registered",
        })
    }

    /// Sends `ValidateAgentLaunch`, never `LaunchAgent`: the latter makes the daemon open the
    /// agent in an external terminal window, while the workspace runs agents in its own PTYs.
    /// Success needs the daemon to answer `validated`; any other answer, an older daemon that
    /// does not know the command (HTTP 422) and an unreachable daemon all refuse the launch.
    fn launch_agent(&self, request: &LaunchAgentRequest) -> Result<()> {
        let agent = request
            .agent
            .agent()
            .ok_or_else(|| invalid("A shell session is not an agent launch."))?;
        let response = self.command(&Command::ValidateAgentLaunch {
            agent_name: agent.to_owned(),
            project_path: path_text(&request.project_root)?,
            prompt: request.prompt.clone(),
            idempotency_key: request.idempotency_key.as_str().to_owned(),
        })?;
        let result: Value = take(response, "result")?;
        match text_field(&result, "status")?.as_str() {
            "validated" => Ok(()),
            other => Err(daemon_error(format!(
                "the daemon answered '{other}' instead of 'validated'; the agent was not started"
            ))),
        }
    }

    fn create_handoff(&self, request: &CreateHandoffRequest) -> Result<QueuedHandoff> {
        let from = request
            .from
            .identity()
            .ok_or_else(|| invalid("The handoff source must be an agent."))?;
        let to = request
            .to
            .identity()
            .ok_or_else(|| invalid("The handoff recipient must be an agent."))?;
        let response = self.command(&Command::CreateHandoff {
            project_path: path_text(&request.project_root)?,
            from_agent: from.to_owned(),
            to_agent: to.to_owned(),
            summary: request.summary.clone(),
            idempotency_key: request.idempotency_key.as_str().to_owned(),
        })?;
        let result: Value = take(response, "result")?;
        Ok(QueuedHandoff {
            approval_id: text_field(&result, "approval_id")?,
            project_path: PathBuf::from(text_field(&result, "project_path")?),
        })
    }

    fn handoff_receipt(&self, approval_id: &str, project_root: &Path) -> Result<HandoffReceiptDto> {
        take(
            self.query(&Query::GetHandoffReceipt {
                approval_id: approval_id.to_owned(),
                project_path: path_text(project_root)?,
            })?,
            "handoff_receipt",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedToken;

    impl TokenSource for FixedToken {
        fn bearer_token(&self) -> Result<String> {
            Ok("token".into())
        }
    }

    #[test]
    fn the_client_remembers_its_base_url() {
        let client = DaemonClient::new("http://127.0.0.1:42071", Box::new(FixedToken));
        assert_eq!(client.base_url(), "http://127.0.0.1:42071");
    }

    #[test]
    fn the_default_port_matches_the_shipped_policy() {
        assert_eq!(DaemonClient::DEFAULT_PORT, 42071);
    }
}
