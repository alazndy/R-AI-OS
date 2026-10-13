//! File presentation: which icon and tone a file or folder gets. Ported one to one from
//! Workspace 0.1: exact file-name rules first, then case-insensitive extensions.

use super::appearance::Tone;

/// The 28 bundled Breeze glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileIcon {
    /// Closed folder.
    Folder,
    /// Open folder.
    FolderOpen,
    /// Any other file.
    File,
    /// Generic source code.
    Code,
    /// Rust.
    Rust,
    /// Python.
    Python,
    /// JavaScript.
    Javascript,
    /// Kotlin.
    Kotlin,
    /// Java.
    Java,
    /// Go.
    Go,
    /// C.
    C,
    /// C++.
    Cpp,
    /// C#.
    Csharp,
    /// Markdown and prose documents.
    Markdown,
    /// JSON.
    Json,
    /// Configuration.
    Config,
    /// HTML.
    Html,
    /// Style sheets.
    Css,
    /// XML.
    Xml,
    /// Shell scripts.
    Shell,
    /// Images.
    Image,
    /// Audio.
    Audio,
    /// Video.
    Video,
    /// Archives and packages.
    Archive,
    /// PDF.
    Pdf,
    /// Fonts.
    Font,
    /// Databases.
    Database,
    /// Tables.
    Table,
}

impl FileIcon {
    /// Every glyph, for exhaustive checks of the bundled assets.
    pub const ALL: [FileIcon; 28] = [
        FileIcon::Folder,
        FileIcon::FolderOpen,
        FileIcon::File,
        FileIcon::Code,
        FileIcon::Rust,
        FileIcon::Python,
        FileIcon::Javascript,
        FileIcon::Kotlin,
        FileIcon::Java,
        FileIcon::Go,
        FileIcon::C,
        FileIcon::Cpp,
        FileIcon::Csharp,
        FileIcon::Markdown,
        FileIcon::Json,
        FileIcon::Config,
        FileIcon::Html,
        FileIcon::Css,
        FileIcon::Xml,
        FileIcon::Shell,
        FileIcon::Image,
        FileIcon::Audio,
        FileIcon::Video,
        FileIcon::Archive,
        FileIcon::Pdf,
        FileIcon::Font,
        FileIcon::Database,
        FileIcon::Table,
    ];

    /// File name of the SVG asset (without directory).
    pub fn asset(self) -> &'static str {
        match self {
            Self::Folder => "folder.svg",
            Self::FolderOpen => "folder-open.svg",
            Self::File => "file.svg",
            Self::Code => "code.svg",
            Self::Rust => "rust.svg",
            Self::Python => "python.svg",
            Self::Javascript => "javascript.svg",
            Self::Kotlin => "kotlin.svg",
            Self::Java => "java.svg",
            Self::Go => "go.svg",
            Self::C => "c.svg",
            Self::Cpp => "cpp.svg",
            Self::Csharp => "csharp.svg",
            Self::Markdown => "markdown.svg",
            Self::Json => "json.svg",
            Self::Config => "config.svg",
            Self::Html => "html.svg",
            Self::Css => "css.svg",
            Self::Xml => "xml.svg",
            Self::Shell => "shell.svg",
            Self::Image => "image.svg",
            Self::Audio => "audio.svg",
            Self::Video => "video.svg",
            Self::Archive => "archive.svg",
            Self::Pdf => "pdf.svg",
            Self::Font => "font.svg",
            Self::Database => "database.svg",
            Self::Table => "table.svg",
        }
    }
}

/// The colour family of an icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileTone {
    /// The muted default.
    Neutral,
    /// Blue.
    Blue,
    /// Yellow.
    Yellow,
    /// Orange.
    Orange,
    /// Green.
    Green,
    /// Purple.
    Purple,
    /// Cyan.
    Cyan,
    /// Pink.
    Pink,
}

