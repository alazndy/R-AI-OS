//! ANKA read-only transcript recall with a rebuildable sidecar cache.

use anyhow::{bail, Context, Result};
use raios_core::anka::{
    default_cache_path, AnkaConfidence, AnkaHarness, AnkaHit, AnkaIndexStatus, AnkaSearchQuery,
    AnkaSourceRef,
};
use raios_core::security::redact_secrets;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const INDEX_FILE: &str = "index.json";
const EXCLUDE_FILE: &str = "anka-exclude";
const TOMBSTONE_FILE: &str = "anka-tombstones";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnkaRecord {
    id: String,
    source: AnkaSourceRef,
    content: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AnkaIndex {
    records: Vec<AnkaRecord>,
    indexed_sources: usize,
    last_indexed_at: Option<String>,
}

pub fn parse_harness(value: &str) -> Result<AnkaHarness> {
    match value.trim().to_ascii_lowercase().as_str() {
        "claude" => Ok(AnkaHarness::Claude),
        "codex" => Ok(AnkaHarness::Codex),
        "opencode" => Ok(AnkaHarness::Opencode),
        "agy" | "antigravity" => Ok(AnkaHarness::Antigravity),
        _ => {
            bail!("unsupported ANKA harness '{value}'; use claude, codex, opencode, or antigravity")
        }
    }
}

pub fn index(harness: Option<AnkaHarness>) -> Result<AnkaIndexStatus> {
    let cache_path = default_cache_path();
    ensure_private_dir(&cache_path)?;
    let exclusions = read_lines(&config_file(EXCLUDE_FILE))?;
    let tombstones = read_lines(&config_file(TOMBSTONE_FILE))?;
    let harnesses = harness
        .map(|item| vec![item])
        .unwrap_or_else(|| AnkaHarness::ALL.to_vec());
    let mut records = Vec::new();
    let mut indexed_sources = 0;
    for harness in harnesses {
        let (found, sources) = discover_harness(&harness)?;
        records.extend(found);
        indexed_sources += sources;
    }
    records.retain(|record| {
        !tombstones.contains(&record.id)
            && !exclusions
                .iter()
                .any(|pattern| contains(&record.source.project, pattern))
    });
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records.dedup_by(|left, right| left.id == right.id);
    let index = AnkaIndex {
        records,
        indexed_sources,
        last_indexed_at: Some(now()),
    };
    write_index(&cache_path, &index)?;
    Ok(status_from_index(cache_path, &index))
}

pub fn status() -> Result<AnkaIndexStatus> {
    let cache_path = default_cache_path();
    Ok(status_from_index(
        cache_path.clone(),
        &read_index(&cache_path)?,
    ))
}

pub fn search(query: AnkaSearchQuery) -> Result<Vec<AnkaHit>> {
    let query_text = query.text.trim();
    if query_text.is_empty() {
        bail!("ANKA search query cannot be empty");
    }
    let terms = terms(query_text);
    let mut hits = read_index(&default_cache_path())?
        .records
        .iter()
        .filter(|record| {
            query
                .project
                .as_deref()
                .map(|project| contains(&record.source.project, project))
                .unwrap_or(true)
        })
        .filter(|record| {
            query
                .harness
                .as_ref()
                .map(|harness| record.source.harness == *harness)
                .unwrap_or(true)
        })
        .filter_map(|record| hit_for(record, query_text, &terms))
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| right.score.total_cmp(&left.score));
    hits.truncate(query.limit.clamp(1, 100));
    Ok(hits)
}

pub fn blame(path: &str, limit: usize) -> Result<Vec<AnkaHit>> {
    let path = path.trim();
    if path.is_empty() {
        bail!("ANKA blame path cannot be empty");
    }
    let query = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    search(AnkaSearchQuery {
        text: query.to_string(),
        project: None,
        harness: None,
        limit,
    })
}

