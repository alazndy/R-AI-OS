//! Where the appearance is stored and how: `$XDG_CONFIG_HOME/raios-workspace-native/appearance.json`
//! (fallback `~/.config`), owner-only. A missing, unreadable or invalid file gives the default
//! appearance, and a failed save is reported to the caller but never blocks the application.

use std::io::Write;
use std::path::{Path, PathBuf};

use rwk_core::workbench::{
    parse_appearance, parse_workspace, serialize_appearance, serialize_workspace, state_file_name,
    Appearance, SavedWorkspace, WorkspaceStateSource,
};

/// Directory name under the config home.
const APP_DIR: &str = "raios-workspace-native";
/// File name of the stored appearance.
const FILE: &str = "appearance.json";

/// The config home: `xdg` when it is an absolute path (the XDG rule), else `home/.config`.
pub fn config_home(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    match xdg.map(Path::new) {
        Some(path) if path.is_absolute() => Some(path.to_path_buf()),
        _ => home
            .map(Path::new)
            .filter(|path| path.is_absolute())
            .map(|path| path.join(".config")),
    }
}

/// File name of the user's task list.
const TASKS_FILE: &str = "tasks.json";
/// Largest task file read, in bytes.
const MAX_TASKS_FILE_BYTES: u64 = 256 * 1024;

/// The tasks file inside a config home.
pub fn tasks_path(config_home: &Path) -> PathBuf {
    config_home.join(APP_DIR).join(TASKS_FILE)
}

const SERVERS_FILE: &str = "language-servers.json";

/// The user's own language servers inside a config home.
pub fn servers_path(config_home: &Path) -> PathBuf {
    config_home.join(APP_DIR).join(SERVERS_FILE)
}

/// Reads the user's language-server file (once, at start); nothing when there is none.
pub fn load_user_servers(path: &Path) -> Vec<(rwk_core::lsp::ServerSpec, Vec<String>, String)> {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut text = String::new();
    if file
        .take(MAX_TASKS_FILE_BYTES)
        .read_to_string(&mut text)
        .is_err()
    {
        return Vec::new();
    }
    rwk_core::lsp::parse_user_servers(&text)
}

/// Reads the user's task file on every call, so edits apply without a restart.
pub struct FsTaskStore {
    path: PathBuf,
}

impl FsTaskStore {
    /// A store over the tasks file inside `config_home`.
    pub fn new(config_home: &Path) -> Self {
        Self {
            path: tasks_path(config_home),
        }
    }
}