impl FileTone {
    /// The 0xRRGGBB colour of this tone on a dark or light surface. The light values come from
    /// 0.1's `body.light` and keep the contrast gates (icons at least 4.94:1).
    pub fn color(self, tone: Tone) -> u32 {
        match (self, tone) {
            (Self::Neutral, Tone::Dark) => 0x9a9a9a,
            (Self::Neutral, Tone::Light) => 0x595959,
            (Self::Blue, Tone::Dark) => 0x84baff,
            (Self::Blue, Tone::Light) => 0x28619f,
            (Self::Yellow, Tone::Dark) => 0xe2c66b,
            (Self::Yellow, Tone::Light) => 0x80621c,
            (Self::Orange, Tone::Dark) => 0xe0a17e,
            (Self::Orange, Tone::Light) => 0xa54d1b,
            (Self::Green, Tone::Dark) => 0x95c5a4,
            (Self::Green, Tone::Light) => 0x286342,
            (Self::Purple, Tone::Dark) => 0xbfa2ef,
            (Self::Purple, Tone::Light) => 0x7040a1,
            (Self::Cyan, Tone::Dark) => 0x78cbd8,
            (Self::Cyan, Tone::Light) => 0x196771,
            (Self::Pink, Tone::Dark) => 0xe7a4cd,
            (Self::Pink, Tone::Light) => 0xa03975,
        }
    }
}

/// What a row shows for one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePresentation {
    /// The glyph.
    pub icon: FileIcon,
    /// Its colour family.
    pub tone: FileTone,
    /// Accessible description of the type.
    pub label: &'static str,
}

const fn pres(icon: FileIcon, tone: FileTone, label: &'static str) -> FilePresentation {
    FilePresentation { icon, tone, label }
}