/// Hides a record from ANKA and records a durable tombstone. Source transcripts are untouched.
pub fn forget(id: &str) -> Result<bool> {
    let id = id.trim();
    if id.is_empty() {
        bail!("ANKA record id cannot be empty");
    }
    let cache_path = default_cache_path();
    let mut index = read_index(&cache_path)?;
    if !index.records.iter().any(|record| record.id == id) {
        return Ok(false);
    }
    append_line(&config_file(TOMBSTONE_FILE), id)?;
    index.records.retain(|record| record.id != id);
    write_index(&cache_path, &index)?;
    Ok(true)
}

fn discover_harness(harness: &AnkaHarness) -> Result<(Vec<AnkaRecord>, usize)> {
    let home = dirs::home_dir().context("could not determine the user home directory")?;
    match harness {
        AnkaHarness::Claude => discover_claude(&home),
        AnkaHarness::Codex => {
            discover_jsonl(&home.join(".codex/history.jsonl"), harness, |value| {
                value["text"].as_str().map(|text| {
                    (
                        "codex-history".into(),
                        text.into(),
                        value["ts"].as_u64().map(|ts| ts.to_string()),
                    )
                })
            })
        }
        AnkaHarness::Opencode => discover_jsonl(
            &home.join(".local/state/opencode/prompt-history.jsonl"),
            harness,
            |value| {
                value["input"].as_str().map(|text| {
                    (
                        value["project"]
                            .as_str()
                            .unwrap_or("opencode-history")
                            .into(),
                        text.into(),
                        value["timestamp"].as_u64().map(|ts| ts.to_string()),
                    )
                })
            },
        ),
        AnkaHarness::Antigravity => discover_jsonl(
            &home.join(".gemini/antigravity-cli/history.jsonl"),
            harness,
            |value| {
                value["display"].as_str().map(|text| {
                    (
                        value["workspace"]
                            .as_str()
                            .unwrap_or("antigravity-history")
                            .into(),
                        text.into(),
                        value["timestamp"].as_u64().map(|ts| ts.to_string()),
                    )
                })
            },
        ),
    }
}

fn discover_claude(home: &Path) -> Result<(Vec<AnkaRecord>, usize)> {
    let root = home.join(".claude/projects");
    if !root.exists() {
        return Ok((Vec::new(), 0));
    }
    let mut records = Vec::new();
    let mut sources = 0;
    for entry in WalkDir::new(root).max_depth(3).into_iter().flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let content = crate::session_memory::extract_transcript(path);
        if content.trim().is_empty() {
            continue;
        }
        sources += 1;
        let project = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("claude-history")
            .to_string();
        records.push(record(
            AnkaHarness::Claude,
            project,
            session_id(path),
            modified_at(path),
            content,
        ));
    }
    Ok((records, sources))
}

fn discover_jsonl<F>(
    path: &Path,
    harness: &AnkaHarness,
    mut extract: F,
) -> Result<(Vec<AnkaRecord>, usize)>
where
    F: FnMut(&serde_json::Value) -> Option<(String, String, Option<String>)>,
{
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()))
        }
    };
    let mut records = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some((project, text, occurred_at)) = extract(&value) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        records.push(record(
            harness.clone(),
            project,
            format!("{}:{}", session_id(path), line_number + 1),
            occurred_at.unwrap_or_else(|| modified_at(path)),
            text,
        ));
    }
    Ok((records, usize::from(path.exists())))
}

fn record(
    harness: AnkaHarness,
    project: String,
    session_id: String,
    occurred_at: String,
    content: String,
) -> AnkaRecord {
    let content = redact_secrets(&content);
    let id = hash(&[
        harness.as_str(),
        &project,
        &session_id,
        &occurred_at,
        &content,
    ]);
    AnkaRecord {
        id,
        source: AnkaSourceRef {
            harness,
            project,
            session_id,
            occurred_at,
        },
        content,
    }
}

