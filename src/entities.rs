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
    let mut known = load_entities(dev_ops);
    let rooms = crate::mempalace::build(dev_ops);
    
    for room in rooms {
        for proj in room.projects {
            // Check if local_path matches (normalized-ish)
            if !known.iter().any(|k| k.local_path == proj.path) {
                known.push(EntityProject {
                    name: proj.name,
                    category: room.folder_name.clone(),
                    local_path: proj.path,
                    github: None,
                    status: proj.status,
                    stars: None,
                    last_commit: None,
                });
            }
        }
    }
    known
}

pub fn save_entities(dev_ops: &Path, projects: Vec<EntityProject>) -> std::io::Result<()> {
    let path = dev_ops.join("entities.json");
    let ef = EntitiesFile { projects };
    let content = serde_json::to_string_pretty(&ef).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, content)
}
