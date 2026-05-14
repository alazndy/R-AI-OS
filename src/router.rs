use crate::cortex::Cortex;
use anyhow::Result;
use std::path::PathBuf;

pub struct AgentRouter {
    cortex: Cortex,
}

impl AgentRouter {
    pub fn init() -> Result<Self> {
        Ok(Self {
            cortex: Cortex::init()?,
        })
    }

    /// Scans agent directories and indexes them for routing.
    pub fn update_agent_index(&mut self) -> Result<()> {
        let home = dirs::home_dir().expect("Home dir not found");

        let paths = vec![
            home.join(".gemini").join("agents"),
            home.join(".gemini")
                .join("extensions")
                .join("maestro")
                .join("agents"),
        ];

        let mut total_indexed = 0;
        for path in paths {
            if path.exists() {
                println!("🔍 Indexing specialists in {}...", path.display());
                total_indexed += self.cortex.index_workspace(&path)?;
            }
        }

        if total_indexed > 0 {
            println!(
                "✨ Indexed {} new specialists. Saving brain map...",
                total_indexed
            );
            self.cortex.rebuild_index();
        }
        Ok(())
    }

    /// Finds the best agent for a given task description.
    pub fn route(&mut self, task: &str) -> Result<Option<String>> {
        // Ensure index is ready (in a real OS we'd do this in background or via cache)
        let _ = self.update_agent_index();

        let results = self.cortex.search(task, 1)?;
        if let Some(best) = results.first() {
            // Extract agent name from file path (e.g. .../coder.md -> coder)
            let path = PathBuf::from(&best.path);
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string());
            return Ok(name);
        }
        Ok(None)
    }
}
