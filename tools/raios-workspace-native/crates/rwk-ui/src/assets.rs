//! The bundled assets: the 28 unmodified KDE Breeze file icons (tinted at paint time as alpha
//! masks) and the Antonio font of the GT-LCARS skin. Everything is compiled in; nothing is
//! resolved from the host at runtime.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};
use rwk_core::workbench::FileIcon;

/// Directory of the icon assets inside the asset source.
pub const ICON_DIRECTORY: &str = "file-icons";
/// Path of the Antonio font inside the asset source.
pub const ANTONIO_PATH: &str = "fonts/Antonio.ttf";

macro_rules! icons {
    ($($file:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $(($file, include_bytes!(concat!("../assets/file-icons/", $file)))),*
        ];
    };
}

icons!(
    "folder.svg",
    "folder-open.svg",
    "file.svg",
    "code.svg",
    "rust.svg",
    "python.svg",
    "javascript.svg",
    "kotlin.svg",
    "java.svg",
    "go.svg",
    "c.svg",
    "cpp.svg",
    "csharp.svg",
    "markdown.svg",
    "json.svg",
    "config.svg",
    "html.svg",
    "css.svg",
    "xml.svg",
    "shell.svg",
    "image.svg",
    "audio.svg",
    "video.svg",
    "archive.svg",
    "pdf.svg",
    "font.svg",
    "database.svg",
    "table.svg",
);

const ANTONIO: &[u8] = include_bytes!("../assets/fonts/Antonio.ttf");

/// The compiled-in asset source.
#[derive(Debug, Default, Clone, Copy)]
pub struct Assets;

/// The asset path of an icon, as `svg().path(..)` takes it.
pub fn icon_path(icon: FileIcon) -> SharedString {
    SharedString::from(format!("{ICON_DIRECTORY}/{}", icon.asset()))
}

/// The Antonio font bytes, for registration with the text system.
pub fn antonio_font() -> Cow<'static, [u8]> {
    Cow::Borrowed(ANTONIO)
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == ANTONIO_PATH {
            return Ok(Some(Cow::Borrowed(ANTONIO)));
        }
        let Some(name) = path
            .strip_prefix(ICON_DIRECTORY)
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            return Ok(None);
        };
        Ok(ICONS
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        if path.trim_end_matches('/') == ICON_DIRECTORY {
            return Ok(ICONS
                .iter()
                .map(|(file, _)| SharedString::from(format!("{ICON_DIRECTORY}/{file}")))
                .collect());
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn every_icon_the_classifier_can_name_is_bundled() {
        for icon in FileIcon::ALL {
            let loaded = Assets
                .load(&icon_path(icon))
                .expect("load")
                .unwrap_or_else(|| panic!("{icon:?} is not bundled"));
            let text = std::str::from_utf8(&loaded).expect("utf-8");
            assert!(text.contains("<svg"), "{icon:?} is not an SVG");
        }
        assert_eq!(ICONS.len(), 28);
    }

    #[test]
    fn the_bundled_icons_match_the_recorded_breeze_checksums() {
        let provenance: serde_json::Value =
            serde_json::from_str(include_str!("../assets/file-icons/provenance.json"))
                .expect("provenance");
        let recorded = provenance["assets"].as_array().expect("assets");
        assert_eq!(recorded.len(), 28);
        for entry in recorded {
            let file = entry["file"].as_str().expect("file");
            let bytes = ICONS
                .iter()
                .find(|(name, _)| *name == file)
                .unwrap_or_else(|| panic!("{file} missing"))
                .1;
            let digest: String = Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(entry["sha256"].as_str(), Some(digest.as_str()), "{file}");
        }
    }

    #[test]
    fn unknown_paths_and_the_font_resolve_sensibly() {
        assert!(Assets.load("file-icons/nope.svg").expect("load").is_none());
        assert!(Assets.load("elsewhere/x").expect("load").is_none());
        assert!(Assets.load(ANTONIO_PATH).expect("load").is_some());
        assert_eq!(Assets.list("file-icons").expect("list").len(), 28);
    }
}
