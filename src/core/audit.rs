use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResult {
    pub url: String,
    pub performance: u8,
    pub accessibility: u8,
    pub best_practices: u8,
    pub seo: u8,
    pub pwa: u8,
    pub duration_ms: u128,
    pub lighthouse_missing: bool,
}

fn score_to_u8(v: &serde_json::Value) -> u8 {
    v.as_f64().map(|f| (f * 100.0).round() as u8).unwrap_or(0)
}

pub fn parse_lighthouse_json(url: &str, json: &str, duration_ms: u128) -> AuditResult {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    let cats = &v["categories"];
    AuditResult {
        url: url.to_string(),
        performance:    score_to_u8(&cats["performance"]["score"]),
        accessibility:  score_to_u8(&cats["accessibility"]["score"]),
        best_practices: score_to_u8(&cats["best-practices"]["score"]),
        seo:            score_to_u8(&cats["seo"]["score"]),
        pwa:            score_to_u8(&cats["pwa"]["score"]),
        duration_ms,
        lighthouse_missing: false,
    }
}

pub fn run_lighthouse(url: &str) -> AuditResult {
    let start = Instant::now();
    let out = Command::new("npx")
        .args([
            "--yes",
            "lighthouse",
            url,
            "--output", "json",
            "--output-path", "stdout",
            "--chrome-flags=--headless --no-sandbox",
            "--quiet",
        ])
        .output();

    let elapsed = start.elapsed().as_millis();

    match out {
        Err(_) => AuditResult {
            url: url.to_string(),
            performance: 0,
            accessibility: 0,
            best_practices: 0,
            seo: 0,
            pwa: 0,
            duration_ms: elapsed,
            lighthouse_missing: true,
        },
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let json_start = stdout.find('{').unwrap_or(0);
            let json_str = &stdout[json_start..];
            let mut result = parse_lighthouse_json(url, json_str, elapsed);
            result.duration_ms = elapsed;
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lighthouse_json_extracts_all_scores() {
        let json = r#"{
            "categories": {
                "performance":     { "score": 0.95 },
                "accessibility":   { "score": 0.88 },
                "best-practices":  { "score": 1.00 },
                "seo":             { "score": 0.92 },
                "pwa":             { "score": 0.30 }
            }
        }"#;
        let result = parse_lighthouse_json("https://example.com", json, 0);
        assert_eq!(result.performance,    95);
        assert_eq!(result.accessibility,  88);
        assert_eq!(result.best_practices, 100);
        assert_eq!(result.seo,            92);
        assert_eq!(result.pwa,            30);
        assert!(!result.lighthouse_missing);
    }

    #[test]
    fn parse_lighthouse_json_handles_null_score() {
        let json = r#"{"categories": {"performance": {"score": null}}}"#;
        let result = parse_lighthouse_json("https://example.com", json, 0);
        assert_eq!(result.performance, 0);
    }

    #[test]
    fn parse_lighthouse_json_handles_empty_categories() {
        let json = r#"{"categories": {}}"#;
        let result = parse_lighthouse_json("https://example.com", json, 0);
        assert_eq!(result.performance,    0);
        assert_eq!(result.accessibility,  0);
        assert_eq!(result.best_practices, 0);
        assert_eq!(result.seo,            0);
        assert_eq!(result.pwa,            0);
    }
}
