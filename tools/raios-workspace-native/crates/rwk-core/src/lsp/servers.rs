//! Which language server serves which file. The catalog is fixed: a project cannot name a
//! command, so opening a repository never runs anything the user did not install themselves.

use std::path::PathBuf;

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

/// The server and the LSP `languageId` for a project-relative path, by extension.
pub fn spec_for_path(path: &str) -> Option<(&'static ServerSpec, &'static str)> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
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