fn hit_for(record: &AnkaRecord, full_query: &str, terms: &[String]) -> Option<AnkaHit> {
    let haystack = record.content.to_ascii_lowercase();
    let exact = haystack.contains(&full_query.to_ascii_lowercase());
    let matched = terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count();
    if !exact && matched == 0 {
        return None;
    }
    let offset = terms
        .iter()
        .find_map(|term| haystack.find(term))
        .unwrap_or(0);
    let start = offset.saturating_sub(180);
    let end = (offset + 820).min(record.content.len());
    let mut snippet = record
        .content
        .get(start..end)
        .unwrap_or(&record.content)
        .to_string();
    if start > 0 {
        snippet.insert(0, '…');
    }
    if end < record.content.len() {
        snippet.push('…');
    }
    Some(AnkaHit {
        id: record.id.clone(),
        source: record.source.clone(),
        snippet,
        score: if exact {
            100.0 + matched as f64
        } else {
            matched as f64
        },
        confidence: if exact {
            AnkaConfidence::Exact
        } else {
            AnkaConfidence::Lexical
        },
    })
}

fn terms(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|term| {
            term.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
                .to_ascii_lowercase()
        })
        .filter(|term| term.len() >= 2)
        .collect()
}

fn status_from_index(cache_path: PathBuf, index: &AnkaIndex) -> AnkaIndexStatus {
    AnkaIndexStatus {
        cache_path,
        indexed_sources: index.indexed_sources,
        indexed_records: index.records.len(),
        last_indexed_at: index.last_indexed_at.clone(),
    }
}

fn read_index(cache_path: &Path) -> Result<AnkaIndex> {
    match fs::read_to_string(cache_path.join(INDEX_FILE)) {
        Ok(content) => serde_json::from_str(&content)
            .context("ANKA index is corrupt; run `raios anka index` to rebuild it"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AnkaIndex::default()),
        Err(error) => Err(error.into()),
    }
}

fn write_index(cache_path: &Path, index: &AnkaIndex) -> Result<()> {
    ensure_private_dir(cache_path)?;
    atomic_write(&cache_path.join(INDEX_FILE), &serde_json::to_vec(index)?)
}

fn config_file(name: &str) -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("raios")
        .join(name)
}

fn read_lines(path: &Path) -> Result<HashSet<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
            .collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(error) => Err(error.into()),
    }
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut content = fs::read_to_string(path).unwrap_or_default();
    if !content.lines().any(|existing| existing == line) {
        content.push_str(line);
        content.push('\n');
        atomic_write(path, content.as_bytes())?;
    }
    Ok(())
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, content)?;
    set_owner_only(&temporary)?;
    fs::rename(temporary, path)?;
    set_owner_only(path)
}

fn ensure_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_owner_only(path: &Path) -> Result<()> {
    raios_core::security::harden_file_perms(path)?;
    Ok(())
}

fn contains(value: &str, needle: &str) -> bool {
    value
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}
fn session_id(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown-session")
        .to_string()
}
fn modified_at(path: &Path) -> String {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|time| time.as_secs().to_string())
        .unwrap_or_else(now)
}
fn hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}
fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_content_is_redacted_before_becoming_a_record() {
        let record = record(
            AnkaHarness::Codex,
            "p".into(),
            "s".into(),
            "1".into(),
            "token=abcdefghijklmno".into(),
        );
        assert!(!record.content.contains("abcdefghijklmno"));
        assert!(record.content.contains("REDACTED"));
    }

    #[test]
    fn exact_match_has_stronger_confidence() {
        let record = record(
            AnkaHarness::Codex,
            "p".into(),
            "s".into(),
            "1".into(),
            "jwt refresh rotation fixed".into(),
        );
        let hit = hit_for(&record, "jwt refresh", &terms("jwt refresh")).unwrap();
        assert_eq!(hit.confidence, AnkaConfidence::Exact);
    }
}
