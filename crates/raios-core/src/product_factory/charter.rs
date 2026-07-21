//! Deterministic Charter composition from validated discovery evidence.

use std::collections::BTreeMap;

pub fn compose_discovery_charter(title: &str, answers: &BTreeMap<String, String>) -> String {
    let mut charter = format!("# {title} Charter\n");
    for (key, heading) in [
        ("problem_statement", "Problem"),
        ("target_user", "Target Users"),
        ("core_outcome", "First Release Outcome"),
        ("first_platform", "Initial Platform"),
        ("success_metric", "Success Metric"),
    ] {
        if let Some(value) = answers.get(key).filter(|value| !value.trim().is_empty()) {
            charter.push_str(&format!("\n## {heading}\n{value}\n"));
        }
    }
    charter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charter_composition_preserves_answer_evidence() {
        let answers = BTreeMap::from([
            ("problem_statement".into(), "Manual release work".into()),
            ("target_user".into(), "Small product teams".into()),
            ("core_outcome".into(), "Guided release readiness".into()),
            ("first_platform".into(), "React Native".into()),
            ("success_metric".into(), "Closed test enrollment".into()),
        ]);
        let charter = compose_discovery_charter("Pilot", &answers);
        assert!(charter.contains("# Pilot Charter"));
        assert!(charter.contains("Guided release readiness"));
    }
}
