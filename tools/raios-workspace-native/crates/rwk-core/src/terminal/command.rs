//! The child process of a session: an allowlisted program and arguments, never command text.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;

use crate::error::{AppError, ErrorCode, Result};
use crate::sessions::SessionKind;

/// Longest first prompt handed to an agent, in characters. The daemon enforces the same limit
/// when it validates the launch.
pub const MAX_PROMPT_CHARS: usize = 600;

/// Environment variables of the parent that must not leak into a session: a desktop session is
/// a fresh wrapper run, not a nested child of whatever agent started the workspace.
const INHERITED_RUN_VARIABLES: [&str; 2] = ["RAIOS_WRAPPER_RUN_ID", "RAIOS_AGENT_IDENTITY"];

/// Where the programs of a session are found. `None` means: look it up the normal way.
#[derive(Debug, Clone, Default)]
pub struct ProgramPaths {
    /// The shell for [`SessionKind::Shell`]. Default: `$SHELL`, else `/bin/sh`.
    pub shell: Option<PathBuf>,
    /// The `raios` executable for agent sessions. Default: the first `raios` on `PATH`.
    pub raios: Option<PathBuf>,
}

/// Checks an optional first prompt and returns it trimmed, or `None` when it is empty.
pub fn validate_prompt(prompt: Option<&str>) -> Result<Option<String>> {
    let Some(prompt) = prompt.map(str::trim).filter(|prompt| !prompt.is_empty()) else {
        return Ok(None);
    };
    if prompt.chars().count() > MAX_PROMPT_CHARS || prompt.starts_with('-') || prompt.contains('\0')
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            format!(
                "A prompt has at most {MAX_PROMPT_CHARS} characters and cannot begin with '-'."
            ),
        ));
    }
    Ok(Some(prompt.to_owned()))
}

/// Builds the command for one session in `root`.
///
/// Agents run as `raios run <agent> --project <root> [prompt]`; the prompt is one argument, never
/// parsed by a shell. A shell session runs the user's shell with no arguments.
pub fn build_command(
    programs: &ProgramPaths,
    root: &Path,
    kind: SessionKind,
    prompt: Option<&str>,
) -> Result<CommandBuilder> {
    let prompt = validate_prompt(prompt)?;
    let mut command = if let Some(agent) = kind.agent() {
        let raios = match &programs.raios {
            Some(path) => path.clone(),
            None => find_in_path(OsStr::new("raios")).ok_or_else(|| {
                AppError::new(
                    ErrorCode::NotFound,
                    "Install the RAIOS CLI before starting an agent.",
                )
            })?,
        };
        let mut command = CommandBuilder::new(raios);
        command.args(["run", agent, "--project"]);
        command.arg(root);
        if let Some(prompt) = &prompt {
            command.arg(prompt);
        }
        command
    } else {
        let shell = match &programs.shell {
            Some(path) => path.clone(),
            None => {
                let configured = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
                find_in_path(&configured).ok_or_else(|| {
                    AppError::new(ErrorCode::NotFound, "No local shell is available.")
                })?
            }
        };
        CommandBuilder::new(shell)
    };
    command.cwd(root);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    for name in INHERITED_RUN_VARIABLES {
        command.env_remove(name);
    }
    Ok(command)
}

/// Resolves a program name against `PATH`; a name containing a slash is checked as given.
fn find_in_path(name: &OsStr) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return is_executable(candidate).then(|| candidate.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}
