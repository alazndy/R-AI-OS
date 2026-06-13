use crate::core::audit::{run_lighthouse, AuditResult};

pub(super) fn cmd_audit(url: &str, threshold: Option<u8>, json_out: bool) -> i32 {
    let result = run_lighthouse(url);

    if result.lighthouse_missing {
        eprintln!("error: lighthouse not found. Install via: npm install -g lighthouse");
        eprintln!("       Or ensure npx is available (comes with Node.js).");
        return 1;
    }

    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
    } else {
        print_audit_table(&result, threshold);
    }

    if let Some(t) = threshold {
        if below_threshold(&result, t) {
            eprintln!("\nFAIL: one or more scores below threshold of {}", t);
            return 1;
        }
    }
    0
}

fn below_threshold(r: &AuditResult, threshold: u8) -> bool {
    r.performance < threshold
        || r.accessibility < threshold
        || r.best_practices < threshold
        || r.seo < threshold
}

fn print_audit_table(r: &AuditResult, threshold: Option<u8>) {
    println!("\n  Lighthouse Audit — {}", r.url);
    println!("  {}", "─".repeat(46));
    print_score("Performance", r.performance, threshold);
    print_score("Accessibility", r.accessibility, threshold);
    print_score("Best Practices", r.best_practices, threshold);
    print_score("SEO", r.seo, threshold);
    print_score("PWA", r.pwa, None);
    println!("  {}", "─".repeat(46));
    println!("  Duration: {}ms\n", r.duration_ms);
}

fn print_score(label: &str, score: u8, threshold: Option<u8>) {
    let bar = score_bar(score);
    let flag = match threshold {
        Some(t) if score < t => " ✗",
        _ => "  ",
    };
    println!("  {:<16} {:>3}/100  {}  {}", label, score, bar, flag);
}

fn score_bar(score: u8) -> String {
    let filled = (score as usize) / 10;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(10 - filled))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audit::AuditResult;

    fn make_result(perf: u8, a11y: u8, bp: u8, seo: u8, pwa: u8) -> AuditResult {
        AuditResult {
            url: "https://example.com".to_string(),
            performance: perf,
            accessibility: a11y,
            best_practices: bp,
            seo,
            pwa,
            duration_ms: 0,
            lighthouse_missing: false,
        }
    }

    #[test]
    fn threshold_fails_when_score_below() {
        assert!(below_threshold(&make_result(75, 90, 88, 95, 0), 80));
    }

    #[test]
    fn threshold_passes_when_all_above() {
        assert!(!below_threshold(&make_result(90, 95, 92, 100, 0), 80));
    }

    #[test]
    fn threshold_ignores_pwa_score() {
        assert!(!below_threshold(&make_result(92, 95, 90, 98, 0), 85));
    }
}