const FAMILIES: &[(&[&str], FilePresentation)] = &[
    (
        &["rs"],
        pres(FileIcon::Rust, FileTone::Orange, "Rust kaynak kodu"),
    ),
    (
        &["py", "pyw", "pyi", "ipynb"],
        pres(FileIcon::Python, FileTone::Yellow, "Python"),
    ),
    (
        &["js", "mjs", "cjs"],
        pres(FileIcon::Javascript, FileTone::Yellow, "JavaScript"),
    ),
    (
        &["ts", "mts", "cts"],
        pres(FileIcon::Code, FileTone::Blue, "TypeScript"),
    ),
    (
        &["jsx", "tsx", "vue", "svelte", "astro"],
        pres(FileIcon::Code, FileTone::Cyan, "Arayüz bileşeni"),
    ),
    (
        &["kt", "kts"],
        pres(FileIcon::Kotlin, FileTone::Purple, "Kotlin"),
    ),
    (&["java"], pres(FileIcon::Java, FileTone::Orange, "Java")),
    (&["go"], pres(FileIcon::Go, FileTone::Cyan, "Go")),
    (
        &["c", "h"],
        pres(FileIcon::C, FileTone::Blue, "C kaynak kodu"),
    ),
    (
        &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
        pres(FileIcon::Cpp, FileTone::Blue, "C++ kaynak kodu"),
    ),
    (
        &["cs"],
        pres(FileIcon::Csharp, FileTone::Purple, "C# kaynak kodu"),
    ),
    (&["dart"], pres(FileIcon::Code, FileTone::Cyan, "Dart")),
    (&["swift"], pres(FileIcon::Code, FileTone::Orange, "Swift")),
    (
        &[
            "rb", "erb", "php", "lua", "pl", "r", "ex", "exs", "erl", "hs", "zig", "asm",
        ],
        pres(FileIcon::Code, FileTone::Pink, "Kaynak kodu"),
    ),
    (
        &["md", "markdown", "mdx", "rst", "adoc"],
        pres(FileIcon::Markdown, FileTone::Blue, "Belge"),
    ),
    (
        &["json", "jsonc", "json5"],
        pres(FileIcon::Json, FileTone::Yellow, "JSON verisi"),
    ),
    (
        &[
            "toml",
            "yaml",
            "yml",
            "ini",
            "cfg",
            "conf",
            "properties",
            "desktop",
            "editorconfig",
        ],
        pres(FileIcon::Config, FileTone::Green, "Yapılandırma"),
    ),
    (
        &["html", "htm", "xhtml"],
        pres(FileIcon::Html, FileTone::Orange, "HTML"),
    ),
    (
        &["css", "scss", "sass", "less"],
        pres(FileIcon::Css, FileTone::Purple, "Stil dosyası"),
    ),
    (
        &["xml", "xsd", "xsl", "xslt", "plist", "csproj", "fsproj"],
        pres(FileIcon::Xml, FileTone::Green, "XML"),
    ),
    (
        &["sh", "bash", "zsh", "fish", "ps1", "bat", "cmd"],
        pres(FileIcon::Shell, FileTone::Green, "Kabuk betiği"),
    ),
    (
        &[
            "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "svgz", "ico", "bmp", "tiff",
            "tif", "heic", "psd", "xcf",
        ],
        pres(FileIcon::Image, FileTone::Pink, "Görsel"),
    ),
    (
        &[
            "mp3", "wav", "flac", "ogg", "opus", "m4a", "aac", "aiff", "mid", "midi",
        ],
        pres(FileIcon::Audio, FileTone::Pink, "Ses"),
    ),
    (
        &["mp4", "webm", "mkv", "mov", "avi", "mpeg", "mpg", "m4v"],
        pres(FileIcon::Video, FileTone::Purple, "Video"),
    ),
    (
        &[
            "zip", "tar", "gz", "bz2", "xz", "zst", "tgz", "7z", "rar", "jar", "war", "deb", "rpm",
            "apk", "dmg", "iso",
        ],
        pres(FileIcon::Archive, FileTone::Orange, "Arşiv veya paket"),
    ),
    (
        &["pdf"],
        pres(FileIcon::Pdf, FileTone::Orange, "PDF belgesi"),
    ),
    (
        &["ttf", "otf", "woff", "woff2"],
        pres(FileIcon::Font, FileTone::Purple, "Yazı tipi"),
    ),
    (
        &["sql", "db", "sqlite", "sqlite3", "db3"],
        pres(FileIcon::Database, FileTone::Cyan, "Veritabanı"),
    ),
    (
        &["csv", "tsv", "xls", "xlsx", "ods"],
        pres(FileIcon::Table, FileTone::Green, "Tablo"),
    ),
];

