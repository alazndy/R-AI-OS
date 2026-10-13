//! The monospace font of the editor and the terminal.
//!
//! GPUI does not resolve the generic `monospace` family through fontconfig, so it would fall back
//! to the proportional UI font. The first installed family of [`MONOSPACE_CANDIDATES`] is used.

use std::sync::OnceLock;

use gpui::Window;

/// Families tried in order; the first one the system has wins.
pub const MONOSPACE_CANDIDATES: [&str; 12] = [
    "JetBrains Mono",
    "Fira Code",
    "Hack",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Adwaita Mono",
    "Menlo",
    "Consolas",
    "Courier New",
    "Cascadia Mono",
    "monospace",
];

/// The first candidate present in `available`, or `monospace` when none is.
pub fn pick_monospace(available: &[String]) -> &'static str {
    MONOSPACE_CANDIDATES
        .iter()
        .copied()
        .find(|candidate| available.iter().any(|name| name == candidate))
        .unwrap_or("monospace")
}

/// The monospace family to draw code with (looked up once per process).
pub fn monospace_family(window: &Window) -> &'static str {
    static FAMILY: OnceLock<&'static str> = OnceLock::new();
    FAMILY.get_or_init(|| pick_monospace(&window.text_system().all_font_names()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_earliest_candidate_that_is_installed_wins() {
        let available = names(&["Noto Sans", "DejaVu Sans Mono", "Hack"]);
        assert_eq!(pick_monospace(&available), "Hack");
    }

    #[test]
    fn without_any_candidate_the_generic_family_is_used() {
        assert_eq!(pick_monospace(&names(&["Noto Sans"])), "monospace");
    }
}
