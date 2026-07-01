use super::*;
pub(super) fn cmd_mem(action: MemAction, json: bool) {
    let project_key_for = |project: &Option<String>| -> String {
        let path = project
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| ".".to_string())
            });
        path.replace('/', "-")
    };

    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {e}");
            return;
        }
    };

    match action {
        MemAction::List { project, item_type } => {
            let key = project_key_for(&project);
            let items = raios_core::db::mem_list(&conn, &key).unwrap_or_default();
            let items: Vec<_> = if let Some(t) = &item_type {
                items.into_iter().filter(|i| &i.item_type == t).collect()
            } else {
                items
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&items).unwrap_or_default());
                return;
            }
            if items.is_empty() {
                println!("  No memory items for {}", key);
                return;
            }
            println!("\n  MEMORY ITEMS  {}\n", key);
            for i in &items {
                println!("  [{:<10}] {}  \x1b[90m{}\x1b[0m", i.item_type, i.slug, i.description);
            }
            println!();
        }
        MemAction::Get { slug, project } => {
            let key = project_key_for(&project);
            match raios_core::db::mem_get(&conn, &key, &slug) {
                Ok(Some(item)) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&item).unwrap_or_default());
                    } else {
                        println!("\n  [{}/{}]\n  Type: {}\n  Description: {}\n\n{}\n",
                            item.project_key, item.slug, item.item_type,
                            item.description, item.body);
                    }
                }
                Ok(None) => eprintln!("  Not found: {}", slug),
                Err(e) => eprintln!("  Error: {e}"),
            }
        }
        MemAction::Add { item_type, slug, title, description, body, project } => {
            let key = project_key_for(&project);
            match raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
                project_key: &key,
                item_type: &item_type,
                slug: &slug,
                title: &title,
                description: &description,
                body: &body,
                session_id: None,
            }) {
                Ok(()) => {
                    if json {
                        println!("{{\"ok\":true,\"slug\":\"{}\"}}", slug);
                    } else {
                        println!("  \x1b[32m✓\x1b[0m  {}/{}", key, slug);
                    }
                }
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m  {e}"),
            }
        }
        MemAction::Delete { slug, project } => {
            let key = project_key_for(&project);
            match raios_core::db::mem_delete(&conn, &key, &slug) {
                Ok(true) => println!("  \x1b[32m✓\x1b[0m  deleted {}/{}", key, slug),
                Ok(false) => eprintln!("  Not found: {}", slug),
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m  {e}"),
            }
        }
        MemAction::Export { project } => {
            let key = project_key_for(&project);
            let home = std::env::var("HOME").unwrap_or_default();
            let memory_dir = std::path::PathBuf::from(&home)
                .join(".claude/projects")
                .join(&key)
                .join("memory");
            match raios_core::db::mem_export(&conn, &key, &memory_dir) {
                Ok(n) => {
                    if json {
                        println!("{{\"exported\":{}}}", n);
                    } else {
                        println!("  \x1b[32m✓\x1b[0m  {} item(s) → {}", n, memory_dir.display());
                    }
                }
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m  {e}"),
            }
        }
        MemAction::Sync { agent, project } => {
            let project_path = project.unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| ".".to_string())
            });
            println!("  \x1b[90mScanning transcript for [{}] → {}…\x1b[0m", agent, project_path);
            raios_runtime::session_memory::auto_sync_agent_memory(
                &agent,
                &project_path,
                std::time::SystemTime::UNIX_EPOCH,
                true,
            );
        }
    }
}

