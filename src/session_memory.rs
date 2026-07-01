use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

struct HeuristicItem {
    item_type: &'static str,
    slug: String,
    title: String,
    description: String,
    body: String,
}

/// Convert an absolute project path to Claude's directory naming convention.
/// `/home/alaz/dev/core/R-AI-OS` → `-home-alaz-dev-core-R-AI-OS`
fn claude_project_dir_name(project_path: &str) -> String {
    project_path.replace('/', "-")
}

/// Find the most recently modified JSONL conversation file for a project,
/// optionally requiring it to have been modified at or after `min_mtime`.
pub fn find_latest_conversation(
    project_path: &str,
    min_mtime: Option<SystemTime>,
) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir_name = claude_project_dir_name(project_path);
    let claude_dir = Path::new(&home).join(".claude/projects").join(&dir_name);

    // CCR sessions (CLAUDE_JOB_DIR is set) always write their JSONL to the home-level
    // project dir (~/.claude/projects/-home-alaz/) regardless of the working project.
    // The job ID is the leading path component of CLAUDE_JOB_DIR and matches the
    // filename prefix of the JSONL (e.g. job 9b3cbb27 → 9b3cbb27-<uuid>.jsonl).
    // Prioritize the CCR JSONL over the primary dir — it is the authoritative transcript
    // for the current session and is always more recent than any leftover project JSONL.
    if let Ok(job_dir) = std::env::var("CLAUDE_JOB_DIR") {
        let job_id = Path::new(&job_dir).file_name()?.to_string_lossy().into_owned();
        let ccr_dir = Path::new(&home).join(".claude/projects/-home-alaz");
        if ccr_dir != claude_dir {
            // Search for the exact JSONL belonging to this job.
            for entry in std::fs::read_dir(&ccr_dir).ok()?.flatten() {
                let path = entry.path();
                let fname = path.file_name().unwrap_or_default().to_string_lossy();
                if path.extension().map(|e| e == "jsonl").unwrap_or(false)
                    && fname.starts_with(&*job_id)
                {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(mtime) = meta.modified() {
                            if min_mtime.map(|m| mtime >= m).unwrap_or(true) {
                                return Some(path);
                            }
                        }
                    }
                }
            }
            // Job JSONL not found (not yet created or older than min_mtime) — fall through
            // to the primary dir scan below.
        }
    }

    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&claude_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
            if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    if let Some(min) = min_mtime {
                        if mtime < min {
                            continue;
                        }
                    }
                    if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                        best = Some((mtime, path));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Extract user + assistant text content from a Claude Code JSONL file.
/// Returns a compact transcript string suitable for summarization.
pub fn extract_transcript(jsonl_path: &Path) -> String {
    let Ok(content) = std::fs::read_to_string(jsonl_path) else {
        return String::new();
    };

    let mut parts: Vec<String> = Vec::new();

    for line in content.lines() {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        match obj["type"].as_str().unwrap_or("") {
            "user" => {
                let text = extract_content_text(&obj["message"]["content"]);
                if !text.trim().is_empty() {
                    parts.push(format!("User: {}", truncate(&text, 600)));
                }
            }
            "assistant" => {
                let text = extract_content_text(&obj["message"]["content"]);
                if !text.trim().is_empty() {
                    parts.push(format!("Assistant: {}", truncate(&text, 800)));
                }
            }
            _ => {}
        }
    }

    parts.join("\n\n")
}

fn extract_content_text(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        return arr
            .iter()
            .filter(|item| item["type"] == "text")
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ");
    }
    String::new()
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        format!("{}…", chars[..max].iter().collect::<String>())
    }
}

/// Run `claude --print` to generate a single memory.md Change Log entry
/// from the given transcript. Returns the raw line(s) Claude produces.
pub fn generate_memory_entry(transcript: &str) -> Option<String> {
    let date = chrono::Local::now().format("%Y-%m-%d");
    let prompt = format!(
        "Based on this Claude Code session transcript, write ONE concise Change Log \
entry for memory.md using this exact format (no extra text, just the line):\n\
`- [{date}] [Claude Kaira]: <brief summary of what was accomplished>`\n\n\
Keep it under 120 characters. Focus on what changed, not how.\n\n\
TRANSCRIPT:\n{}\n",
        &transcript[..transcript.len().min(6000)]
    );

    let output = Command::new("claude")
        .arg("--print")
        .arg(&prompt)
        .env_remove("OPENAI_API_KEY")
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() || !output.status.success() {
        return None;
    }
    // Keep only lines that look like a change log entry
    let entry = text
        .lines()
        .find(|l| l.starts_with("- ["))
        .unwrap_or(text.lines().next().unwrap_or(""))
        .to_string();
    if entry.is_empty() {
        None
    } else {
        Some(entry)
    }
}

