//! Code snippets: a few built in, more from the user's `snippets.json`, expanded at the cursor.

use serde::Deserialize;

/// Longest snippet file read, parsed or inserted, in bytes.
pub const MAX_SNIPPET_BYTES: usize = 64 * 1024;

/// One snippet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Snippet {
    /// The name the picker lists.
    pub prefix: String,
    /// File extensions it applies to (`rs`); empty means every file.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// The text: `${1:default}` / `$1` mark places to edit (the first one gets the cursor),
    /// `$0` the final cursor place, `\$` a literal dollar.
    pub body: String,
}

#[derive(Deserialize)]
struct File {
    snippets: Vec<Snippet>,
}

/// Reads `{"snippets":[{"prefix":"fn","extensions":["rs"],"body":"fn ${1:name}() {\n\t$0\n}"}]}`.
/// A broken or oversized file gives no snippets.
pub fn parse_user_snippets(text: &str) -> Vec<Snippet> {
    if text.len() > MAX_SNIPPET_BYTES {
        return Vec::new();
    }
    serde_json::from_str::<File>(text)
        .map(|file| {
            file.snippets
                .into_iter()
                .filter(|s| !s.prefix.trim().is_empty() && s.body.len() <= MAX_SNIPPET_BYTES)
                .collect()
        })
        .unwrap_or_default()
}

fn builtin(prefix: &str, extension: &str, body: &str) -> Snippet {
    Snippet {
        prefix: prefix.to_owned(),
        extensions: vec![extension.to_owned()],
        body: body.to_owned(),
    }
}

/// The snippets that ship with the application.
pub fn builtin_snippets() -> Vec<Snippet> {
    vec![
        builtin("fn", "rs", "fn ${1:name}($2) {\n\t$0\n}"),
        builtin("test", "rs", "#[test]\nfn ${1:name}() {\n\t$0\n}"),
        builtin("impl", "rs", "impl ${1:Type} {\n\t$0\n}"),
        builtin("match", "rs", "match ${1:value} {\n\t${2:_} => $0,\n}"),
        builtin("def", "py", "def ${1:name}($2):\n\t$0"),
        builtin("ifmain", "py", "if __name__ == \"__main__\":\n\t$0"),
        builtin("fn", "js", "function ${1:name}($2) {\n\t$0\n}"),
        builtin("fn", "ts", "function ${1:name}($2) {\n\t$0\n}"),
        builtin("func", "go", "func ${1:name}($2) {\n\t$0\n}"),
        builtin("main", "c", "int main(void) {\n\t$0\n\treturn 0;\n}"),
    ]
}

/// Built-in and user snippets that apply to `path`; a user snippet hides a built-in one with
/// the same prefix. Sorted by prefix.
pub fn snippets_for(path: &str, user: Vec<Snippet>) -> Vec<Snippet> {
    let extension = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let applies = |s: &Snippet| {
        s.extensions.is_empty()
            || extension
                .as_ref()
                .is_some_and(|e| s.extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
    };
    let mut all: Vec<Snippet> = user.into_iter().filter(|s| applies(s)).collect();
    for snippet in builtin_snippets().into_iter().filter(|s| applies(s)) {
        if !all.iter().any(|s| s.prefix == snippet.prefix) {
            all.push(snippet);
        }
    }
    all.sort_by(|a, b| a.prefix.cmp(&b.prefix));
    all
}

/// A snippet ready to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion {
    /// The text, placeholders replaced by their defaults, continuation lines indented.
    pub text: String,
    /// Byte offset in `text` where the cursor goes (the first tab stop, else `$0`, else the end).
    pub cursor: usize,
}

/// Expands `body`; every line after the first starts with `indent` (the indentation of the line
/// the snippet is inserted into).
pub fn expand(body: &str, indent: &str) -> Expansion {
    let mut text = String::new();
    let mut first: Option<usize> = None;
    let mut last: Option<usize> = None;
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if chars.peek() == Some(&'$') => {
                chars.next();
                text.push('$');
            }
            '$' => {
                let mut default = None;
                let mut digits = String::new();
                if chars.peek() == Some(&'{') {
                    chars.next();
                    while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                        digits.push(d);
                        chars.next();
                    }
                    if chars.peek() == Some(&':') {
                        chars.next();
                        let mut value = String::new();
                        for c in chars.by_ref() {
                            if c == '}' {
                                break;
                            }
                            value.push(c);
                        }
                        default = Some(value);
                    } else if chars.peek() == Some(&'}') {
                        chars.next();
                    }
                } else {
                    while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                        digits.push(d);
                        chars.next();
                    }
                }
                if digits.is_empty() {
                    text.push('$');
                    continue;
                }
                let at = text.len();
                if digits == "0" {
                    last = Some(at);
                } else if first.is_none() {
                    first = Some(at);
                }
                text.push_str(&default.unwrap_or_default());
            }
            '\n' => {
                text.push('\n');
                text.push_str(indent);
            }
            other => text.push(other),
        }
    }
    let cursor = first.or(last).unwrap_or(text.len());
    Expansion { text, cursor }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_keep_their_defaults_and_the_cursor_goes_to_the_first() {
        let e = expand("fn ${1:name}($2) {\n\t$0\n}", "  ");
        assert_eq!(e.text, "fn name() {\n  \t\n  }");
        assert_eq!(&e.text[e.cursor..e.cursor + 4], "name");
    }

    #[test]
    fn the_final_stop_is_used_without_numbered_ones_and_a_dollar_can_be_escaped() {
        let e = expand("a$0b \\$5 $", "");
        assert_eq!(e.text, "ab $5 $");
        assert_eq!(e.cursor, 1);
        assert_eq!(expand("plain", "").cursor, 5);
    }

    #[test]
    fn user_snippets_win_by_prefix_and_filter_by_extension() {
        let user = parse_user_snippets(
            r#"{"snippets":[{"prefix":"fn","extensions":["rs"],"body":"mine"},{"prefix":"todo","body":"// TODO"}]}"#,
        );
        let rust = snippets_for("src/a.rs", user.clone());
        assert_eq!(rust.iter().filter(|s| s.prefix == "fn").count(), 1);
        assert_eq!(rust.iter().find(|s| s.prefix == "fn").unwrap().body, "mine");
        assert!(rust.iter().any(|s| s.prefix == "todo"));
        let python = snippets_for("a.py", user);
        assert!(python.iter().any(|s| s.prefix == "def"));
        assert!(!python.iter().any(|s| s.prefix == "impl"));
        assert!(parse_user_snippets("not json").is_empty());
    }
}
