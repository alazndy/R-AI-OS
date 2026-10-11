//! The persisted appearance: `{skin, tone}`, validated the way Workspace 0.1 did. Anything that
//! does not parse falls back to the neutral appearance; a storage failure never blocks work.

use serde::{Deserialize, Serialize};

/// The visual skin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Skin {
    /// The neutral skin.
    Standard,
    /// The GT-LCARS skin.
    Lcars,
}

/// Light or dark tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// Dark surfaces.
    Dark,
    /// Light surfaces.
    Light,
}

/// Skin and tone, stored and validated independently of each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appearance {
    /// The skin.
    pub skin: Skin,
    /// The tone.
    pub tone: Tone,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            skin: Skin::Standard,
            tone: Tone::Dark,
        }
    }
}

/// Parses stored appearance text. Only a JSON object with a known `skin` and a known `tone` is
/// accepted (extra fields are ignored); missing, malformed or partly invalid data, including a
/// JSON array that serde would otherwise read as a tuple, yields the default.
pub fn parse_appearance(stored: Option<&str>) -> Appearance {
    stored
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(serde_json::Value::is_object)
        .and_then(|value| serde_json::from_value::<Appearance>(value).ok())
        .unwrap_or_default()
}

/// The text to store.
pub fn serialize_appearance(appearance: &Appearance) -> String {
    serde_json::to_string(appearance).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_malformed_or_unknown_values_fall_back_to_standard_dark() {
        let neutral = Appearance::default();
        for stored in [
            None,
            Some(""),
            Some("not json"),
            Some("null"),
            Some(r#"{"skin":"neon","tone":"dark"}"#),
            Some(r#"{"skin":"lcars","tone":"sepia"}"#),
            Some(r#"{"skin":"lcars"}"#),
            Some(r#"["lcars","light"]"#),
        ] {
            assert_eq!(parse_appearance(stored), neutral, "{stored:?}");
        }
    }

    #[test]
    fn a_valid_pair_round_trips() {
        let chosen = Appearance {
            skin: Skin::Lcars,
            tone: Tone::Light,
        };
        assert_eq!(
            parse_appearance(Some(&serialize_appearance(&chosen))),
            chosen
        );
        assert_eq!(
            serialize_appearance(&chosen),
            r#"{"skin":"lcars","tone":"light"}"#
        );
    }

    #[test]
    fn extra_fields_do_not_invalidate_a_valid_pair() {
        let parsed = parse_appearance(Some(r#"{"skin":"lcars","tone":"dark","zoom":2}"#));
        assert_eq!(parsed.skin, Skin::Lcars);
        assert_eq!(parsed.tone, Tone::Dark);
    }
}
