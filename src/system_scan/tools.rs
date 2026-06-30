use std::path::PathBuf;
use std::process::Command;

use super::{SystemAiTool, ToolStatus};

pub(super) fn check_ollama() -> SystemAiTool {
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

pub(super) fn check_npm_tool(cmd: &str, name: &str) -> SystemAiTool {
    match crate::core::process::resolve_command_path(cmd) {
        Some(path) => SystemAiTool {
            name: name.into(),
            status: ToolStatus::Installed,
            version: None,
            path: Some(path),
        },
        None => SystemAiTool {
            name: name.into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        },
    }
}

pub(super) fn check_cursor() -> SystemAiTool {
    if let Some(p) = find_existing_path(&cursor_candidates()) {
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

pub(super) fn check_lm_studio() -> SystemAiTool {
    if let Some(p) = find_existing_path(&lm_studio_candidates()) {
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

pub(super) fn check_antigravity() -> SystemAiTool {
    let path = crate::core::process::resolve_command_path("antigravity");

    if let Some(p) = path {
        SystemAiTool {
            name: "Antigravity Assistant".into(),
            status: ToolStatus::Running,
            version: Some("Active".into()),
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

pub(super) fn check_opencode() -> SystemAiTool {
    match crate::core::process::resolve_command_path("opencode") {
        Some(path) => SystemAiTool {
            name: "OpenCode".into(),
            status: ToolStatus::Installed,
            version: Some("Active".into()),
            path: Some(path),
        },
        None => SystemAiTool {
            name: "OpenCode".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        },
    }
}

fn find_existing_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.exists()).cloned()
}

fn cursor_candidates() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut paths = Vec::new();

    if let Some(path) = crate::core::process::resolve_command_path("cursor") {
        paths.push(path);
    }

    paths.push(home.join("AppData/Local/Programs/cursor/Cursor.exe"));
    paths.push(PathBuf::from("/Applications/Cursor.app"));
    paths.push(home.join("Applications/Cursor.app"));
    paths.push(PathBuf::from("/usr/bin/cursor"));
    paths.push(PathBuf::from("/usr/local/bin/cursor"));
    paths.push(home.join(".local/bin/cursor"));
    paths
}

fn lm_studio_candidates() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut paths = Vec::new();

    if let Some(path) = crate::core::process::resolve_command_path("lmstudio") {
        paths.push(path);
    }
    if let Some(path) = crate::core::process::resolve_command_path("lm-studio") {
        paths.push(path);
    }

    paths.push(home.join("AppData/Local/LM-Studio/LM Studio.exe"));
    paths.push(PathBuf::from("/Applications/LM Studio.app"));
    paths.push(home.join("Applications/LM Studio.app"));
    paths.push(PathBuf::from("/usr/bin/lmstudio"));
    paths.push(PathBuf::from("/usr/local/bin/lmstudio"));
    paths.push(home.join(".local/bin/lmstudio"));
    paths
}

pub(super) fn scan_env_keys() -> Vec<String> {
    let mut keys = Vec::new();
    let common = ["OPENAI_API_KEY", "ANTHROPIC_API_KEY", "GOOGLE_API_KEY"];
    for key in common {
        if std::env::var(key).is_ok() {
            keys.push(key.to_string());
        }
    }
    keys
}

pub(super) fn scan_local_models() -> Vec<String> {
    let mut models = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();

    let ollama_path = home.join(".ollama/models");
    if ollama_path.exists() {
        models.push(format!("Ollama: {}", ollama_path.display()));
    }

    let hf_path = home.join(".cache/huggingface/hub");
    if hf_path.exists() {
        models.push(format!("HuggingFace Cache: {}", hf_path.display()));
    }

    models
}
