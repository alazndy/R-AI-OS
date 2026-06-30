use crate::proxy_store::CapabilityProxy;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub async fn handle_execute_capability<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    proxy: &Arc<CapabilityProxy>,
    writer: &mut W,
) {
    let capability = v["capability"].as_str().unwrap_or("").to_string();
    let input = v["input"].as_str().unwrap_or("").to_string();
    if capability.is_empty() {
        let err =
            serde_json::json!({ "event": "CapabilityError", "error": "capability name is required" });
        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
        return;
    }
    let response = match proxy.execute(&capability, &input) {
        Ok(result) => serde_json::json!({
            "event": "CapabilityResult",
            "capability": capability,
            "result": result
        }),
        Err(e) => serde_json::json!({
            "event": "CapabilityError",
            "capability": capability,
            "error": e.to_string()
        }),
    };
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
}

pub async fn handle_route_capability<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    proxy: &Arc<CapabilityProxy>,
    writer: &mut W,
) {
    let query = v["query"].as_str().unwrap_or("").to_string();
    let input = v["input"].as_str().unwrap_or("").to_string();
    if query.is_empty() {
        let err = serde_json::json!({ "event": "RouteError", "error": "query is required" });
        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
        return;
    }
    let response = match proxy.route(&query, &input) {
        Ok(result) => serde_json::json!({
            "event": "RouteResult",
            "query": query,
            "result": result
        }),
        Err(e) => serde_json::json!({
            "event": "RouteError",
            "query": query,
            "error": e.to_string()
        }),
    };
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
}

pub async fn handle_list_capabilities<W: AsyncWriteExt + Unpin>(
    proxy: &Arc<CapabilityProxy>,
    writer: &mut W,
) {
    let caps: Vec<serde_json::Value> = proxy
        .store()
        .list()
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "description": c.description,
                "platforms": c.platforms
            })
        })
        .collect();
    let r = serde_json::json!({ "event": "CapabilityList", "capabilities": caps });
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}
