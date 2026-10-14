//! Client-side checks of what the user types into the handoff composer and the agent launcher.
//!
//! The daemon scans for secrets again; these checks exist so that an obviously bad request never
//! leaves the workspace and the user sees why. The secret detector is injected because this crate
//! must not depend on `raios-core`.

use crate::control::CreateHandoffRequest;

/// Largest handoff summary the daemon accepts, in bytes.
pub const MAX_SUMMARY_BYTES: usize = 16_000;
/// Longest first prompt of an agent launch, in characters.
pub const MAX_PROMPT_CHARS: usize = 600;

/// Why an input was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputProblem {
    /// Nothing but whitespace.
    Empty,
    /// Longer than the daemon accepts.
    TooLong {
        /// Size of the input in the unit of the limit.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// Starts with `-`, which a command line could take for an option.
    LooksLikeOption,
    /// Contains a control character other than newline and tab.
    ControlCharacter,
    /// The injected detector found something that looks like a credential.
    LooksLikeSecret,
}

impl InputProblem {
    /// The message shown next to the input.
    pub fn message(&self) -> String {
        match self {
            Self::Empty => "Metin boş olamaz.".to_owned(),
            Self::TooLong { size, limit } => {
                format!("Metin çok uzun: {size} (üst sınır {limit}).")
            }
            Self::LooksLikeOption => "Metin '-' ile başlayamaz.".to_owned(),
            Self::ControlCharacter => "Metin denetim karakteri içeremez.".to_owned(),
            Self::LooksLikeSecret => {
                "Metin bir parola veya anahtar gibi görünüyor; gönderilmedi.".to_owned()
            }
        }
    }
}

fn check_text(text: &str, secret: &dyn Fn(&str) -> bool) -> Result<(), InputProblem> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(InputProblem::Empty);
    }
    if trimmed.starts_with('-') {
        return Err(InputProblem::LooksLikeOption);
    }
    if text
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(InputProblem::ControlCharacter);
    }
    if secret(text) {
        return Err(InputProblem::LooksLikeSecret);
    }
    Ok(())
}

/// A conservative check for text that looks like a credential. It is a guard against accidents
/// (a pasted key), not a security boundary: the daemon scans again, and a clean result proves
/// nothing.
pub fn looks_like_secret(text: &str) -> bool {
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
        return true;
    }
    let tokens: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | '(' | ')'))
        .filter(|token| !token.is_empty())
        .collect();
    tokens.iter().any(|token| token_looks_like_secret(token))
        // "API_KEY: value" arrives as two tokens.
        || tokens
            .windows(2)
            .any(|pair| names_a_secret(pair[0].trim_end_matches([':', '='])) && pair[0].ends_with([':', '=']) && pair[1].len() >= 8)
}

fn names_a_secret(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["password", "passwd", "secret", "token", "apikey", "api_key"]
        .iter()
        .any(|word| name.contains(word))
}

/// The text with every whitespace-separated word that looks like a credential replaced by
/// `[gizli]`; whitespace and everything else is kept. Used before stored memory is shown.
pub fn redact_secrets(text: &str) -> String {
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
        return "[gizli]".to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            out.push_str(if token_looks_like_secret(word) {
                "[gizli]"
            } else {
                word
            });
            word.clear();
        }
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn token_looks_like_secret(token: &str) -> bool {
    const PREFIXES: [(&str, usize); 7] = [
        ("sk-", 20),
        ("ghp_", 20),
        ("gho_", 20),
        ("github_pat_", 20),
        ("xoxb-", 20),
        ("xoxp-", 20),
        ("AIza", 30),
    ];
    if PREFIXES
        .iter()
        .any(|(prefix, length)| token.starts_with(prefix) && token.len() >= *length)
    {
        return true;
    }
    let aws = token.len() == 20
        && token.starts_with("AKIA")
        && token
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if aws {
        return true;
    }
    if let Some((name, value)) = token.split_once(['=', ':']) {
        if names_a_secret(name) && value.len() >= 8 {
            return true;
        }
    }
    let long_blob = token.len() >= 40
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_' | '=' | '-'))
        && token.chars().any(|c| c.is_ascii_digit())
        && token.chars().any(|c| c.is_ascii_lowercase())
        && token.chars().any(|c| c.is_ascii_uppercase());
    long_blob
}

/// Validates a handoff summary (at most [`MAX_SUMMARY_BYTES`] bytes).
pub fn validate_summary(text: &str, secret: &dyn Fn(&str) -> bool) -> Result<(), InputProblem> {
    if text.len() > MAX_SUMMARY_BYTES {
        return Err(InputProblem::TooLong {
            size: text.len(),
            limit: MAX_SUMMARY_BYTES,
        });
    }
    check_text(text, secret)
}

/// Validates the first prompt of an agent launch (at most [`MAX_PROMPT_CHARS`] characters).
pub fn validate_prompt(text: &str, secret: &dyn Fn(&str) -> bool) -> Result<(), InputProblem> {
    let size = text.chars().count();
    if size > MAX_PROMPT_CHARS {
        return Err(InputProblem::TooLong {
            size,
            limit: MAX_PROMPT_CHARS,
        });
    }
    check_text(text, secret)
}

