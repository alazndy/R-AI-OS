//! Which language server serves which file. The built-in catalog is fixed, and the user can
//! add servers of their own in a file under their config home (never in a project), so
//! opening a repository never runs anything the user did not install or name themselves.

use std::path::PathBuf;
use std::sync::OnceLock;

/// One language server: how to start it and which files it serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerSpec {
    /// Short name, also the key of one running server per project.
    pub name: &'static str,
    /// The executable, looked up on `PATH`.
    pub program: &'static str,
    /// Its arguments.
    pub args: &'static [&'static str],
}

/// The servers the workspace can use.
pub const SERVERS: [ServerSpec; 5] = [
    ServerSpec {
        name: "rust",
        program: "rust-analyzer",
        args: &[],
    },
    ServerSpec {
        name: "typescript",
        program: "typescript-language-server",
        args: &["--stdio"],
    },
    ServerSpec {
        name: "python",
        program: "pyright-langserver",
        args: &["--stdio"],
    },
    ServerSpec {
        name: "go",
        program: "gopls",
        args: &[],
    },
    ServerSpec {
        name: "c",
        program: "clangd",
        args: &[],
    },
];

/// A server the user added: its spec and the extensions it takes over.
struct UserServer {
    spec: &'static ServerSpec,
    extensions: Vec<String>,
    language: &'static str,
}

static USER_SERVERS: OnceLock<Vec<UserServer>> = OnceLock::new();

/// Most user servers read from the file.
const MAX_USER_SERVERS: usize = 32;

fn bare_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && !text.contains(['/', '\\', ' ', '\0'])
        && !text.starts_with('-')
}

/// Reads the user's server file:
/// `{"servers":[{"name":"zig","program":"zls","args":[],"extensions":["zig"],"language":"zig"}]}`.
/// An entry that is malformed, whose program is a path, or whose name repeats an earlier one
/// or a built-in one is skipped. Where extensions overlap, the user's server wins.
pub fn parse_user_servers(text: &str) -> Vec<(ServerSpec, Vec<String>, String)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let mut found: Vec<(ServerSpec, Vec<String>, String)> = Vec::new();
    let entries = value
        .get("servers")
        .and_then(|servers| servers.as_array())
        .into_iter()
        .flatten()
        .take(MAX_USER_SERVERS);
    for entry in entries {
        let text = |key: &str| entry.get(key).and_then(|v| v.as_str());
        let (Some(name), Some(program), Some(language)) =
            (text("name"), text("program"), text("language"))
        else {
            continue;
        };
        let args: Vec<&str> = entry
            .get("args")
            .and_then(|args| args.as_array())
            .map(|args| args.iter().filter_map(|arg| arg.as_str()).collect())
            .unwrap_or_default();
        let extensions: Vec<String> = entry
            .get("extensions")
            .and_then(|list| list.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|e| e.as_str())
                    .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                    .filter(|e| {
                        !e.is_empty() && e.len() <= 12 && e.chars().all(char::is_alphanumeric)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let taken =
            SERVERS.iter().any(|s| s.name == name) || found.iter().any(|(s, _, _)| s.name == name);
        if !bare_name(name)
            || !bare_name(program)
            || !bare_name(language)
            || taken
            || extensions.is_empty()
            || args.len() > 16
            || args.iter().any(|a| a.len() > 256 || a.contains('\0'))
        {
            continue;
        }
        // The values live as long as the process: a handful of strings read once at start.
        let leak = |text: &str| -> &'static str { Box::leak(text.to_owned().into_boxed_str()) };
        let args: Vec<&'static str> = args.into_iter().map(leak).collect();
        found.push((
            ServerSpec {
                name: leak(name),
                program: leak(program),
                args: Box::leak(args.into_boxed_slice()),
            },
            extensions,
            language.to_owned(),
        ));
    }
    found
}

/// Installs the user's servers; later calls are ignored (they are read once at start).
pub fn set_user_servers(servers: Vec<(ServerSpec, Vec<String>, String)>) {
    let _ = USER_SERVERS.set(
        servers
            .into_iter()
            .map(|(spec, extensions, language)| UserServer {
                spec: Box::leak(Box::new(spec)),
                extensions,
                language: Box::leak(language.into_boxed_str()),
            })
            .collect(),
    );
}

/// The server called `name`, built-in or the user's.
pub fn spec_named(name: &str) -> Option<&'static ServerSpec> {
    SERVERS.iter().find(|spec| spec.name == name).or_else(|| {
        USER_SERVERS
            .get()?
            .iter()
            .map(|user| user.spec)
            .find(|spec| spec.name == name)
    })
}

