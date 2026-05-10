use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct InstinctData {
    pub session_count: u64,
    pub preferences: HashMap<String, String>,
    pub learned_rules: Vec<String>,
}

pub struct InstinctEngine {
    path: PathBuf,
    pub data: InstinctData,
}

impl InstinctEngine {
    pub fn init() -> Self {
        let home = dirs::home_dir().expect("Home dir not found");
        let path = home.join(".agents").join("instincts.json");
        
        let data = if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            InstinctData::default()
        };

        Self { path, data }
    }

    pub fn add_rule(&mut self, rule: String) {
        if !self.data.learned_rules.contains(&rule) {
            self.data.learned_rules.push(rule);
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(&self.data)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    /// Generates a system prompt snippet based on learned instincts.
    pub fn get_instinct_prompt(&self) -> String {
        if self.data.learned_rules.is_empty() {
            return String::new();
        }
        let mut prompt = String::from("\n[RAIOS INSTINCTS - GLOBAL LEARNINGS]\n");
        for rule in &self.data.learned_rules {
            prompt.push_str(&format!("- {}\n", rule));
        }
        prompt
    }
}
