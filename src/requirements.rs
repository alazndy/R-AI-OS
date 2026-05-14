use std::process::Command;

#[derive(Debug, Clone)]
pub struct Requirement {
    pub name: &'static str,
    pub command: &'static str,
    pub installed: bool,
    pub version: String,
    pub critical: bool,
}

pub fn check_requirements() -> Vec<Requirement> {
    vec![
        check("Node.js", "node", "--version", true),
        check("NPM", "npm", "--version", true),
        check("Rust / Cargo", "cargo", "--version", true),
        check("Git", "git", "--version", false),
        check("Claude Code", "claude", "--version", false),
        check("Gemini CLI", "gemini", "--version", false),
    ]
}

fn check(name: &'static str, cmd: &'static str, args: &'static str, critical: bool) -> Requirement {
    let output = Command::new("cmd").args(["/c", cmd, args]).output();

    match output {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            Requirement {
                name,
                command: cmd,
                installed: true,
                version: ver,
                critical,
            }
        }
        _ => Requirement {
            name,
            command: cmd,
            installed: false,
            version: "Not found".into(),
            critical,
        },
    }
}