/// The server and the LSP `languageId` for a project-relative path, by extension.
pub fn spec_for_path(path: &str) -> Option<(&'static ServerSpec, &'static str)> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    if let Some(user) = USER_SERVERS
        .get()
        .and_then(|servers| servers.iter().find(|s| s.extensions.contains(&extension)))
    {
        return Some((user.spec, user.language));
    }
    let (server, language) = match extension.as_str() {
        "rs" => ("rust", "rust"),
        "ts" | "mts" | "cts" => ("typescript", "typescript"),
        "tsx" => ("typescript", "typescriptreact"),
        "js" | "mjs" | "cjs" => ("typescript", "javascript"),
        "jsx" => ("typescript", "javascriptreact"),
        "py" | "pyi" => ("python", "python"),
        "go" => ("go", "go"),
        "c" | "h" => ("c", "c"),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => ("c", "cpp"),
        _ => return None,
    };
    SERVERS
        .iter()
        .find(|spec| spec.name == server)
        .map(|spec| (spec, language))
}

/// The absolute path of `spec`'s program on `PATH`, when it is installed. A relative `PATH`
/// entry is ignored: it would make the result depend on the project directory.
pub fn find_program(spec: &ServerSpec) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(spec.program))
        .find(|path| crate::search::find_in_path(path.as_os_str()).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_pick_a_server_and_a_language_id() {
        let pick = |path: &str| spec_for_path(path).map(|(spec, language)| (spec.name, language));
        assert_eq!(pick("src/main.rs"), Some(("rust", "rust")));
        assert_eq!(pick("a/b.TSX"), Some(("typescript", "typescriptreact")));
        assert_eq!(pick("x.js"), Some(("typescript", "javascript")));
        assert_eq!(pick("t.py"), Some(("python", "python")));
        assert_eq!(pick("m.go"), Some(("go", "go")));
        assert_eq!(pick("k.hpp"), Some(("c", "cpp")));
        assert_eq!(pick("README"), None);
        assert_eq!(pick("notes.txt"), None);
        assert_eq!(pick(".rs"), Some(("rust", "rust")));
    }

    #[test]
    fn the_user_file_adds_servers_and_skips_unsafe_or_repeated_entries() {
        let found = parse_user_servers(
            r#"{"servers":[
                {"name":"zig","program":"zls","args":["--x"],"extensions":[".Zig","zon"],"language":"zig"},
                {"name":"evil","program":"/tmp/x","extensions":["x"],"language":"x"},
                {"name":"rust","program":"other","extensions":["rs"],"language":"rust"},
                {"name":"zig","program":"zls2","extensions":["z2"],"language":"z"},
                {"name":"noext","program":"p","extensions":[],"language":"p"},
                {"name":"-flag","program":"p","extensions":["q"],"language":"p"}
            ]}"#,
        );
        assert_eq!(found.len(), 1);
        let (spec, extensions, language) = &found[0];
        assert_eq!(
            (spec.name, spec.program, spec.args),
            ("zig", "zls", &["--x"][..])
        );
        assert_eq!(extensions, &["zig", "zon"]);
        assert_eq!(language, "zig");
        assert!(parse_user_servers("not json").is_empty());
    }

    #[test]
    fn every_catalog_name_is_unique_and_has_a_fixed_program() {
        let mut names: Vec<&str> = SERVERS.iter().map(|spec| spec.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SERVERS.len());
        assert!(SERVERS
            .iter()
            .all(|spec| !spec.program.contains(['/', '\\', ' '])));
    }
}
