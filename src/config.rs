use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use anyhow::Result;

/// Runtime config — loaded from ~/.config/raios/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Root workspace folder (e.g. Desktop/Dev Ops)
    pub dev_ops_path: PathBuf,
    /// Path to MASTER.md (central agent constitution)
    pub master_md_path: PathBuf,
    /// Path to .agents/skills directory
    pub skills_path: PathBuf,
    /// Path to Obsidian Vault Projects folder
    pub vault_projects_path: PathBuf,
}

impl Config {
    /// Returns the path to the config file.
    pub fn config_file() -> PathBuf {
        dirs::config_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("config.toml")
    }

    /// Load config from disk. Returns None if it doesn't exist yet.
    pub fn load() -> Option<Self> {
        let path = Self::config_file();
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    }

    /// Persist config to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Try to build a config by auto-detecting common paths.
    /// Returns a DetectResult with found paths and a list of what is missing.
    pub fn auto_detect() -> DetectResult {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let desktop = dirs::desktop_dir().unwrap_or_else(|| home.join("Desktop"));

        // ── dev_ops_path ─────────────────────────────────────────────────────
        let dev_ops = find_dev_ops(&desktop, &home);

        // ── master_md_path ────────────────────────────────────────────────────
        let master_md = find_master_md(&home, dev_ops.as_ref());

        // ── skills_path ───────────────────────────────────────────────────────
        let skills = find_skills(dev_ops.as_ref());

        // ── vault_projects_path ──────────────────────────────────────────────
        let vault_projects = find_vault_projects(&home, master_md.as_ref());

        DetectResult { dev_ops, master_md, skills, vault_projects }
    }
}

pub struct DetectResult {
    pub dev_ops:   Option<PathBuf>,
    pub master_md: Option<PathBuf>,
    pub skills:    Option<PathBuf>,
    pub vault_projects: Option<PathBuf>,
}


// ─── Auto-detect helpers ──────────────────────────────────────────────────────

fn find_dev_ops(desktop: &PathBuf, home: &PathBuf) -> Option<PathBuf> {
    // Common names for the workspace root
    let candidates = [
        desktop.join("Dev Ops"),
        home.join("Dev Ops"),
        desktop.join("devops"),
        home.join("devops"),
        desktop.join("workspace"),
        home.join("workspace"),
        home.join("projects"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

fn find_master_md(home: &PathBuf, dev_ops: Option<&PathBuf>) -> Option<PathBuf> {
    // 1. Search Obsidian vaults
    let obsidian_root = home.join("Documents").join("Obsidian Vaults");
    if obsidian_root.is_dir() {
        if let Ok(vaults) = std::fs::read_dir(&obsidian_root) {
            for vault in vaults.filter_map(|e| e.ok()) {
                let candidate = vault.path().join("MASTER.md");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    // 2. Check AI OS / System inside Dev Ops
    if let Some(base) = dev_ops {
        let candidate = base.join("AI OS").join("System").join("MASTER.md");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 3. Home dir
    let h = home.join("MASTER.md");
    if h.exists() { return Some(h); }

    None
}

fn find_skills(dev_ops: Option<&PathBuf>) -> Option<PathBuf> {
    // Common locations under dev_ops and home
    if let Some(base) = dev_ops {
        let candidate = base
            .join("AI OS")
            .join("System")
            .join(".agents")
            .join("skills");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    // Try Antigravity/global skills
    let home = dirs::home_dir()?;
    let ag = home.join(".gemini").join("antigravity").join("skills");
    if ag.is_dir() { return Some(ag); }

    None
}

fn find_vault_projects(home: &PathBuf, master_md: Option<&PathBuf>) -> Option<PathBuf> {
    if let Some(master) = master_md {
        if let Some(vault_root) = master.parent() {
            let candidate = vault_root.join("Projeler");
            if candidate.is_dir() { return Some(candidate); }
            let candidate2 = vault_root.join("01_Projects");
            if candidate2.is_dir() { return Some(candidate2); }
            let candidate3 = vault_root.join("Dev Ops Projeleri");
            if candidate3.is_dir() { return Some(candidate3); }
        }
    }

    let obsidian_root = home.join("Documents").join("Obsidian Vaults");
    if obsidian_root.is_dir() {
        if let Ok(vaults) = std::fs::read_dir(&obsidian_root) {
            for vault in vaults.filter_map(|e| e.ok()) {
                let candidate = vault.path().join("Projeler");
                if candidate.is_dir() { return Some(candidate); }
            }
        }
    }

    None
}
