use crate::evolution::CandidateStore;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub async fn handle_list_instinct_candidates<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    writer: &mut W,
) {
    let limit = v["limit"].as_u64().unwrap_or(20) as usize;
    let store = CandidateStore::new(CandidateStore::default_path());
    let candidates = store.list_pending(limit);
    let r = serde_json::json!({ "event": "InstinctCandidatesList", "candidates": candidates });
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}

pub async fn handle_promote_instinct<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    writer: &mut W,
) {
    if let Some(rule) = v["rule"].as_str() {
        let store = CandidateStore::new(CandidateStore::default_path());
        store.promote(rule);
        let mut engine = crate::instinct::InstinctEngine::init();
        engine.add_rule(rule.to_string());
        let _ = engine.save();
        let r = serde_json::json!({ "event": "InstinctPromoted", "rule": rule });
        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
    } else {
        let err = serde_json::json!({ "event": "PromoteError", "error": "rule is required" });
        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
    }
}

pub async fn handle_list_evolution_candidates<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    evolution_store: &Arc<CandidateStore>,
    writer: &mut W,
) {
    let limit = v["limit"].as_u64().unwrap_or(20) as usize;
    let candidates = evolution_store.list_pending(limit);
    let r = serde_json::json!({ "event": "EvolutionCandidates", "candidates": candidates });
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}

pub async fn handle_promote_evolution_candidate<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    evolution_store: &Arc<CandidateStore>,
    writer: &mut W,
) {
    if let Some(rule) = v["rule"].as_str() {
        evolution_store.promote(rule);
        let r = serde_json::json!({ "event": "EvolutionCandidatePromoted", "rule": rule });
        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
    }
}

pub async fn handle_prune_expired_candidates<W: AsyncWriteExt + Unpin>(
    evolution_store: &Arc<CandidateStore>,
    writer: &mut W,
) {
    let removed = evolution_store.sweep_expired();
    let r = serde_json::json!({ "event": "EvolutionPruned", "removed": removed });
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}
