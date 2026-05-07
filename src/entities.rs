use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntityProject {
    pub name: String,
    pub category: String,
    pub local_path: PathBuf,
    pub github: Option<String>,
    pub status: String,
    pub stars: Option<u32>,
    pub last_commit: Option<String>,
    pub version: Option<String>,
    pub version_nickname: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct EntitiesFile {
    #[serde(default)]
    projects: Vec<EntityProject>,
}

pub fn load_entities(dev_ops: &Path) -> Vec<EntityProject> {
    let path = dev_ops.join("entities.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    match serde_json::from_str::<EntitiesFile>(&content) {
        Ok(ef) => ef.projects,
        Err(_) => vec![],
    }
}

pub fn discover_entities(dev_ops: &Path) -> Vec<EntityProject> {
    let known = load_entities(dev_ops);
    let rooms = crate::mempalace::build(dev_ops);

    // Build fresh list from the current scan
    let mut result: Vec<EntityProject> = Vec::new();

    for room in rooms {
        for proj in room.projects {
            // Preserve GitHub URL, stars, last_commit from existing entry
            let existing = known.iter().find(|k| k.local_path == proj.path);
            result.push(EntityProject {
                name: proj.name,
                category: room.folder_name.clone(),
                local_path: proj.path,
                github: existing.and_then(|e| e.github.clone()),
                status: proj.status,
                stars: existing.and_then(|e| e.stars),
                last_commit: existing.and_then(|e| e.last_commit.clone()),
                version: proj.version.or_else(|| existing.and_then(|e| e.version.clone())),
                version_nickname: proj.version_nickname.or_else(|| existing.and_then(|e| e.version_nickname.clone())),
            });
        }
    }

    result
}

pub fn save_entities(dev_ops: &Path, projects: Vec<EntityProject>) -> std::io::Result<()> {
    let path = dev_ops.join("entities.json");
    let ef = EntitiesFile { projects };
    let content = serde_json::to_string_pretty(&ef).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, content)
}
