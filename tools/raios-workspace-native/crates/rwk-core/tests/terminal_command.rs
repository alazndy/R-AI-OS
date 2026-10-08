//! How a session's command is built: allowlisted programs, a prompt that is one argument, and a
//! clean environment.

#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::path::PathBuf;

use rwk_core::error::ErrorCode;
use rwk_core::sessions::SessionKind;
use rwk_core::terminal::{build_command, validate_prompt, ProgramPaths, MAX_PROMPT_CHARS};
fn argv(command: &portable_pty::CommandBuilder) -> Vec<String> {
    command
        .get_argv()
        .iter()
        .map(|part| part.to_string_lossy().into_owned())
        .collect()
}

fn programs() -> ProgramPaths {
    ProgramPaths {
        shell: Some(PathBuf::from("/bin/sh")),
        raios: Some(PathBuf::from("/opt/raios/bin/raios")),
    }
}

#[test]
fn an_empty_or_blank_prompt_means_no_prompt_and_a_good_one_is_trimmed() {
    assert_eq!(validate_prompt(None).unwrap(), None);
    assert_eq!(validate_prompt(Some("   \n")).unwrap(), None);
    assert_eq!(
        validate_prompt(Some("  fix the build \n"))
            .unwrap()
            .as_deref(),
        Some("fix the build")
    );
}

#[test]
fn a_prompt_that_could_act_as_an_option_or_is_too_long_is_refused() {
    for bad in ["--dangerous", "-x", "nul\0inside"] {
        assert_eq!(
            validate_prompt(Some(bad)).unwrap_err().code,
            ErrorCode::InvalidInput,
            "{bad:?}"
        );
    }
    assert!(validate_prompt(Some(&"x".repeat(MAX_PROMPT_CHARS))).is_ok());
    assert!(validate_prompt(Some(&"x".repeat(MAX_PROMPT_CHARS + 1))).is_err());
    // The limit counts characters: 600 multi-byte characters are fine.
    assert!(validate_prompt(Some(&"é".repeat(MAX_PROMPT_CHARS))).is_ok());
}

#[test]
fn an_agent_runs_through_the_raios_wrapper_with_the_prompt_as_one_argument() {
    let root = PathBuf::from("/work/my app");
    let prompt = "look at it; rm -rf / && echo $HOME";
    let command = build_command(&programs(), &root, SessionKind::Opencode, Some(prompt)).unwrap();

    assert_eq!(
        argv(&command),
        vec![
            "/opt/raios/bin/raios",
            "run",
            "opencode",
            "--project",
            "/work/my app",
            prompt,
        ]
    );
    assert_eq!(
        command.get_cwd().map(OsString::as_os_str),
        Some(root.as_os_str())
    );
}

#[test]
fn a_shell_session_runs_the_shell_without_arguments_and_ignores_no_prompt() {
    let root = PathBuf::from("/work/app");
    let command = build_command(&programs(), &root, SessionKind::Shell, None).unwrap();

    assert_eq!(argv(&command), vec!["/bin/sh"]);
    assert_eq!(
        command.get_cwd().map(OsString::as_os_str),
        Some(root.as_os_str())
    );
}

#[test]
fn every_session_gets_the_terminal_environment_and_no_inherited_run_identity() {
    for kind in [SessionKind::Shell, SessionKind::Claude, SessionKind::Agy] {
        let command = build_command(&programs(), &PathBuf::from("/work/app"), kind, None).unwrap();
        assert_eq!(
            command
                .get_env("TERM")
                .map(|v| v.to_string_lossy().into_owned()),
            Some("xterm-256color".into())
        );
        assert_eq!(
            command
                .get_env("COLORTERM")
                .map(|v| v.to_string_lossy().into_owned()),
            Some("truecolor".into())
        );
        assert!(command.get_env("RAIOS_WRAPPER_RUN_ID").is_none());
        assert!(command.get_env("RAIOS_AGENT_IDENTITY").is_none());
    }
}

#[test]
fn a_bad_prompt_is_refused_before_anything_is_built() {
    let error = build_command(
        &programs(),
        &PathBuf::from("/work/app"),
        SessionKind::Codex,
        Some("--x"),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn a_missing_raios_executable_is_reported_as_not_found() {
    let nothing = ProgramPaths {
        shell: None,
        raios: Some(PathBuf::from("/nonexistent/raios")),
    };
    // An explicit path is trusted as given; the spawn itself then fails. Only a missing PATH
    // lookup is reported here, so make the lookup impossible.
    let lookup = ProgramPaths {
        shell: None,
        raios: None,
    };
    let saved = std::env::var_os("PATH");
    std::env::set_var("PATH", "/nonexistent-directory");
    let result = build_command(
        &lookup,
        &PathBuf::from("/work/app"),
        SessionKind::Claude,
        None,
    );
    match saved {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }
    assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
    assert!(build_command(
        &nothing,
        &PathBuf::from("/work/app"),
        SessionKind::Claude,
        None
    )
    .is_ok());
}