/// The sentence the consent dialog shows for a handoff, derived from the request itself.
pub fn handoff_operation_text(request: &CreateHandoffRequest) -> String {
    format!(
        "{} → {} devir notu kuyruğa alınacak ({}). Not {} bayt.",
        request.from.identity().unwrap_or("shell"),
        request.to.identity().unwrap_or("shell"),
        request.project_root.display(),
        request.summary.len()
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::control::IdempotencyKey;
    use crate::sessions::SessionKind;

    fn never(_: &str) -> bool {
        false
    }

    fn token(text: &str) -> bool {
        text.contains("sk-")
    }

    #[test]
    fn ordinary_text_passes() {
        assert_eq!(
            validate_summary("Devam et: testleri çalıştır.\nSonra raporla.", &never),
            Ok(())
        );
        assert_eq!(validate_prompt("fix the build", &never), Ok(()));
    }

    #[test]
    fn empty_or_blank_text_is_refused() {
        assert_eq!(validate_summary("", &never), Err(InputProblem::Empty));
        assert_eq!(validate_summary(" \n\t ", &never), Err(InputProblem::Empty));
    }

    #[test]
    fn text_that_could_be_an_option_is_refused() {
        assert_eq!(
            validate_summary("--to evil", &never),
            Err(InputProblem::LooksLikeOption)
        );
        assert_eq!(
            validate_prompt("  -rf", &never),
            Err(InputProblem::LooksLikeOption)
        );
        assert_eq!(validate_summary("a - b", &never), Ok(()));
    }

    #[test]
    fn control_characters_are_refused_but_newlines_and_tabs_are_not() {
        assert_eq!(
            validate_summary("a\u{0}b", &never),
            Err(InputProblem::ControlCharacter)
        );
        assert_eq!(
            validate_summary("a\u{1b}[31mb", &never),
            Err(InputProblem::ControlCharacter)
        );
        assert_eq!(validate_summary("a\n\tb", &never), Ok(()));
    }

    #[test]
    fn the_limits_count_bytes_for_summaries_and_characters_for_prompts() {
        let ok = "a".repeat(MAX_SUMMARY_BYTES);
        assert_eq!(validate_summary(&ok, &never), Ok(()));
        let too_long = "a".repeat(MAX_SUMMARY_BYTES + 1);
        assert!(matches!(
            validate_summary(&too_long, &never),
            Err(InputProblem::TooLong { .. })
        ));
        // 600 two-byte characters are 1,200 bytes but exactly at the prompt limit.
        let prompt = "ş".repeat(MAX_PROMPT_CHARS);
        assert_eq!(validate_prompt(&prompt, &never), Ok(()));
        assert!(matches!(
            validate_prompt(&"ş".repeat(MAX_PROMPT_CHARS + 1), &never),
            Err(InputProblem::TooLong {
                size: 601,
                limit: 600
            })
        ));
    }

    #[test]
    fn the_injected_detector_blocks_secrets() {
        assert_eq!(
            validate_summary("key sk-abc", &token),
            Err(InputProblem::LooksLikeSecret)
        );
        assert_eq!(
            validate_prompt("key sk-abc", &token),
            Err(InputProblem::LooksLikeSecret)
        );
    }

    #[test]
    fn the_operation_text_names_both_agents_the_project_and_the_size() {
        let request = CreateHandoffRequest {
            project_root: PathBuf::from("/work/app"),
            from: SessionKind::Claude,
            to: SessionKind::Codex,
            summary: "devam".to_owned(),
            idempotency_key: IdempotencyKey::new("k"),
        };

        let text = handoff_operation_text(&request);

        assert!(
            text.contains("claude_kaira") && text.contains("codex_kaira"),
            "{text}"
        );
        assert!(
            text.contains("/work/app") && text.contains("5 bayt"),
            "{text}"
        );
    }

    #[test]
    fn the_local_detector_flags_common_credential_shapes() {
        for secret in [
            "sk-abcdefghijklmnopqrstuvwx",
            "token ghp_abcdefghijklmnopqrstuvwxyz0123",
            "AKIAABCDEFGHIJKLMNOP",
            "password=hunter2hunter2",
            "API_KEY: abcdef123456",
            "-----BEGIN RSA PRIVATE KEY-----\nMIIE",
            "Zm9vYmFyQmF6MTIzNDU2Nzg5MEFiQ2RFZkdoSWpLbE1uT3A=",
        ] {
            assert!(looks_like_secret(secret), "{secret}");
        }
    }

    #[test]
    fn redaction_replaces_only_the_secret_words() {
        assert_eq!(
            redact_secrets("use sk-abcdefghijklmnopqrstuvwx for\nthe call"),
            "use [gizli] for\nthe call"
        );
        assert_eq!(redact_secrets("nothing here"), "nothing here");
        assert_eq!(
            redact_secrets("-----BEGIN PRIVATE KEY-----\nabc"),
            "[gizli]"
        );
    }

    #[test]
    fn ordinary_prose_and_short_identifiers_are_not_flagged() {
        for text in [
            "Devam et: testleri çalıştır ve raporla.",
            "the token budget is small; see section 3",
            "commit 4f2a9c1 fixes the parser",
            "path /home/alaz/dev/core/R-AI-OS/crates/rwk-core/src/handoff_input.rs",
            "password reset flow needs review",
        ] {
            assert!(!looks_like_secret(text), "{text}");
        }
    }

    #[test]
    fn every_problem_has_a_message() {
        for problem in [
            InputProblem::Empty,
            InputProblem::TooLong { size: 2, limit: 1 },
            InputProblem::LooksLikeOption,
            InputProblem::ControlCharacter,
            InputProblem::LooksLikeSecret,
        ] {
            assert!(!problem.message().is_empty());
        }
    }
}
