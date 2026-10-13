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