impl rwk_core::runner::TaskSource for FsTaskStore {
    fn load(&self, project: &Path) -> Vec<rwk_core::runner::RunTask> {
        use std::io::Read;
        let Ok(file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let mut text = String::new();
        if file
            .take(MAX_TASKS_FILE_BYTES)
            .read_to_string(&mut text)
            .is_err()
        {
            return Vec::new();
        }
        rwk_core::runner::tasks_for(&text, project)
    }
}

/// File name of the projects the daemon confirmed as registered.
const REGISTERED_FILE: &str = "registered-projects.json";

/// The registered-projects file inside a config home.
pub fn registered_path(config_home: &Path) -> PathBuf {
    config_home.join(APP_DIR).join(REGISTERED_FILE)
}

/// The projects remembered as registered; a missing or invalid file gives none.
pub fn load_registered(path: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect()
}

/// Remembers `project` as registered (no duplicates), atomically and owner-only.
pub fn remember_registered(path: &Path, project: &Path) -> std::io::Result<()> {
    let mut projects = load_registered(path);
    if projects.iter().any(|known| known == project) {
        return Ok(());
    }
    projects.push(project.to_path_buf());
    let Some(directory) = path.parent() else {
        return Err(std::io::Error::other("the registry path has no directory"));
    };
    create_private_dir(directory)?;
    let temporary = directory.join(format!(".{REGISTERED_FILE}.tmp"));
    let names: Vec<String> = projects
        .iter()
        .map(|project| project.to_string_lossy().into_owned())
        .collect();
    let mut file = open_private(&temporary)?;
    file.write_all(serde_json::to_string(&names).unwrap_or_default().as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

const RECENT_FILE: &str = "recent-projects.json";

/// How many projects the recent list keeps.
pub const MAX_RECENT_PROJECTS: usize = 10;

/// The recent-projects file inside a config home.
pub fn recent_path(config_home: &Path) -> PathBuf {
    config_home.join(APP_DIR).join(RECENT_FILE)
}

/// The stored recent projects, newest first; projects that no longer exist are left out.
pub fn load_recent(path: &Path) -> Vec<PathBuf> {
    let names: Vec<String> = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    names
        .into_iter()
        .map(PathBuf::from)
        .filter(|project| project.is_dir())
        .take(MAX_RECENT_PROJECTS)
        .collect()
}

/// Writes the recent projects atomically, owner-only.
pub fn save_recent(path: &Path, projects: &[PathBuf]) -> std::io::Result<()> {
    let Some(directory) = path.parent() else {
        return Err(std::io::Error::other("the recent path has no directory"));
    };
    create_private_dir(directory)?;
    let temporary = directory.join(format!(".{RECENT_FILE}.tmp"));
    let names: Vec<String> = projects
        .iter()
        .take(MAX_RECENT_PROJECTS)
        .map(|project| project.to_string_lossy().into_owned())
        .collect();
    let mut file = open_private(&temporary)?;
    file.write_all(serde_json::to_string(&names).unwrap_or_default().as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

/// The appearance file inside a config home.
pub fn appearance_path(config_home: &Path) -> PathBuf {
    config_home.join(APP_DIR).join(FILE)
}

/// The stored appearance, or the default when there is none or it is invalid.
pub fn load_appearance(path: &Path) -> Appearance {
    parse_appearance(std::fs::read_to_string(path).ok().as_deref())
}

/// Writes the appearance atomically (temporary file, then rename). The directory is created
/// owner-only and the file is owner-only.
pub fn save_appearance(path: &Path, appearance: &Appearance) -> std::io::Result<()> {
    let Some(directory) = path.parent() else {
        return Err(std::io::Error::other(
            "the appearance path has no directory",
        ));
    };
    create_private_dir(directory)?;
    let temporary = directory.join(format!(".{FILE}.tmp"));
    let mut file = open_private(&temporary)?;
    file.write_all(serialize_appearance(appearance).as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

const PREFERENCES_FILE: &str = "preferences.json";

/// The editing switches that survive a restart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Preferences {
    /// Save changed files when the window loses focus.
    pub auto_save: bool,
    /// Format with the language server before saving.
    pub format_on_save: bool,
    /// What the toolbar shows; a missing or invalid entry is the default layout.
    pub toolbar: rwk_core::workbench::ToolbarLayout,
    /// Text size, tab width and wrapping.
    pub editor: rwk_core::workbench::EditorPrefs,
}

/// The preferences file inside a config home.
pub fn preferences_path(config_home: &Path) -> PathBuf {
    config_home.join(APP_DIR).join(PREFERENCES_FILE)
}

/// The stored preferences; anything missing or invalid is off.
pub fn load_preferences(path: &Path) -> Preferences {
    let value: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let flag = |key: &str| value.get(key).and_then(serde_json::Value::as_bool) == Some(true);
    Preferences {
        auto_save: flag("auto_save"),
        format_on_save: flag("format_on_save"),
        toolbar: value
            .get("toolbar")
            .and_then(|toolbar| serde_json::from_value(toolbar.clone()).ok())
            .unwrap_or_default(),
        editor: value
            .get("editor")
            .and_then(|editor| {
                serde_json::from_value::<rwk_core::workbench::EditorPrefs>(editor.clone()).ok()
            })
            .unwrap_or_default()
            .clamped(),
    }
}

/// Writes the preferences atomically, owner-only.
pub fn save_preferences(path: &Path, preferences: Preferences) -> std::io::Result<()> {
    let Some(directory) = path.parent() else {
        return Err(std::io::Error::other(
            "the preferences path has no directory",
        ));
    };
    create_private_dir(directory)?;
    let temporary = directory.join(format!(".{PREFERENCES_FILE}.tmp"));
    let mut file = open_private(&temporary)?;
    let text = serde_json::json!({
        "auto_save": preferences.auto_save,
        "format_on_save": preferences.format_on_save,
        "toolbar": preferences.toolbar,
        "editor": preferences.editor,
    });
    file.write_all(text.to_string().as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

/// Directory under the app dir holding one state file per project.
const WORKSPACES_DIR: &str = "workspaces";

/// The state file of `project` inside a config home.
pub fn workspace_path(config_home: &Path, project: &Path) -> PathBuf {
    config_home
        .join(APP_DIR)
        .join(WORKSPACES_DIR)
        .join(state_file_name(project))
}

/// The stored workspace of `project`, or `None` when there is none, it is unreadable, or it is
/// invalid (the parser refuses anything a hand-edited file could smuggle in).
pub fn load_workspace(path: &Path) -> Option<SavedWorkspace> {
    parse_workspace(std::fs::read_to_string(path).ok()?.as_str())
}

/// Writes one project's workspace atomically (temporary file, then rename), owner-only.
pub fn save_workspace(path: &Path, workspace: &SavedWorkspace) -> std::io::Result<()> {
    let Some(directory) = path.parent() else {
        return Err(std::io::Error::other("the workspace path has no directory"));
    };
    create_private_dir(directory)?;
    let Some(name) = path.file_name() else {
        return Err(std::io::Error::other("the workspace path has no file name"));
    };
    let temporary = directory.join(format!(".{}.tmp", name.to_string_lossy()));
    let mut file = open_private(&temporary)?;
    file.write_all(serialize_workspace(workspace).as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

/// The store the workbench reads from: one state file per project under the config home.
pub struct FsWorkspaceStore {
    config_home: PathBuf,
}

impl FsWorkspaceStore {
    /// A store rooted at `config_home`.
    pub fn new(config_home: PathBuf) -> Self {
        Self { config_home }
    }

    /// Writes the state of the project it describes, atomically and owner-only.
    pub fn save(&self, workspace: &SavedWorkspace) -> std::io::Result<()> {
        save_workspace(
            &workspace_path(&self.config_home, &workspace.project),
            workspace,
        )
    }
}

impl WorkspaceStateSource for FsWorkspaceStore {
    fn load(&self, project: &Path) -> Option<SavedWorkspace> {
        load_workspace(&workspace_path(&self.config_home, project))
    }
}

#[cfg(unix)]
fn create_private_dir(directory: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
}

#[cfg(not(unix))]
fn create_private_dir(directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)
}

#[cfg(unix)]
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rwk_core::workbench::{Skin, Tone};

    #[test]
    fn preferences_round_trip_and_default_to_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = preferences_path(dir.path());
        assert_eq!(load_preferences(&path), Preferences::default());
        let on = Preferences {
            auto_save: true,
            format_on_save: false,
            toolbar: rwk_core::workbench::ToolbarLayout::preset(
                rwk_core::workbench::ToolbarPreset::Minimal,
            ),
            editor: rwk_core::workbench::EditorPrefs {
                font_size: 18,
                tab_size: 2,
                soft_wrap: true,
            },
        };
        save_preferences(&path, on).unwrap();
        assert_eq!(load_preferences(&path), on);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_preferences(&path), Preferences::default());
    }

    #[test]
    fn recent_projects_round_trip_and_skip_folders_that_are_gone() {
        let dir = tempfile::tempdir().unwrap();
        let path = recent_path(dir.path());
        assert!(load_recent(&path).is_empty());
        let here = dir.path().to_path_buf();
        let gone = dir.path().join("gone");
        save_recent(&path, &[here.clone(), gone]).unwrap();
        assert_eq!(load_recent(&path), vec![here]);
        std::fs::write(&path, "not json").unwrap();
        assert!(load_recent(&path).is_empty());
    }

    #[test]
    fn the_appearance_file_lives_under_the_xdg_config_home() {
        let home = config_home(Some("/xdg"), Some("/home/u")).expect("xdg");
        assert_eq!(
            appearance_path(&home),
            Path::new("/xdg/raios-workspace-native/appearance.json")
        );
    }

    #[test]
    fn a_relative_or_missing_xdg_falls_back_to_the_home_config() {
        assert_eq!(
            config_home(Some("relative/dir"), Some("/home/u")),
            Some(PathBuf::from("/home/u/.config"))
        );
        assert_eq!(
            config_home(None, Some("/home/u")),
            Some(PathBuf::from("/home/u/.config"))
        );
        assert_eq!(config_home(None, None), None);
        assert_eq!(config_home(None, Some("not-absolute")), None);
    }

    #[test]
    fn a_missing_unreadable_or_invalid_file_gives_the_default_appearance() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(
            load_appearance(&dir.path().join("none.json")),
            Appearance::default()
        );
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{not json").expect("write");
        assert_eq!(load_appearance(&bad), Appearance::default());
        assert_eq!(
            load_appearance(dir.path()),
            Appearance::default(),
            "a directory is unreadable"
        );
    }

    #[test]
    fn saving_creates_the_directory_owner_only_and_round_trips() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().expect("dir");
        let path = appearance_path(home.path());
        let chosen = Appearance {
            skin: Skin::Lcars,
            tone: Tone::Light,
        };
        save_appearance(&path, &chosen).expect("save");
        assert_eq!(load_appearance(&path), chosen);
        let dir_mode = std::fs::metadata(path.parent().expect("parent"))
            .expect("dir meta")
            .permissions()
            .mode();
        let file_mode = std::fs::metadata(&path)
            .expect("file meta")
            .permissions()
            .mode();
        assert_eq!(
            dir_mode & 0o077,
            0,
            "directory must be owner-only: {dir_mode:o}"
        );
        assert_eq!(
            file_mode & 0o077,
            0,
            "file must be owner-only: {file_mode:o}"
        );
    }

    #[test]
    fn a_failed_save_is_reported_but_never_panics() {
        let dir = tempfile::tempdir().expect("dir");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "a file where a directory is needed").expect("write");
        let result = save_appearance(
            &blocker.join("deeper").join("appearance.json"),
            &Appearance::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn registered_projects_round_trip_without_duplicates_and_ignore_junk() {
        let directory = tempfile::tempdir().expect("tempdir");
        let file = registered_path(directory.path());
        assert!(load_registered(&file).is_empty());

        remember_registered(&file, Path::new("/work/a")).expect("first");
        remember_registered(&file, Path::new("/work/b")).expect("second");
        remember_registered(&file, Path::new("/work/a")).expect("again");

        assert_eq!(
            load_registered(&file),
            vec![PathBuf::from("/work/a"), PathBuf::from("/work/b")]
        );
        std::fs::write(&file, r#"["relative", "/ok"]"#).expect("write");
        assert_eq!(load_registered(&file), vec![PathBuf::from("/ok")]);
        std::fs::write(&file, "not json").expect("write");
        assert!(load_registered(&file).is_empty());
    }

    #[test]
    fn the_workspace_file_lives_under_the_config_home_with_one_per_project() {
        let home = Path::new("/xdg");
        let one = workspace_path(home, Path::new("/work/app"));
        let two = workspace_path(home, Path::new("/work/other"));
        assert_eq!(
            one.parent(),
            Some(Path::new("/xdg/raios-workspace-native/workspaces"))
        );
        assert!(one.to_string_lossy().ends_with(".json"), "{one:?}");
        assert_ne!(one, two, "one state file per project");
    }

    #[test]
    fn a_saved_workspace_round_trips_through_the_store() {
        use rwk_core::workbench::{LayoutPreset, SavedTab, SavedTerminal};

        let directory = tempfile::tempdir().expect("dir");
        let store = FsWorkspaceStore::new(directory.path().to_path_buf());
        let project = Path::new("/work/app");
        assert!(store.load(project).is_none(), "nothing saved yet");

        let workspace = SavedWorkspace {
            project: project.to_path_buf(),
            tabs: vec![SavedTab {
                path: "src/main.rs".into(),
                line: 7,
            }],
            active: 0,
            layout: LayoutPreset::Editor,
            dock: None,
            terminals: vec![
                SavedTerminal::Shell { name: None },
                SavedTerminal::Agent {
                    kind: rwk_core::sessions::SessionKind::Claude,
                    native_session: None,
                    name: None,
                },
            ],
            task_links: Vec::new(),
        };
        store.save(&workspace).expect("save");
        assert_eq!(store.load(project), Some(workspace));
        assert!(store.load(Path::new("/work/other")).is_none());
    }

    #[test]
    fn a_workspace_save_creates_the_directory_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("dir");
        let store = FsWorkspaceStore::new(directory.path().to_path_buf());
        let state = SavedWorkspace {
            project: PathBuf::from("/work/app"),
            tabs: Vec::new(),
            active: 0,
            layout: rwk_core::workbench::LayoutPreset::Balanced,
            terminals: Vec::new(),
            dock: None,
            task_links: Vec::new(),
        };
        store.save(&state).expect("save");
        assert_eq!(
            WorkspaceStateSource::load(&store, &state.project),
            Some(state)
        );
        let path = workspace_path(directory.path(), Path::new("/work/app"));
        let dir_mode = std::fs::metadata(path.parent().expect("parent"))
            .expect("dir meta")
            .permissions()
            .mode();
        let file_mode = std::fs::metadata(&path)
            .expect("file meta")
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o077, 0, "{dir_mode:o}");
        assert_eq!(file_mode & 0o077, 0, "{file_mode:o}");
    }
}

#[cfg(test)]
mod task_store_tests {
    use super::*;
    use rwk_core::runner::TaskSource;

    #[test]
    fn the_task_file_is_read_from_the_config_directory_and_a_missing_one_gives_no_tasks() {
        let home = tempfile::tempdir().unwrap();
        let store = FsTaskStore::new(home.path());
        assert!(store.load(Path::new("/work/app")).is_empty());
        let path = tasks_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"projects": {"/work/app": [{"name": "test", "command": ["cargo", "test"]}]}}"#,
        )
        .unwrap();
        let tasks = store.load(Path::new("/work/app"));
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "test");
    }
}