/// Append a change log entry to the project's memory.md.
/// Inserts after the `## Change Log` heading (or appends at end if not found).
pub fn append_to_memory_md(project_path: &str, entry: &str) -> std::io::Result<()> {
    let memory_path = Path::new(project_path).join("memory.md");
    if !memory_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&memory_path)?;
    let updated = if content.contains("## Change Log") {
        // Append right before the end of the file
        format!("{}\n{}\n", content.trim_end(), entry)
    } else {
        format!("{}\n\n## Change Log\n{}\n", content.trim_end(), entry)
    };
    std::fs::write(&memory_path, updated)
}

/// Interactive post-session prompt. Called by agent_runner after a claude session.
/// Finds the conversation JSONL, optionally summarizes it, and appends to memory.md.
pub fn post_session_memory_prompt(project_path: &str, session_started: SystemTime) {
    // Find the JSONL that was written during this session
    let Some(jsonl) = find_latest_conversation(project_path, Some(session_started)) else {
        return;
    };

    print!(
        "\n  \x1b[36m✦\x1b[0m  memory.md güncelle?  \x1b[90m[y = oto-özet / s = atla]\x1b[0m  "
    );
    let _ = std::io::stdout().flush();

    let stdin = std::io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_err() {
        return;
    }

    let choice = line.trim().to_lowercase();
    if choice != "y" && choice != "yes" {
        println!("  \x1b[90mAtlandı.\x1b[0m  Sonra elle: raios memory-gen\n");
        return;
    }

    println!("  \x1b[90mKonuşma okunuyor…\x1b[0m");
    let transcript = extract_transcript(&jsonl);
    if transcript.is_empty() {
        println!("  \x1b[33mKonuşma içeriği bulunamadı.\x1b[0m\n");
        return;
    }

    println!("  \x1b[90mÖzet oluşturuluyor (claude --print)…\x1b[0m");
    match generate_memory_entry(&transcript) {
        Some(entry) => {
            println!("  \x1b[32m→\x1b[0m  {}", entry);
            print!("  \x1b[90mEklensin mi? [y/N]\x1b[0m  ");
            let _ = std::io::stdout().flush();
            let mut confirm = String::new();
            let _ = stdin.lock().read_line(&mut confirm);
            if confirm.trim().eq_ignore_ascii_case("y") {
                match append_to_memory_md(project_path, &entry) {
                    Ok(()) => println!("  \x1b[32m✓ memory.md güncellendi\x1b[0m\n"),
                    Err(e) => println!("  \x1b[31m✗ Yazılamadı: {e}\x1b[0m\n"),
                }
            } else {
                println!("  \x1b[90mAtlandı.\x1b[0m\n");
            }
        }
        None => {
            println!("  \x1b[31m✗ Özet üretilemedi (claude --print başarısız).\x1b[0m\n");
        }
    }
}

/// CLI handler for `raios memory-gen` — manual invocation after a session.
pub fn cmd_memory_gen(project: Option<&str>, json: bool) {
    let project_path = match project {
        Some(p) => p.to_string(),
        None => std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string()),
    };

    let Some(jsonl) = find_latest_conversation(&project_path, None) else {
        if json {
            eprintln!("{{\"error\":\"no conversation found\"}}");
        } else {
            eprintln!("Konuşma dosyası bulunamadı: {}", project_path);
        }
        return;
    };

    if !json {
        println!("Konuşma: {}", jsonl.display());
    }

    let transcript = extract_transcript(&jsonl);
    if transcript.is_empty() {
        if json {
            eprintln!("{{\"error\":\"empty transcript\"}}");
        } else {
            eprintln!("Konuşma içeriği boş.");
        }
        return;
    }

    let Some(entry) = generate_memory_entry(&transcript) else {
        if json {
            eprintln!("{{\"error\":\"generation failed\"}}");
        } else {
            eprintln!("Özet üretilemedi.");
        }
        return;
    };

    if json {
        println!("{}", serde_json::json!({"entry": entry}));
        return;
    }

    println!("\n  Önerilen entry:\n  \x1b[36m{}\x1b[0m\n", entry);
    print!("  memory.md'ye ekle? [y/N]  ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    if line.trim().eq_ignore_ascii_case("y") {
        match append_to_memory_md(&project_path, &entry) {
            Ok(()) => println!("  \x1b[32m✓ Eklendi.\x1b[0m"),
            Err(e) => eprintln!("  \x1b[31m✗ {e}\x1b[0m"),
        }
    } else {
        println!("  Atlandı.");
    }
}

