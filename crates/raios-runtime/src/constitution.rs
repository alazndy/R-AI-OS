use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct ConstitutionSection {
    pub level: u8,
    pub title: String,
    pub line_start: usize,
    pub line_end: usize,
    pub items: Vec<String>,
    pub children: Vec<ConstitutionSection>,
}

pub fn parse_sections(content: &str) -> Vec<ConstitutionSection> {
    let lines: Vec<&str> = content.lines().collect();
    let mut sections = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(title) = lines[i].strip_prefix("## ") {
            let start = i;
            i += 1;
            let (items, children, end) = parse_body(&lines, &mut i, start, false);
            sections.push(ConstitutionSection {
                level: 1,
                title: title.trim().to_string(),
                line_start: start,
                line_end: end,
                items,
                children,
            });
        } else {
            i += 1;
        }
    }
    sections
}

/// Consumes lines starting at `*i` until the next `## ` header (exclusive), collecting
/// plain body lines as `items` and `### ` headers as `children`. Returns the last
/// non-empty line index consumed (falls back to `fallback_line` if nothing was consumed).
///
/// `stop_at_child_header` controls whether a `### ` header also terminates this call: the
/// top-level call from `parse_sections` passes `false` so it keeps consuming sibling `### `
/// headers as children of the enclosing `## ` section, while the recursive call made when a
/// `### ` header is hit passes `true` so it stops as soon as it sees the *next* `### ` (or
/// `## `) header, handing control back to the outer loop so that next sibling gets its own
/// entry instead of being swallowed into the previous child's body.
fn parse_body(
    lines: &[&str],
    i: &mut usize,
    fallback_line: usize,
    stop_at_child_header: bool,
) -> (Vec<String>, Vec<ConstitutionSection>, usize) {
    let mut items = Vec::new();
    let mut children = Vec::new();
    let mut last = fallback_line;
    while *i < lines.len() {
        let line = lines[*i];
        if line.starts_with("## ") {
            break;
        }
        if stop_at_child_header && line.starts_with("### ") {
            break;
        }
        if let Some(title) = line.strip_prefix("### ") {
            let start = *i;
            *i += 1;
            let (child_items, _grandchildren, end) = parse_body(lines, i, start, true);
            children.push(ConstitutionSection {
                level: 2,
                title: title.trim().to_string(),
                line_start: start,
                line_end: end,
                items: child_items,
                children: Vec::new(),
            });
            last = end;
            continue;
        }
        if !line.trim().is_empty() {
            items.push(strip_list_marker(line));
            last = *i;
        }
        *i += 1;
    }
    (items, children, last)
}

fn strip_list_marker(line: &str) -> String {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("* ") {
        return rest.to_string();
    }
    if let Some(rest) = t.strip_prefix("- ") {
        return rest.to_string();
    }
    if let Some(dot) = t.find(". ") {
        if !t[..dot].is_empty() && t[..dot].chars().all(|c| c.is_ascii_digit()) {
            return t[dot + 2..].to_string();
        }
    }
    t.to_string()
}

