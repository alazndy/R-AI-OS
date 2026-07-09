pub(super) fn first_n_words(text: &str, n: usize) -> String {
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

pub(super) fn fact_slug(item_type: &str, text: &str) -> String {
    let h = fnv1a64(&normalize_fact(text)) & 0xFFFF_FFFF_FFFF; // 48 bits is plenty
    format!("{}-{:012x}", item_type, h)
}

pub(super) fn heuristic_extract_facts(transcript: &str) -> Vec<AtomicFact> {
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

pub fn decision_lines_from_transcript(transcript: &str) -> Vec<String> {
    heuristic_extract_facts(transcript)
        .into_iter()
        .filter(|f| f.item_type == "project")
        .map(|f| format!("- {}", f.text))
        .collect()
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
}
