use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct SystemAiTool {
    pub name: String,
    pub status: ToolStatus,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub enum ToolStatus {
    Running,
    Installed,
    Missing,
    #[allow(dead_code)]
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct AiAuditReport {
    pub tools: Vec<SystemAiTool>,
    pub env_keys: Vec<String>,
    pub local_models: Vec<String>,
}

pub fn scan_system() -> AiAuditReport {
    let mut tools = Vec::new();

    // 1. Check Ollama
    tools.push(check_ollama());

    // 2. Check Claude Code
    tools.push(check_npm_tool("claude", "Claude Code"));

    // 3. Check Gemini CLI
    tools.push(check_npm_tool("gemini", "Gemini CLI"));

    // 4. Check Cursor
    tools.push(check_cursor());

    // 5. Check LM Studio (Common paths)
    tools.push(check_lm_studio());

    // 6. Check Antigravity (Assistant)
    tools.push(check_antigravity());

    AiAuditReport {
        tools,
        env_keys: scan_env_keys(),
        local_models: scan_local_models(),
    }
}

fn check_ollama() -> SystemAiTool {
    let output = Command::new("ollama").arg("list").output();
    match output {
        Ok(out) if out.status.success() => SystemAiTool {
            name: "Ollama (Local LLM)".into(),
            status: ToolStatus::Running,
            version: Some("Active".into()),
            path: None,
        },
        _ => SystemAiTool {
            name: "Ollama".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        },
    }
}

fn check_npm_tool(cmd: &str, name: &str) -> SystemAiTool {
    let output = Command::new("where.exe").arg(cmd).output();
    match output {
        Ok(out) if out.status.success() => {
            let path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            SystemAiTool {
                name: name.into(),
                status: ToolStatus::Installed,
                version: None,
                path: Some(PathBuf::from(path_str)),
            }
        }
        _ => SystemAiTool {
            name: name.into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        },
    }
}

fn check_cursor() -> SystemAiTool {
    let home = dirs::home_dir().unwrap_or_default();
    let p = home.join("AppData/Local/Programs/cursor/Cursor.exe");
    if p.exists() {
        SystemAiTool {
            name: "Cursor IDE".into(),
            status: ToolStatus::Installed,
            version: None,
            path: Some(p),
        }
    } else {
        SystemAiTool {
            name: "Cursor IDE".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        }
    }
}

fn check_lm_studio() -> SystemAiTool {
    let home = dirs::home_dir().unwrap_or_default();
    let p = home.join("AppData/Local/LM-Studio/LM Studio.exe");
    if p.exists() {
        SystemAiTool {
            name: "LM Studio".into(),
            status: ToolStatus::Installed,
            version: None,
            path: Some(p),
        }
    } else {
        SystemAiTool {
            name: "LM Studio".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        }
    }
}

fn scan_env_keys() -> Vec<String> {
    let mut keys = Vec::new();
    let common = [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
    ];
    for key in common {
        if std::env::var(key).is_ok() {
            keys.push(key.to_string());
        }
    }
    keys
}

fn scan_local_models() -> Vec<String> {
    let mut models = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();

    // Antigravity data
    let ag_path = home.join(".gemini/antigravity");
    if ag_path.exists() {
        models.push(format!("Antigravity Home: {}", ag_path.display()));

        let skills_count = std::fs::read_dir(ag_path.join("skills"))
            .map(|d| d.count())
            .unwrap_or(0);
        models.push(format!("Antigravity Skills: {} deployed", skills_count));

        let brain_count = std::fs::read_dir(ag_path.join("brain"))
            .map(|d| d.count())
            .unwrap_or(0);
        models.push(format!(
            "Antigravity Brain: {} sessions stored",
            brain_count
        ));
    }

    // Ollama models path
    let ollama_path = home.join(".ollama/models");
    if ollama_path.exists() {
        models.push(format!("Ollama: {}", ollama_path.display()));
    }

    // HuggingFace cache
    let hf_path = home.join(".cache/huggingface/hub");
    if hf_path.exists() {
        models.push(format!("HuggingFace Cache: {}", hf_path.display()));
    }

    models
}

fn check_antigravity() -> SystemAiTool {
    let home = dirs::home_dir().unwrap_or_default();
    let p = home.join(".gemini/antigravity");
    if p.exists() {
        SystemAiTool {
            name: "Antigravity Assistant".into(),
            status: ToolStatus::Running, // If I'm here, I'm running
            version: Some("Active (v4.0)".into()),
            path: Some(p),
        }
    } else {
        SystemAiTool {
            name: "Antigravity Assistant".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        }
    }
}
