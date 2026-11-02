//! Toggling line comments: which prefix a language uses and the text edit that comments or
//! uncomments a run of lines.

/// The line-comment prefix of the language of `path`, `None` where there is no line comment.
pub fn line_comment_prefix(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    match (name, extension) {
        ("Dockerfile" | "Makefile" | "makefile", _) => Some("#"),
        (_, Some(extension)) => match extension.to_ascii_lowercase().as_str() {
            "rs" | "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "go" | "c" | "h" | "cc"
            | "cpp" | "hpp" | "cs" | "java" | "kt" | "kts" | "swift" | "scala" | "dart" | "php"
            | "zig" | "proto" | "jsonc" => Some("//"),
            "py" | "sh" | "bash" | "zsh" | "fish" | "rb" | "toml" | "yaml" | "yml" | "conf"
            | "ini" | "r" | "pl" | "mk" | "cmake" | "nix" | "ex" | "exs" | "tf" | "gitignore" => {
                Some("#")
            }
            "lua" | "sql" | "hs" | "elm" | "ada" => Some("--"),
            "vim" => Some("\""),
            _ => None,
        },
        _ => None,
    }
}

/// `text` with the lines `first..=last` (zero-based) commented, or uncommented when every
/// non-blank one of them already is. Blank lines stay as they are.
pub fn toggle_line_comments(text: &str, first: usize, last: usize, prefix: &str) -> String {
    let had_trailing_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let last = last.min(lines.len().saturating_sub(1));
    if first > last || lines.is_empty() {
        return text.to_owned();
    }
    let range = first..=last;
    let body = |line: &String| line.trim_start().to_owned();
    let all_commented = lines[range.clone()]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .all(|line| body(line).starts_with(prefix));
    let any = lines[range.clone()]
        .iter()
        .any(|line| !line.trim().is_empty());
    if !any {
        return text.to_owned();
    }
    if all_commented {
        for line in &mut lines[range] {
            if line.trim().is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            let rest = &line[indent + prefix.len()..];
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            *line = format!("{}{}", &line[..indent], rest);
        }
    } else {
        // The comment marks sit in one column: the shallowest indent of the lines.
        let column = lines[range.clone()]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        for line in &mut lines[range] {
            if line.trim().is_empty() {
                continue;
            }
            line.insert_str(column, &format!("{prefix} "));
        }
    }
    let mut out = lines.join("\n");
    if had_trailing_newline {
        out.push('\n');
    }
    out
}

/// `120` or `120:8` as 1-based line and column (the column defaults to 1); `None` for anything
/// else, and for a zero.
pub fn parse_line_target(text: &str) -> Option<(u32, u32)> {
    let (line, column) = match text.trim().split_once(':') {
        Some((line, column)) => (line, column),
        None => (text.trim(), "1"),
    };
    let line: u32 = line.trim().parse().ok()?;
    let column: u32 = column.trim().parse().ok()?;
    (line >= 1 && column >= 1).then_some((line, column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_target_is_a_line_with_an_optional_column() {
        assert_eq!(parse_line_target("120"), Some((120, 1)));
        assert_eq!(parse_line_target(" 7 : 3 "), Some((7, 3)));
        for bad in ["", "0", "a", "1:0", "1:2:3", "-4"] {
            assert_eq!(parse_line_target(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn languages_have_their_prefix() {
        assert_eq!(line_comment_prefix("src/main.rs"), Some("//"));
        assert_eq!(line_comment_prefix("a/b/run.py"), Some("#"));
        assert_eq!(line_comment_prefix("Dockerfile"), Some("#"));
        assert_eq!(line_comment_prefix("q.sql"), Some("--"));
        assert_eq!(line_comment_prefix("notes.md"), None);
        assert_eq!(line_comment_prefix("noextension"), None);
    }

    #[test]
    fn lines_are_commented_at_the_shallowest_indent_and_back() {
        let text = "fn a() {\n    let x = 1;\n\n      let y = 2;\n}\n";
        let commented = toggle_line_comments(text, 1, 3, "//");
        assert_eq!(
            commented,
            "fn a() {\n    // let x = 1;\n\n    //   let y = 2;\n}\n"
        );
        assert_eq!(toggle_line_comments(&commented, 1, 3, "//"), text);
    }

    #[test]
    fn a_mixed_run_is_commented_whole_and_blank_runs_are_left_alone() {
        assert_eq!(toggle_line_comments("# a\nb", 0, 1, "#"), "# # a\n# b");
        assert_eq!(toggle_line_comments("\n\n", 0, 1, "#"), "\n\n");
        // A range past the end is clamped.
        assert_eq!(toggle_line_comments("x", 0, 9, "#"), "# x");
    }
}