// ─── Auto-sync: raios-native heuristic memory extraction ─────────────────────

fn to_slug(text: &str, max_words: usize) -> String {
    text.split_whitespace()
        .take(max_words)
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn first_n_words(text: &str, n: usize) -> String {
    text.split_whitespace().take(n).collect::<Vec<_>>().join(" ")
}

fn heuristic_extract(transcript: &str) -> Vec<HeuristicItem> {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let date_compact = date.replace('-', "");

    let mut feedback_lines: Vec<String> = Vec::new();
    let mut decision_lines: Vec<String> = Vec::new();
    let mut user_lines: Vec<String> = Vec::new();

    for line in transcript.lines() {
        let (role, text) = if let Some(t) = line.strip_prefix("User: ") {
            ("user", t)
        } else {
            continue;
        };
        let _ = role;
        let lower = text.to_lowercase();

        // Feedback — user corrects or confirms a non-obvious approach (EN + TR)
        if ["don't ", "do not ", "stop ", "avoid ", "no, ", "wrong", "not that", "incorrect", "please don't",
            "yapma", "etme", "hayır", "yanlış", "olmaz", "değil", "bunu yapma", "böyle değil",
            "istemiyorum", "kullanma", "ekleme", "silme"]
            .iter()
            .any(|p| lower.contains(p))
        {
            feedback_lines.push(format!("- {}", first_n_words(text, 30)));
        }

        // Project decisions / architecture choices (EN + TR)
        if ["we'll use", "we're using", "we decided", "let's use", "going with", "we chose", "architecture is", "we're building",
            "kullanalım", "kullanıyoruz", "karar verdik", "yapacağız", "tercih", "mimari", "gideceğiz",
            "yapıyoruz", "seçtik", "geçiyoruz", "kullanacağız", "artık", "bundan sonra"]
            .iter()
            .any(|p| lower.contains(p))
        {
            decision_lines.push(format!("- {}", first_n_words(text, 30)));
        }

        // User background (EN + TR)
        if ["i'm a ", "i am a ", "i work ", "i've been", "my role", "my stack", "my background", "i specialize",
            "ben ", "benim ", "çalışıyorum", "uzmanlık", "stack'im", "yıldır", "geliştiriciyim", "mühendisim"]
            .iter()
            .any(|p| lower.contains(p))
        {
            user_lines.push(first_n_words(text, 40));
        }
    }

    let mut items: Vec<HeuristicItem> = Vec::new();

    if !feedback_lines.is_empty() {
        let body = feedback_lines.join("\n");
        let title = format!("Session feedback ({})", date);
        let slug = format!("feedback-{}", date_compact);
        items.push(HeuristicItem {
            item_type: "feedback",
            description: format!("{} correction/confirmation(s) detected", feedback_lines.len()),
            slug,
            title,
            body,
        });
    }

    if !decision_lines.is_empty() {
        let body = decision_lines.join("\n");
        let slug = format!("decision-{}", date_compact);
        let title = format!("Decisions ({})", date);
        items.push(HeuristicItem {
            item_type: "project",
            description: format!("{} decision(s) detected", decision_lines.len()),
            slug,
            title,
            body,
        });
    }

    if !user_lines.is_empty() {
        let body = user_lines.join("\n");
        let slug = to_slug(&user_lines[0], 4);
        let slug = if slug.is_empty() { "user-background".to_string() } else { format!("user-{}", slug) };
        let title = format!("User background ({})", date);
        items.push(HeuristicItem {
            item_type: "user",
            description: "User background/role information".to_string(),
            slug,
            title,
            body,
        });
    }

    items
}

pub fn decision_lines_from_transcript(transcript: &str) -> Vec<String> {
    heuristic_extract(transcript)
        .into_iter()
        .filter(|item| item.item_type == "project")
        .flat_map(|item| {
            item.body
                .lines()
                .map(|line| line.trim().trim_start_matches("- ").to_string())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

// ─── Per-agent transcript readers ────────────────────────────────────────────

fn started_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_codex_transcript(path: &Path, since_secs: u64) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for line in content.lines() {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if obj["ts"].as_u64().unwrap_or(0) < since_secs {
            continue;
        }
        if let Some(text) = obj["text"].as_str() {
            if !text.trim().is_empty() {
                parts.push(format!("User: {}", truncate(text, 600)));
            }
        }
    }
    parts.join("\n\n")
}

fn read_agy_transcript(path: &Path, workspace: &str, since_secs: u64) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for line in content.lines() {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if obj["timestamp"].as_u64().unwrap_or(0) / 1000 < since_secs {
            continue;
        }
        if let Some(ws) = obj["workspace"].as_str() {
            if !workspace.is_empty() && ws != workspace {
                continue;
            }
        }
        if let Some(display) = obj["display"].as_str() {
            if !display.trim().is_empty() {
                parts.push(format!("User: {}", truncate(display, 600)));
            }
        }
    }
    parts.join("\n\n")
}

fn read_opencode_transcript(path: &Path, since_secs: u64) -> String {
    // opencode prompt-history has no per-entry timestamps; use file mtime as gate.
    let Ok(meta) = std::fs::metadata(path) else {
        return String::new();
    };
    if let Ok(mtime) = meta.modified() {
        if started_secs(mtime) < since_secs {
            return String::new();
        }
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let mut parts: Vec<String> = content
        .lines()
        .rev()
        .take(60)
        .filter_map(|line| {
            let obj = serde_json::from_str::<serde_json::Value>(line).ok()?;
            let input = obj["input"].as_str()?;
            if input.trim().is_empty() { return None; }
            Some(format!("User: {}", truncate(input, 600)))
        })
        .collect();
    parts.reverse();
    parts.join("\n\n")
}

pub fn collect_transcript(agent: &str, project_path: &str, session_started: SystemTime) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let since = started_secs(session_started);
    match agent.to_lowercase().as_str() {
        "claude" => find_latest_conversation(project_path, Some(session_started))
            .map(|p| extract_transcript(&p))
            .unwrap_or_default(),
        "codex" => read_codex_transcript(
            &PathBuf::from(&home).join(".codex/history.jsonl"),
            since,
        ),
        "agy" | "antigravity" => read_agy_transcript(
            &PathBuf::from(&home).join(".gemini/antigravity-cli/history.jsonl"),
            project_path,
            since,
        ),
        "opencode" => read_opencode_transcript(
            &PathBuf::from(&home).join(".local/state/opencode/prompt-history.jsonl"),
            since,
        ),
        _ => String::new(),
    }
}

/// Agent-agnostic memory sync: heuristic extraction → raios DB → markdown export.
/// Works for claude, codex, opencode, and agy. No LLM dependency.
/// `verbose = false` during periodic background syncs (TUI is live); `true` at session end.
pub fn auto_sync_agent_memory(
    agent: &str,
    project_path: &str,
    session_started: SystemTime,
    verbose: bool,
) {
    let transcript = collect_transcript(agent, project_path, session_started);
    if transcript.is_empty() {
        return;
    }

    let items = heuristic_extract(&transcript);
    if items.is_empty() {
        return;
    }

    let project_key = claude_project_dir_name(project_path);
    let Ok(conn) = crate::db::open_db() else { return };

    for item in &items {
        let _ = crate::db::mem_upsert(
            &conn,
            crate::db::MemUpsert {
                project_key: &project_key,
                item_type: item.item_type,
                slug: &item.slug,
                title: &item.title,
                description: &item.description,
                body: &item.body,
                session_id: None,
            },
        );
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let memory_dir = PathBuf::from(&home)
        .join(".claude/projects")
        .join(&project_key)
        .join("memory");

    if let Ok(n) = crate::db::mem_export(&conn, &project_key, &memory_dir) {
        if verbose && n > 0 {
            println!(
                "  \x1b[32m✦ memory sync\x1b[0m  [{agent}]  {} item(s) → DB + {}/memory/",
                n, project_key
            );
        }
    }
}

/// Backward-compat shim used by memory-gen flow.
pub fn auto_sync_claude_memory(project_path: &str, session_started: SystemTime) {
    auto_sync_agent_memory("claude", project_path, session_started, true);
}
