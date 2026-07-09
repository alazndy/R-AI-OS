mod transcript_io;
pub use transcript_io::collect_transcript;
use transcript_io::{claude_project_dir_name, extract_transcript, find_latest_conversation};

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

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

fn first_n_words(text: &str, n: usize) -> String {
    text.split_whitespace().take(n).collect::<Vec<_>>().join(" ")
}

pub(crate) struct AtomicFact {
    pub item_type: &'static str,
    pub text: String,
    pub raw_line: String,
}

fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 14_695_981_039_346_656_037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1_099_511_628_211);
    }
    h
}

fn normalize_fact(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

fn fact_slug(item_type: &str, text: &str) -> String {
    let h = fnv1a64(&normalize_fact(text)) & 0xFFFF_FFFF_FFFF; // 48 bits is plenty
    format!("{}-{:012x}", item_type, h)
}

fn heuristic_extract_facts(transcript: &str) -> Vec<AtomicFact> {
    let mut facts: Vec<AtomicFact> = Vec::new();

    for line in transcript.lines() {
        let Some(text) = line.strip_prefix("User: ") else {
            continue;
        };
        let lower = text.to_lowercase();

        // Feedback — user corrects or confirms a non-obvious approach (EN + TR)
        if ["don't ", "do not ", "stop ", "avoid ", "no, ", "wrong", "not that", "incorrect", "please don't",
            "yapma", "etme", "hayır", "yanlış", "olmaz", "değil", "bunu yapma", "böyle değil",
            "istemiyorum", "kullanma", "ekleme", "silme"]
            .iter()
            .any(|p| lower.contains(p))
        {
            facts.push(AtomicFact {
                item_type: "feedback",
                text: first_n_words(text, 30),
                raw_line: line.to_string(),
            });
        }

        // Project decisions / architecture choices (EN + TR)
        if ["we'll use", "we're using", "we decided", "let's use", "going with", "we chose", "architecture is", "we're building",
            "kullanalım", "kullanıyoruz", "karar verdik", "yapacağız", "tercih", "mimari", "gideceğiz",
            "yapıyoruz", "seçtik", "geçiyoruz", "kullanacağız", "artık", "bundan sonra"]
            .iter()
            .any(|p| lower.contains(p))
        {
            facts.push(AtomicFact {
                item_type: "project",
                text: first_n_words(text, 30),
                raw_line: line.to_string(),
            });
        }

        // User background (EN + TR)
        if ["i'm a ", "i am a ", "i work ", "i've been", "my role", "my stack", "my background", "i specialize",
            "ben ", "benim ", "çalışıyorum", "uzmanlık", "stack'im", "yıldır", "geliştiriciyim", "mühendisim"]
            .iter()
            .any(|p| lower.contains(p))
        {
            facts.push(AtomicFact {
                item_type: "user",
                text: first_n_words(text, 40),
                raw_line: line.to_string(),
            });
        }
    }

    facts
}

/// L2: upsert the per-day scene block digest for today and link it to its facts.
/// Returns the scene slug, or None if fact_slugs is empty or DB writes failed.
fn upsert_scene_block(
    conn: &rusqlite::Connection,
    project_key: &str,
    fact_slugs: &[(String, &'static str, String)],
) -> Option<String> {
    if fact_slugs.is_empty() {
        return None;
    }
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let scene_slug = format!("scene-{}", date.replace('-', ""));

    // Cumulative merge: multiple syncs on the same day must accumulate facts into the
    // scene, not overwrite it — mem_upsert REPLACES the body on conflict, so we need to
    // read the existing body first and only append genuinely new lines.
    let mut lines: Vec<String> = raios_core::db::mem_get(conn, project_key, &scene_slug)
        .ok()
        .flatten()
        .map(|s| s.body.lines().map(String::from).collect())
        .unwrap_or_default();
    for (slug, t, text) in fact_slugs {
        let line = format!("- [{t}] {text} ([[{slug}]])");
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    let body = lines.join("\n");

    raios_core::db::mem_upsert(
        conn,
        raios_core::db::MemUpsert {
            project_key,
            item_type: "project",
            slug: &scene_slug,
            title: &format!("Scene ({})", date),
            description: &format!("{} fact(s) distilled", fact_slugs.len()),
            body: &body,
            session_id: None,
            layer: 2,
        },
    )
    .ok()?;

    let scene = raios_core::db::mem_get(conn, project_key, &scene_slug).ok()??;
    for (slug, _, _) in fact_slugs {
        if let Ok(Some(fact)) = raios_core::db::mem_get(conn, project_key, slug) {
            let _ = raios_core::db::mem_lineage_add(
                conn, "item", &scene.id, "item", &fact.id, "derived_from",
            );
        }
    }
    Some(scene_slug)
}

/// L3: rebuild the project persona from L1 user/feedback facts. Deterministic, no LLM.
pub fn rebuild_persona(conn: &rusqlite::Connection, project_key: &str) -> Option<()> {
    let items = raios_core::db::mem_list(conn, project_key).ok()?;
    let mut user: Vec<&raios_core::db::MemItemRow> = items
        .iter()
        .filter(|i| i.layer == 1 && i.item_type == "user" && i.slug != "persona")
        .collect();
    let mut feedback: Vec<&raios_core::db::MemItemRow> = items
        .iter()
        .filter(|i| i.layer == 1 && i.item_type == "feedback")
        .collect();
    if user.is_empty() && feedback.is_empty() {
        return None;
    }
    // newest first, capped
    user.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    feedback.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    user.truncate(10);
    feedback.truncate(20);

    let mut body = String::new();
    if !user.is_empty() {
        body.push_str("## Background\n");
        for i in &user {
            body.push_str(&format!("- {} ([[{}]])\n", i.body, i.slug));
        }
    }
    if !feedback.is_empty() {
        body.push_str("\n## Working Rules\n");
        for i in &feedback {
            body.push_str(&format!("- {} ([[{}]])\n", i.body, i.slug));
        }
    }

    raios_core::db::mem_upsert(
        conn,
        raios_core::db::MemUpsert {
            project_key,
            item_type: "user",
            slug: "persona",
            title: "Persona",
            description: &format!(
                "{} background + {} rule fact(s)",
                user.len(),
                feedback.len()
            ),
            body: body.trim_end(),
            session_id: None,
            layer: 3,
        },
    )
    .ok()?;

    let persona = raios_core::db::mem_get(conn, project_key, "persona").ok()??;
    for i in user.iter().chain(feedback.iter()) {
        let _ = raios_core::db::mem_lineage_add(
            conn, "item", &persona.id, "item", &i.id, "derived_from",
        );
    }
    Some(())
}

pub fn decision_lines_from_transcript(transcript: &str) -> Vec<String> {
    heuristic_extract_facts(transcript)
        .into_iter()
        .filter(|f| f.item_type == "project")
        .map(|f| format!("- {}", f.text))
        .collect()
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

    let facts = heuristic_extract_facts(&transcript);
    if facts.is_empty() {
        return;
    }

    let project_key = claude_project_dir_name(project_path);
    let Ok(conn) = raios_core::db::open_db() else { return };

    let mut written: Vec<(String, &'static str, String)> = Vec::new();
    for fact in &facts {
        // L0: immutable raw evidence
        let Ok(node_id) = raios_core::db::mem_node_add(
            &conn, &project_key, "l0_raw", agent, &fact.raw_line, None,
        ) else {
            continue;
        };

        // L1: atomic fact — hash slug makes re-detection idempotent
        let slug = fact_slug(fact.item_type, &fact.text);
        let title = first_n_words(&fact.text, 8);
        let ok = raios_core::db::mem_upsert(
            &conn,
            raios_core::db::MemUpsert {
                project_key: &project_key,
                item_type: fact.item_type,
                slug: &slug,
                title: &title,
                description: &fact.text,
                body: &fact.text,
                session_id: None,
                layer: 1,
            },
        )
        .is_ok();

        // Lineage: fact derived_from raw line
        if ok {
            if let Ok(Some(item)) = raios_core::db::mem_get(&conn, &project_key, &slug) {
                let _ = raios_core::db::mem_lineage_add(
                    &conn, "item", &item.id, "node", &node_id, "derived_from",
                );
                written.push((slug.clone(), fact.item_type, fact.text.clone()));
            }
        }
    }
    let _ = upsert_scene_block(&conn, &project_key, &written);

    if written.iter().any(|(_, t, _)| *t == "user" || *t == "feedback") {
        let _ = rebuild_persona(&conn, &project_key);
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let memory_dir = PathBuf::from(&home)
        .join(".claude/projects")
        .join(&project_key)
        .join("memory");

    if let Ok(n) = raios_core::db::mem_export(&conn, &project_key, &memory_dir) {
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

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSCRIPT: &str = "User: don't use npm here, use pnpm\n\nAssistant: Anlaşıldı.\n\nUser: we decided to use SQLite for everything\n\nUser: ben gömülü sistem geliştiriciyim";

    #[test]
    fn extract_facts_one_per_matched_line() {
        let facts = heuristic_extract_facts(TRANSCRIPT);
        let types: Vec<&str> = facts.iter().map(|f| f.item_type).collect();
        assert!(types.contains(&"feedback"));
        assert!(types.contains(&"project"));
        assert!(types.contains(&"user"));
        // raw_line preserves the untruncated source line
        assert!(facts.iter().any(|f| f.raw_line.contains("don't use npm")));
    }

    #[test]
    fn fact_slug_is_deterministic_and_normalized() {
        let a = fact_slug("feedback", "Don't use NPM here!");
        let b = fact_slug("feedback", "don't use npm  here");
        assert_eq!(a, b);
        assert!(a.starts_with("feedback-"));
        let c = fact_slug("feedback", "something else entirely");
        assert_ne!(a, c);
    }

    #[test]
    fn decision_lines_still_work() {
        let lines = decision_lines_from_transcript(TRANSCRIPT);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("- "));
        assert!(lines[0].contains("SQLite"));
    }

    #[test]
    fn scene_block_upsert_links_facts() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        // seed two L1 facts
        for (t, txt) in [("feedback", "don't use npm"), ("project", "we decided sqlite")] {
            let slug = fact_slug(t, txt);
            raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
                project_key: key, item_type: t, slug: &slug, title: txt,
                description: txt, body: txt, session_id: None, layer: 1,
            }).unwrap();
        }
        let slugs: Vec<(String, &'static str, String)> = vec![
            (fact_slug("feedback", "don't use npm"), "feedback", "don't use npm".into()),
            (fact_slug("project", "we decided sqlite"), "project", "we decided sqlite".into()),
        ];

        let scene_slug = upsert_scene_block(&conn, key, &slugs).unwrap();
        let scene = raios_core::db::mem_get(&conn, key, &scene_slug).unwrap().unwrap();
        assert_eq!(scene.layer, 2);
        assert!(scene.body.contains("don't use npm"));
        assert!(scene.body.contains(&fact_slug("project", "we decided sqlite")));

        // lineage: scene → 2 fact parents
        let parents = raios_core::db::mem_lineage_parents(&conn, "item", &scene.id).unwrap();
        assert_eq!(parents.len(), 2);
        assert!(parents.iter().all(|(k, _, r)| k == "item" && r == "derived_from"));
    }

    #[test]
    fn scene_block_accumulates_across_same_day_calls() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        // seed first L1 fact and upsert the scene with only that fact
        let (t1, txt1) = ("feedback", "don't use npm");
        let slug1 = fact_slug(t1, txt1);
        raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
            project_key: key, item_type: t1, slug: &slug1, title: txt1,
            description: txt1, body: txt1, session_id: None, layer: 1,
        }).unwrap();
        let fact1_tuple: (String, &'static str, String) = (slug1.clone(), t1, txt1.into());

        let scene_slug_1 = upsert_scene_block(&conn, key, std::slice::from_ref(&fact1_tuple)).unwrap();

        // seed a second, different L1 fact and upsert the scene again with only that fact
        let (t2, txt2) = ("project", "we decided sqlite");
        let slug2 = fact_slug(t2, txt2);
        raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
            project_key: key, item_type: t2, slug: &slug2, title: txt2,
            description: txt2, body: txt2, session_id: None, layer: 1,
        }).unwrap();
        let fact2_tuple: (String, &'static str, String) = (slug2.clone(), t2, txt2.into());

        let scene_slug_2 = upsert_scene_block(&conn, key, std::slice::from_ref(&fact2_tuple)).unwrap();

        // same day → same scene slug
        assert_eq!(scene_slug_1, scene_slug_2);

        // both facts' text must be present in the merged body (accumulation, not overwrite)
        let scene = raios_core::db::mem_get(&conn, key, &scene_slug_2).unwrap().unwrap();
        assert!(scene.body.contains(txt1));
        assert!(scene.body.contains(txt2));

        // calling a third time with the SAME first fact again must not duplicate its line
        let scene_slug_3 = upsert_scene_block(&conn, key, std::slice::from_ref(&fact1_tuple)).unwrap();
        assert_eq!(scene_slug_3, scene_slug_1);
        let scene = raios_core::db::mem_get(&conn, key, &scene_slug_3).unwrap().unwrap();
        let fact_lines: Vec<&str> = scene.body.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(fact_lines.len(), 2, "expected exactly 2 deduped fact lines, got: {:?}", fact_lines);

        // lineage: even after re-passing fact1 a second time, there must still be exactly
        // 2 "derived_from" fact parents, not 3 — lineage_add is idempotent per Task 2.
        // (mem_upsert also records a separate "revision" lineage row, with parent_kind
        // "node", whenever the scene body changes between calls — that's the Task 3
        // body-versioning mechanism working as intended, not fact lineage, so it's
        // excluded here rather than asserting the raw total.)
        let parents = raios_core::db::mem_lineage_parents(&conn, "item", &scene.id).unwrap();
        let fact_parents: Vec<_> = parents
            .iter()
            .filter(|(k, _, r)| k == "item" && r == "derived_from")
            .collect();
        assert_eq!(
            fact_parents.len(),
            2,
            "expected exactly 2 derived_from fact parents, got: {:?}",
            parents
        );
    }

    #[test]
    fn persona_assembles_from_user_and_feedback_facts() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        for (t, txt) in [
            ("user", "ben gömülü sistem geliştiriciyim"),
            ("feedback", "don't use npm, use pnpm"),
            ("project", "we decided sqlite"), // must NOT appear in persona
        ] {
            let slug = fact_slug(t, txt);
            raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
                project_key: key, item_type: t, slug: &slug, title: txt,
                description: txt, body: txt, session_id: None, layer: 1,
            }).unwrap();
        }

        rebuild_persona(&conn, key).unwrap();
        let p = raios_core::db::mem_get(&conn, key, "persona").unwrap().unwrap();
        assert_eq!(p.layer, 3);
        assert!(p.body.contains("## Background"));
        assert!(p.body.contains("gömülü sistem"));
        assert!(p.body.contains("## Working Rules"));
        assert!(p.body.contains("use pnpm"));
        assert!(!p.body.contains("sqlite"));

        let parents = raios_core::db::mem_lineage_parents(&conn, "item", &p.id).unwrap();
        assert_eq!(parents.len(), 2);
    }

    /// Regression test for the periodic-resync bloat bug: `auto_sync_agent_memory`
    /// runs on a background timer with a FIXED `session_start_time`, so
    /// `collect_transcript` re-reads the whole transcript since session start on
    /// every tick — meaning the same matched lines are offered to the fact loop
    /// over and over across a long session. The L1 fact itself is deduped via its
    /// deterministic `fact_slug` → `mem_upsert` on the same slug, but the L0
    /// evidence node (`mem_node_add(.., "l0_raw", ..)`) and its `derived_from`
    /// lineage edge were NOT deduped before the fix in `mem_node_add`, so
    /// `mem_nodes`/`mem_lineage` grew without bound even though the facts they
    /// backed did not change.
    ///
    /// `auto_sync_agent_memory` itself isn't directly testable here (it reads the
    /// transcript from disk and opens the real production DB via `open_db()`), so
    /// this test replicates its per-fact write sequence — the exact same three
    /// calls in the exact same order as the loop body in `auto_sync_agent_memory`
    /// (mem_node_add for L0 evidence, mem_upsert for the L1 fact, mem_lineage_add
    /// to link fact → evidence) — against the same in-memory DB, run twice over
    /// the identical transcript, simulating two consecutive 90-second sync ticks
    /// with no new conversation content in between.
    #[test]
    fn periodic_resync_does_not_grow_mem_nodes_for_unchanged_transcript() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        // Mirrors the fact loop body inside `auto_sync_agent_memory` exactly.
        let run_sync_tick = |conn: &rusqlite::Connection| {
            let facts = heuristic_extract_facts(TRANSCRIPT);
            assert!(!facts.is_empty(), "fixture transcript must yield at least one fact");

            let mut written: Vec<(String, &'static str, String)> = Vec::new();
            for fact in &facts {
                let node_id = raios_core::db::mem_node_add(
                    conn, key, "l0_raw", "claude", &fact.raw_line, None,
                )
                .unwrap();

                let slug = fact_slug(fact.item_type, &fact.text);
                let title = first_n_words(&fact.text, 8);
                raios_core::db::mem_upsert(
                    conn,
                    raios_core::db::MemUpsert {
                        project_key: key,
                        item_type: fact.item_type,
                        slug: &slug,
                        title: &title,
                        description: &fact.text,
                        body: &fact.text,
                        session_id: None,
                        layer: 1,
                    },
                )
                .unwrap();

                let item = raios_core::db::mem_get(conn, key, &slug).unwrap().unwrap();
                raios_core::db::mem_lineage_add(
                    conn, "item", &item.id, "node", &node_id, "derived_from",
                )
                .unwrap();
                written.push((slug, fact.item_type, fact.text.clone()));
            }
            written
        };

        // Tick 1: session just started, first sync of the (only) transcript so far.
        run_sync_tick(&conn);
        let l0_count_after_first: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_nodes WHERE kind = 'l0_raw'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(l0_count_after_first > 0);

        // Tick 2: 90 seconds later, `session_start_time` is unchanged so
        // `collect_transcript` re-reads the SAME transcript from the start again —
        // no new conversation content.
        run_sync_tick(&conn);
        let l0_count_after_second: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_nodes WHERE kind = 'l0_raw'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(
            l0_count_after_first, l0_count_after_second,
            "re-syncing the identical transcript must not grow mem_nodes l0_raw rows"
        );

        // Lineage must also stay flat: same fact, same evidence node id both times,
        // so mem_lineage's UNIQUE(child_kind, child_id, parent_kind, parent_id, relation)
        // catches the repeat and INSERT OR IGNORE is a no-op on tick 2.
        let lineage_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mem_lineage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            lineage_count, l0_count_after_first,
            "lineage edges must not duplicate across resync ticks either"
        );
    }
}