pub fn is_include_only(content: &str) -> bool {
    let meaningful: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    !meaningful.is_empty() && meaningful.iter().all(|l| l.starts_with('@'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFileKind {
    ClaudeMd,
    AgentsMd,
    GeminiMd,
}

impl ProjectFileKind {
    pub fn filename(&self) -> &'static str {
        match self {
            ProjectFileKind::ClaudeMd => "CLAUDE.md",
            ProjectFileKind::AgentsMd => "AGENTS.md",
            ProjectFileKind::GeminiMd => "GEMINI.md",
        }
    }
}

pub fn discover_project_constitution_files(project_root: &Path) -> Vec<(ProjectFileKind, PathBuf)> {
    [
        ProjectFileKind::ClaudeMd,
        ProjectFileKind::AgentsMd,
        ProjectFileKind::GeminiMd,
    ]
    .into_iter()
    .filter_map(|kind| {
        let p = project_root.join(kind.filename());
        p.exists().then_some((kind, p))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_sections_and_items() {
        let content = "\
## 1. Identity\n\
* System Name: k-ai-ra\n\
* Role: Partner\n\
\n\
## 2. Standard\n\
Every task follows this loop.\n\
1. Requirement\n\
2. Investigation\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "1. Identity");
        assert_eq!(
            sections[0].items,
            vec!["System Name: k-ai-ra", "Role: Partner"]
        );
        assert_eq!(sections[1].title, "2. Standard");
        assert_eq!(
            sections[1].items,
            vec![
                "Every task follows this loop.",
                "Requirement",
                "Investigation"
            ]
        );
    }

    #[test]
    fn parses_nested_subsections_as_children() {
        let content = "\
## 4. Engineering Standards\n\
### AgentShield: Absolute OWASP Rules\n\
1. **Broken Access Control:** Enforce least privilege.\n\
2. **Cryptographic Failures:** No custom crypto.\n\
\n\
## 5. Communication\n\
Turkish in chat.\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "4. Engineering Standards");
        assert!(sections[0].items.is_empty());
        assert_eq!(sections[0].children.len(), 1);
        assert_eq!(
            sections[0].children[0].title,
            "AgentShield: Absolute OWASP Rules"
        );
        assert_eq!(
            sections[0].children[0].items,
            vec![
                "**Broken Access Control:** Enforce least privilege.",
                "**Cryptographic Failures:** No custom crypto.",
            ]
        );
        assert_eq!(sections[1].title, "5. Communication");
    }

    #[test]
    fn parses_multiple_sibling_subsections_without_swallowing_them() {
        let content = "\
## 4. Engineering Standards\n\
### Skeleton-First Architecture (Mandatory)\n\
* No Blind Coding rule\n\
\n\
### AgentShield: Absolute OWASP Rules\n\
1. Broken Access Control\n\
2. Cryptographic Failures\n\
\n\
### Anti-Laziness\n\
* Never write lazy shortcuts\n\
\n\
## 5. Communication\n\
Turkish in chat.\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);

        let engineering = &sections[0];
        assert_eq!(engineering.title, "4. Engineering Standards");
        assert!(engineering.items.is_empty());
        assert_eq!(engineering.children.len(), 3);

        assert_eq!(
            engineering.children[0].title,
            "Skeleton-First Architecture (Mandatory)"
        );
        assert_eq!(engineering.children[0].items, vec!["No Blind Coding rule"]);

        assert_eq!(
            engineering.children[1].title,
            "AgentShield: Absolute OWASP Rules"
        );
        assert_eq!(
            engineering.children[1].items,
            vec!["Broken Access Control", "Cryptographic Failures"]
        );

        assert_eq!(engineering.children[2].title, "Anti-Laziness");
        assert_eq!(
            engineering.children[2].items,
            vec!["Never write lazy shortcuts"]
        );

        let communication = &sections[1];
        assert_eq!(communication.title, "5. Communication");
        assert!(communication.children.is_empty());
        assert_eq!(communication.items, vec!["Turkish in chat."]);
    }

    #[test]
    fn line_ranges_cover_header_through_last_body_line() {
        let content = "## A\nfoo\nbar\n## B\nbaz\n";
        let sections = parse_sections(content);
        assert_eq!(sections[0].line_start, 0);
        assert_eq!(sections[0].line_end, 2);
        assert_eq!(sections[1].line_start, 3);
        assert_eq!(sections[1].line_end, 4);
    }

    #[test]
    fn no_headers_yields_empty_sections() {
        assert!(parse_sections("just some text\nno headers here\n").is_empty());
        assert!(parse_sections("").is_empty());
    }

    #[test]
    fn include_only_file_detected() {
        assert!(is_include_only("@/home/alaz/AGENT_CONSTITUTION.md\n"));
        assert!(is_include_only("\n@/home/alaz/AGENT_CONSTITUTION.md\n\n"));
        assert!(!is_include_only("## 1. Identity\nSome real content\n"));
        assert!(!is_include_only(""));
    }

    #[test]
    fn discover_project_files_finds_existing_ones_only() {
        let dir =
            std::env::temp_dir().join(format!("raios-constitution-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "@/home/alaz/AGENT_CONSTITUTION.md\n").unwrap();
        // AGENTS.md and GEMINI.md deliberately absent

        let found = discover_project_constitution_files(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, ProjectFileKind::ClaudeMd);
        assert_eq!(found[0].1, dir.join("CLAUDE.md"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
