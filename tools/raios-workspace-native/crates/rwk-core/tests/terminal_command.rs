//! How a session's command is built: allowlisted programs, a prompt that is one argument, and a
//! clean environment.

#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::path::PathBuf;

use rwk_core::error::ErrorCode;
use rwk_core::sessions::{ResumeKey, SessionKind, SessionLaunch};
use rwk_core::terminal::{build_command, validate_prompt, ProgramPaths, MAX_PROMPT_CHARS};

/// A fresh run of `kind`, with no first prompt.
fn fresh() -> SessionLaunch {
    SessionLaunch::default()
}

/// A fresh run of `kind` with one first prompt.
fn fresh_with(prompt: &str) -> SessionLaunch {
    SessionLaunch::Fresh {
        prompt: Some(prompt.to_owned()),
        name: None,
    }
}
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
    let command = build_command(
        &programs(),
        &root,
        SessionKind::Opencode,
        &fresh_with(prompt),
    )
    .unwrap();

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
    let command = build_command(&programs(), &root, SessionKind::Shell, &fresh()).unwrap();

    assert_eq!(argv(&command), vec!["/bin/sh"]);
    assert_eq!(
        command.get_cwd().map(OsString::as_os_str),
        Some(root.as_os_str())
    );
}

#[test]
fn every_session_gets_the_terminal_environment_and_no_inherited_run_identity() {
    for kind in [SessionKind::Shell, SessionKind::Claude, SessionKind::Agy] {
        let command =
            build_command(&programs(), &PathBuf::from("/work/app"), kind, &fresh()).unwrap();
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
        &fresh_with("--x"),
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
        &fresh(),
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
        &fresh(),
    )
    .is_ok());
}

#[test]
fn a_resume_runs_raios_resume_with_the_key_as_one_argument() {
    let root = PathBuf::from("/work/app");
    let key = ResumeKey::parse("claude:3294c005-aee0-4e95-860f-5800e045b624").expect("valid key");

    let command = build_command(
        &programs(),
        &root,
        SessionKind::Claude,
        &SessionLaunch::Resume(key),
    )
    .unwrap();

    assert_eq!(
        argv(&command),
        vec![
            "/opt/raios/bin/raios",
            "resume",
            "claude:3294c005-aee0-4e95-860f-5800e045b624",
        ],
        "the key is one argument, never split or shell-parsed"
    );
    assert_eq!(
        command.get_cwd().map(OsString::as_os_str),
        Some(root.as_os_str())
    );
}

#[test]
fn a_resume_of_another_agent_is_refused_before_anything_starts() {
    let key = ResumeKey::parse("claude:3294c005-aee0-4e95-860f-5800e045b624").expect("valid key");

    let error = build_command(
        &programs(),
        &PathBuf::from("/work/app"),
        SessionKind::Codex,
        &SessionLaunch::Resume(key),
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(
        error.message.contains("does not match"),
        "{}",
        error.message
    );
}

#[test]
fn a_resume_gets_the_same_clean_environment_as_any_session() {
    let key = ResumeKey::parse("opencode:ses_abc123").expect("valid key");

    let command = build_command(
        &programs(),
        &PathBuf::from("/work/app"),
        SessionKind::Opencode,
        &SessionLaunch::Resume(key),
    )
    .unwrap();

    assert_eq!(
        command
            .get_env("TERM")
            .map(|value| value.to_string_lossy().into_owned()),
        Some("xterm-256color".into())
    );
    assert!(command.get_env("RAIOS_WRAPPER_RUN_ID").is_none());
    assert!(command.get_env("RAIOS_AGENT_IDENTITY").is_none());
}

#[test]
fn an_agent_name_is_one_name_argument_before_the_prompt_and_a_shell_ignores_it() {
    use rwk_core::sessions::SessionName;

    let root = PathBuf::from("/work/app");
    let launch = SessionLaunch::Fresh {
        prompt: Some("fix it".to_owned()),
        name: Some(SessionName::parse("  api; rm -rf /  ").unwrap()),
    };
    let command = build_command(&programs(), &root, SessionKind::Codex, &launch).unwrap();
    assert_eq!(
        argv(&command),
        [
            "/opt/raios/bin/raios",
            "run",
            "codex",
            "--project",
            "/work/app",
            "--name",
            "api; rm -rf /",
            "fix it",
        ]
    );

    let shell = build_command(&programs(), &root, SessionKind::Shell, &launch).unwrap();
    assert_eq!(argv(&shell), ["/bin/sh"]);
}

#[test]
fn a_session_name_is_trimmed_and_refuses_what_could_harm_the_command_or_the_ledger() {
    use rwk_core::sessions::{SessionName, MAX_NAME_CHARS};

    assert_eq!(
        SessionName::parse("  build box ").unwrap().as_str(),
        "build box"
    );
    for bad in [
        "",
        "   ",
        "-rf",
        "--name",
        "tab\there",
        "nul\0x",
        "line\nbreak",
    ] {
        assert_eq!(
            SessionName::parse(bad).unwrap_err().code,
            ErrorCode::InvalidInput,
            "{bad:?}"
        );
    }
    assert!(SessionName::parse(&"é".repeat(MAX_NAME_CHARS)).is_ok());
    assert!(SessionName::parse(&"é".repeat(MAX_NAME_CHARS + 1)).is_err());
    assert!(SessionName::parse("token sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789").is_err());
    // A saved file goes through the same rules.
    assert!(serde_json::from_str::<SessionName>("\"ok\"").is_ok());
    assert!(serde_json::from_str::<SessionName>("\"-bad\"").is_err());
}
