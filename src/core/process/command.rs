use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn command_exists(command: &str) -> bool {
    resolve_command_path(command).is_some()
}

pub fn resolve_command_path(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists().then(|| path.to_path_buf());
    }

    let path_var = std::env::var_os("PATH")?;

    #[cfg(target_os = "windows")]
    let exts: Vec<String> = std::env::var_os("PATHEXT")
        .map(|v| {
            v.to_string_lossy()
                .split(';')
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_else(|| vec![".EXE".into(), ".BAT".into(), ".CMD".into()]);

    #[cfg(not(target_os = "windows"))]
    let exts: Vec<String> = vec![String::new()];

    std::env::split_paths(&path_var).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{command}{ext}")))
            .find(|candidate| candidate.exists())
    })
}

pub fn python_command() -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        if command_exists("py") {
            return ("py".to_string(), vec!["-3".to_string()]);
        }
        ("python".to_string(), Vec::new())
    }

    #[cfg(not(target_os = "windows"))]
    {
        if command_exists("python3") {
            return ("python3".to_string(), Vec::new());
        }
        ("python".to_string(), Vec::new())
    }
}

pub fn shell_command(command: &str) -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        (
            "cmd".to_string(),
            vec!["/C".to_string(), command.to_string()],
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        (
            "sh".to_string(),
            vec!["-lc".to_string(), command.to_string()],
        )
    }
}

pub fn open_in_system_editor(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/c", "start", "", &path.to_string_lossy()])
            .spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?;
        return Ok(());
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(path).spawn()?;
        Ok(())
    }
}

pub fn copy_to_clipboard(text: &str) -> bool {
    #[cfg(target_os = "windows")]
    let mut child = match Command::new("clip.exe").stdin(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };

    #[cfg(target_os = "macos")]
    let mut child = match Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let mut child = {
        let candidates: [(&str, &[&str]); 3] = [
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];

        let mut spawned = None;
        for (program, args) in candidates {
            if let Ok(child) = Command::new(program)
                .args(args)
                .stdin(Stdio::piped())
                .spawn()
            {
                spawned = Some(child);
                break;
            }
        }

        match spawned {
            Some(child) => child,
            None => return false,
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().is_ok()
}

pub fn launch_in_terminal(command: &str, work_dir: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let work_dir = work_dir.to_string_lossy().into_owned();
        if Command::new("wt")
            .args(["-d", &work_dir, "--", "cmd", "/K", command])
            .spawn()
            .is_ok()
        {
            return true;
        }

        let cmd_str = format!("cd /d \"{}\" && {}", work_dir, command);
        return Command::new("cmd")
            .args(["/c", "start", "cmd", "/k", &cmd_str])
            .spawn()
            .is_ok();
    }

    #[cfg(target_os = "macos")]
    {
        let escaped_dir = escape_shell_arg(&work_dir.to_string_lossy());
        let escaped_cmd = escape_applescript(&format!("cd {} && {}", escaped_dir, command));
        return Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "tell application \"Terminal\" to do script \"{}\"",
                    escaped_cmd
                ),
            ])
            .spawn()
            .is_ok();
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let shell_cmd = format!(
            "cd {} && {}; exec sh",
            escape_shell_arg(&work_dir.to_string_lossy()),
            command
        );

        let candidates: [(&str, Vec<String>); 4] = [
            (
                "x-terminal-emulator",
                vec!["-e".into(), "sh".into(), "-lc".into(), shell_cmd.clone()],
            ),
            (
                "gnome-terminal",
                vec![
                    "--working-directory".into(),
                    work_dir.to_string_lossy().into_owned(),
                    "--".into(),
                    "sh".into(),
                    "-lc".into(),
                    shell_cmd.clone(),
                ],
            ),
            (
                "konsole",
                vec![
                    "--workdir".into(),
                    work_dir.to_string_lossy().into_owned(),
                    "-e".into(),
                    "sh".into(),
                    "-lc".into(),
                    shell_cmd.clone(),
                ],
            ),
            (
                "xterm",
                vec!["-e".into(), "sh".into(), "-lc".into(), shell_cmd],
            ),
        ];

        for (program, args) in candidates {
            if Command::new(program).args(&args).spawn().is_ok() {
                return true;
            }
        }

        false
    }
}

fn escape_shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_command_returns_some_program_name() {
        let (program, _args) = python_command();
        assert!(!program.is_empty());
    }
}