/// The icon, tone and label of an entry. `open` picks the open folder glyph for a directory.
pub fn presentation(name: &str, is_directory: bool, open: bool) -> FilePresentation {
    if is_directory {
        let icon = if open {
            FileIcon::FolderOpen
        } else {
            FileIcon::Folder
        };
        return pres(icon, FileTone::Blue, "Klasör");
    }
    let name = name.to_lowercase();
    if name == ".env" || name.starts_with(".env.") {
        return pres(FileIcon::Config, FileTone::Green, "Ortam yapılandırması");
    }
    let compose = matches!(
        name.as_str(),
        "compose.yml" | "compose.yaml" | "docker-compose.yml" | "docker-compose.yaml"
    );
    if name == "dockerfile" || name.starts_with("dockerfile.") || compose {
        return pres(FileIcon::Config, FileTone::Cyan, "Docker yapılandırması");
    }
    if matches!(
        name.as_str(),
        "makefile" | "gnumakefile" | "cmakelists.txt" | "justfile" | "gemfile" | "procfile"
    ) {
        return pres(
            FileIcon::Config,
            FileTone::Green,
            "Derleme veya çalışma yapılandırması",
        );
    }
    if matches!(
        name.as_str(),
        ".gitignore" | ".gitattributes" | ".gitmodules"
    ) {
        return pres(FileIcon::Config, FileTone::Orange, "Git yapılandırması");
    }
    if matches!(
        name.as_str(),
        ".editorconfig" | ".npmrc" | ".nvmrc" | ".prettierrc" | ".eslintrc" | ".bashrc" | ".zshrc"
    ) {
        return pres(FileIcon::Config, FileTone::Green, "Yapılandırma");
    }
    if matches!(
        name.as_str(),
        "cargo.lock"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "package-lock.json"
            | "bun.lock"
            | "bun.lockb"
            | "composer.lock"
            | "poetry.lock"
            | "uv.lock"
            | "gemfile.lock"
    ) {
        return pres(FileIcon::Config, FileTone::Neutral, "Bağımlılık kilidi");
    }
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    FAMILIES
        .iter()
        .find(|(extensions, _)| extensions.contains(&extension))
        .map_or(
            pres(FileIcon::File, FileTone::Neutral, "Dosya"),
            |(_, p)| *p,
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(name: &str) -> FilePresentation {
        presentation(name, false, false)
    }

    #[test]
    fn exact_names_win_over_extensions() {
        assert_eq!(of("Cargo.lock").label, "Bağımlılık kilidi");
        assert_eq!(of("Dockerfile").tone, FileTone::Cyan);
        assert_eq!(of("docker-compose.yml").label, "Docker yapılandırması");
        assert_eq!(of(".env.local").label, "Ortam yapılandırması");
        assert_eq!(of(".gitignore").tone, FileTone::Orange);
        assert_eq!(of("Makefile").icon, FileIcon::Config);
    }

    #[test]
    fn extensions_are_case_insensitive() {
        assert_eq!(of("MAIN.RS").icon, FileIcon::Rust);
        assert_eq!(of("photo.JPEG").icon, FileIcon::Image);
        assert_eq!(of("notes.Md").icon, FileIcon::Markdown);
    }

    #[test]
    fn unknown_and_extensionless_files_are_plain() {
        assert_eq!(of("LICENSE").icon, FileIcon::File);
        assert_eq!(of("data.unknownext").icon, FileIcon::File);
        assert_eq!(of("trailing.").icon, FileIcon::File);
    }

    #[test]
    fn folders_follow_their_open_state() {
        assert_eq!(presentation("src", true, false).icon, FileIcon::Folder);
        assert_eq!(presentation("src", true, true).icon, FileIcon::FolderOpen);
    }

    #[test]
    fn every_family_uses_a_listed_icon_and_no_extension_repeats() {
        let mut seen = std::collections::HashSet::new();
        for (extensions, presentation) in FAMILIES {
            assert!(FileIcon::ALL.contains(&presentation.icon));
            for extension in *extensions {
                assert!(seen.insert(*extension), "duplicate extension {extension}");
            }
        }
    }

    #[test]
    fn asset_names_are_unique() {
        let names: std::collections::HashSet<_> = FileIcon::ALL.iter().map(|i| i.asset()).collect();
        assert_eq!(names.len(), 28);
    }

    /// Relative luminance contrast of two 0xRRGGBB colours.
    fn contrast(a: u32, b: u32) -> f64 {
        let luminance = |c: u32| {
            let channel = |shift: u32| {
                let v = f64::from((c >> shift) & 0xff) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
        };
        let (hi, lo) = (
            luminance(a).max(luminance(b)),
            luminance(a).min(luminance(b)),
        );
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn icon_colours_keep_their_contrast_on_both_surfaces() {
        let tones = [
            FileTone::Neutral,
            FileTone::Blue,
            FileTone::Yellow,
            FileTone::Orange,
            FileTone::Green,
            FileTone::Purple,
            FileTone::Cyan,
            FileTone::Pink,
        ];
        for tone in tones {
            assert!(
                contrast(tone.color(Tone::Dark), 0x1e1e1e) >= 4.5,
                "{tone:?} dark"
            );
            assert!(
                contrast(tone.color(Tone::Light), 0xffffff) >= 4.5,
                "{tone:?} light"
            );
        }
    }
}
