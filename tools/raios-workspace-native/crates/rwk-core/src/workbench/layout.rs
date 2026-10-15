//! The three layout presets (Ctrl+1/2/3) and how they divide the window. Pure data: the saved
//! workspace stores a preset, so the enum lives in the core and the UI re-exports it.

use serde::{Deserialize, Serialize};

/// A fixed division of the window between the file tree, the editor and the terminal dock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutPreset {
    /// Ctrl+1: all three panes in reasonable proportions.
    #[default]
    Balanced,
    /// Ctrl+2: the editor gets most of the window.
    Editor,
    /// Ctrl+3: terminals and agents get most of the window.
    Agents,
}

/// Smallest width of the file tree and of the dock in pixels.
pub const MIN_SIDE_PANE_PX: f32 = 160.0;
/// Smallest width of the editor in pixels.
pub const MIN_EDITOR_PX: f32 = 240.0;

impl LayoutPreset {
    /// Fractions of the window for (files, editor, dock); they sum to 1.
    pub fn proportions(self) -> [f32; 3] {
        match self {
            LayoutPreset::Balanced => [0.18, 0.52, 0.30],
            LayoutPreset::Editor => [0.15, 0.70, 0.15],
            LayoutPreset::Agents => [0.12, 0.28, 0.60],
        }
    }

    /// Pixel widths of (files, editor, dock) for a window `total` pixels wide. The side panes
    /// never shrink below [`MIN_SIDE_PANE_PX`] and the editor never below [`MIN_EDITOR_PX`];
    /// when the window is too narrow for the minimums the proportions are used as they are.
    pub fn widths(self, total: f32) -> [f32; 3] {
        let [files, editor, dock] = self.proportions().map(|fraction| fraction * total);
        if total < 2.0 * MIN_SIDE_PANE_PX + MIN_EDITOR_PX {
            return [files, editor, dock];
        }
        let files = files.max(MIN_SIDE_PANE_PX);
        let dock = dock.max(MIN_SIDE_PANE_PX);
        let editor = (total - files - dock).max(MIN_EDITOR_PX);
        [files, editor, total - files - editor]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRESETS: [LayoutPreset; 3] = [
        LayoutPreset::Balanced,
        LayoutPreset::Editor,
        LayoutPreset::Agents,
    ];

    #[test]
    fn layout_presets_set_the_three_pane_proportions() {
        for preset in PRESETS {
            let sum: f32 = preset.proportions().iter().sum();
            assert!((sum - 1.0).abs() < 1e-6, "{preset:?}");
        }
        let [_, editor_balanced, _] = LayoutPreset::Balanced.proportions();
        let [_, editor_focus, _] = LayoutPreset::Editor.proportions();
        let [_, _, dock_agents] = LayoutPreset::Agents.proportions();
        let [_, _, dock_balanced] = LayoutPreset::Balanced.proportions();
        assert!(editor_focus > editor_balanced);
        assert!(dock_agents > dock_balanced);
    }

    #[test]
    fn widths_add_up_and_respect_the_minimums() {
        for preset in PRESETS {
            for total in [600.0_f32, 1000.0, 1920.0] {
                let [files, editor, dock] = preset.widths(total);
                assert!(
                    (files + editor + dock - total).abs() < 0.01,
                    "{preset:?} {total}"
                );
                assert!(files >= MIN_SIDE_PANE_PX - 0.01, "{preset:?} {total}");
                assert!(dock >= MIN_SIDE_PANE_PX - 0.01, "{preset:?} {total}");
                assert!(editor >= MIN_EDITOR_PX - 0.01, "{preset:?} {total}");
            }
        }
    }

    #[test]
    fn a_window_too_narrow_for_the_minimums_still_gets_a_division() {
        let [files, editor, dock] = LayoutPreset::Balanced.widths(300.0);
        assert!((files + editor + dock - 300.0).abs() < 0.01);
    }

    #[test]
    fn presets_serialize_as_lowercase_names_and_default_to_balanced() {
        assert_eq!(
            serde_json::to_string(&LayoutPreset::Agents).unwrap(),
            "\"agents\""
        );
        assert_eq!(
            serde_json::from_str::<LayoutPreset>("\"editor\"").unwrap(),
            LayoutPreset::Editor
        );
        assert_eq!(LayoutPreset::default(), LayoutPreset::Balanced);
    }
}
