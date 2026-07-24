use std::path::Path;

#[derive(Debug, Clone)]
pub struct Violation {
    pub line: usize,
    pub rule: &'static str,
    pub severity: u8,
}

#[derive(Debug, Clone)]
pub struct ComplianceReport {
    pub score: u8,
    pub violations: Vec<Violation>,
    pub file_type: FileType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    Rust,
    TypeScript,
    Python,
    Markdown,
    Config,
    Other,
}

impl ComplianceReport {
    pub fn grade(&self) -> &'static str {
        match self.score {
            90..=100 => "A",
            80..=89 => "B",
            70..=79 => "C",
            60..=69 => "D",
            _ => "F",
        }
    }

    pub fn score_color(&self) -> u8 {
        // 0=green, 1=amber, 2=red
        if self.score >= 80 {
            0
        } else if self.score >= 60 {
            1
        } else {
            2
        }
    }

    pub fn language(&self) -> &'static str {
        match self.file_type {
            FileType::Rust => "Rust",
            FileType::TypeScript => "TS",
            FileType::Python => "Py",
            FileType::Markdown => "MD",
            FileType::Config => "Config",
            FileType::Other => "",
        }
    }

    pub fn first_issue(&self) -> Option<String> {
        self.violations.first().map(|v| {
            if v.line > 0 {
                format!("Ln {:3}: {}", v.line, v.rule)
            } else {
                v.rule.to_string()
            }
        })
    }
}

pub fn check_file(path: &Path, content: &str) -> ComplianceReport {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let file_type = match ext {
        "rs" => FileType::Rust,
        "ts" | "tsx" => FileType::TypeScript,
        "py" => FileType::Python,
        "md" => FileType::Markdown,
        "toml" | "json" | "yaml" | "yml" => FileType::Config,
        _ => FileType::Other,
    };

    let mut violations: Vec<Violation> = Vec::new();

    for (i, line) in content.lines().enumerate() {
        let ln = i + 1;
        check_secrets(line, ln, &mut violations);
        match file_type {
            FileType::Rust => check_rust(line, ln, &mut violations),
            FileType::TypeScript => check_typescript(line, ln, &mut violations),
            FileType::Python => check_python(line, ln, &mut violations),
            _ => {}
        }
    }

    if path
        .file_name()
        .map(|n| n == "package.json")
        .unwrap_or(false)
    {
        check_package_json(content, &mut violations);
    }

    let total_deduction = violations
        .iter()
        .map(|v| v.severity as u32)
        .sum::<u32>()
        .min(100) as u8;

    ComplianceReport {
        score: 100u8.saturating_sub(total_deduction),
        violations,
        file_type,
    }
}

fn check_secrets(line: &str, ln: usize, v: &mut Vec<Violation>) {
    let t = line.trim();
    if t.starts_with("//") || t.starts_with('#') || t.starts_with('*') {
        return;
    }
    let lower = line.to_lowercase();
    let has_key = lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("password")
        || lower.contains("secret_key")
        || lower.contains("access_token");
    let has_assign = line.contains('=') || line.contains(':');
    let has_literal = line.contains('"') || line.contains('\'') || line.contains('`');
    if has_key && has_assign && has_literal {
        v.push(Violation {
            line: ln,
            rule: "Possible hardcoded secret/key",
            severity: 25,
        });
    }
}

fn check_rust(line: &str, ln: usize, v: &mut Vec<Violation>) {
    let t = line.trim();
    if !t.starts_with("//") {
        if t.contains(".unwrap()") && !t.contains("assert") && !t.contains("#[test]") {
            v.push(Violation {
                line: ln,
                rule: "Prefer ? over .unwrap()",
                severity: 3,
            });
        }
        if t.starts_with("println!") || t.starts_with("print!") {
            v.push(Violation {
                line: ln,
                rule: "Prefer log::* over println!/print!",
                severity: 2,
            });
        }
        if t.starts_with("panic!(") {
            v.push(Violation {
                line: ln,
                rule: "Avoid panic! — return Result instead",
                severity: 5,
            });
        }
    }
    if t.contains("// TODO") || t.contains("// FIXME") || t.contains("// HACK") {
        v.push(Violation {
            line: ln,
            rule: "Unresolved TODO/FIXME/HACK",
            severity: 1,
        });
    }
}

fn check_typescript(line: &str, ln: usize, v: &mut Vec<Violation>) {
    let t = line.trim();
    if !t.starts_with("//") {
        if t.contains(": any") || t.contains("<any>") || t.contains("as any") {
            v.push(Violation {
                line: ln,
                rule: "Avoid `any` — use unknown + type guard",
                severity: 5,
            });
        }
        if t.contains("console.log(") {
            v.push(Violation {
                line: ln,
                rule: "Remove console.log from production",
                severity: 3,
            });
        }
        if t.contains("!important") {
            v.push(Violation {
                line: ln,
                rule: "Avoid !important — fix specificity",
                severity: 4,
            });
        }
        if t.starts_with("export default ") && !t.contains("function ") && !t.contains("class ") {
            v.push(Violation {
                line: ln,
                rule: "Prefer named exports over export default",
                severity: 2,
            });
        }
    }
    if t.contains("// TODO") || t.contains("// FIXME") {
        v.push(Violation {
            line: ln,
            rule: "Unresolved TODO/FIXME",
            severity: 1,
        });
    }
}

fn check_python(line: &str, ln: usize, v: &mut Vec<Violation>) {
    let t = line.trim();
    if t == "except:" || t.starts_with("except: ") {
        v.push(Violation {
            line: ln,
            rule: "Bare except: — specify exception type",
            severity: 5,
        });
    }
    if t.starts_with("print(") && !t.starts_with("print(\"#") {
        v.push(Violation {
            line: ln,
            rule: "Use logging instead of print()",
            severity: 2,
        });
    }
}

fn check_package_json(content: &str, v: &mut Vec<Violation>) {
    if content.contains("\"npm install\"") || content.contains("npm i ") {
        v.push(Violation {
            line: 0,
            rule: "Use pnpm not npm (MASTER.md)",
            severity: 5,
        });
    }
    if content.contains("\"yarn ") || content.contains("\"yarn\"") {
        v.push(Violation {
            line: 0,
            rule: "Use pnpm not yarn (MASTER.md)",
            severity: 5,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn report(path: &str, content: &str) -> ComplianceReport {
        check_file(&PathBuf::from(path), content)
    }

    #[test]
    fn detects_hardcoded_secret_assignments_but_not_comments_or_discussion() {
        let detected = report("config.rs", "let api_key = \"sk-abc123\";");
        assert_eq!(detected.violations.len(), 1);
        assert_eq!(detected.violations[0].rule, "Possible hardcoded secret/key");
        assert_eq!(detected.score, 75);

        assert!(report("config.rs", "// api_key = \"sk-example\"")
            .violations
            .is_empty());
        assert!(report("notes.md", "discuss api_key rotation \"policy\"")
            .violations
            .is_empty());
    }

    #[test]
    fn rust_rules_distinguish_production_and_test_lines() {
        let production = report("lib.rs", "let value = fallible().unwrap();");
        assert_eq!(production.violations[0].rule, "Prefer ? over .unwrap()");

        assert!(
            report("lib.rs", "#[test] fn case() { fallible().unwrap(); }")
                .violations
                .is_empty()
        );
    }

    #[test]
    fn rust_rules_report_independent_panic_and_todo_violations() {
        let rules: Vec<&str> = report("lib.rs", "panic!(\"boom\"); // TODO: fix")
            .violations
            .iter()
            .map(|violation| violation.rule)
            .collect();
        assert!(rules.contains(&"Avoid panic! — return Result instead"));
        assert!(rules.contains(&"Unresolved TODO/FIXME/HACK"));
    }

    #[test]
    fn typescript_rules_cover_any_console_and_important() {
        let rules: Vec<&str> = report(
            "app.ts",
            "const value: any = console.log(x); /* !important */",
        )
        .violations
        .iter()
        .map(|violation| violation.rule)
        .collect();
        assert!(rules.contains(&"Avoid `any` — use unknown + type guard"));
        assert!(rules.contains(&"Remove console.log from production"));
        assert!(rules.contains(&"Avoid !important — fix specificity"));
    }

    #[test]
    fn python_bare_except_is_flagged_but_typed_except_is_not() {
        assert_eq!(
            report("app.py", "except:").violations[0].rule,
            "Bare except: — specify exception type"
        );
        assert!(report("app.py", "except ValueError:").violations.is_empty());
    }

    #[test]
    fn package_json_rejects_npm_and_yarn_but_allows_pnpm() {
        assert!(
            report("package.json", "{\"scripts\":{\"setup\":\"npm install\"}}")
                .violations
                .iter()
                .any(|violation| violation.rule == "Use pnpm not npm (MASTER.md)")
        );
        assert!(
            report("package.json", "{\"scripts\":{\"setup\":\"yarn install\"}}")
                .violations
                .iter()
                .any(|violation| violation.rule == "Use pnpm not yarn (MASTER.md)")
        );
        assert!(
            report("package.json", "{\"scripts\":{\"setup\":\"pnpm install\"}}")
                .violations
                .is_empty()
        );
    }

    #[test]
    fn score_saturates_and_summary_helpers_observe_thresholds() {
        let mut report = report("lib.rs", &".unwrap();\n".repeat(40));
        assert_eq!(report.score, 0);

        report.score = 100;
        assert_eq!((report.grade(), report.score_color()), ("A", 0));
        report.score = 65;
        assert_eq!((report.grade(), report.score_color()), ("D", 1));
        report.score = 10;
        assert_eq!((report.grade(), report.score_color()), ("F", 2));
    }

    #[test]
    fn first_issue_formats_line_numbers_and_file_types_are_classified() {
        assert_eq!(
            report("lib.rs", "let value = fallible().unwrap();").first_issue(),
            Some("Ln   1: Prefer ? over .unwrap()".to_string())
        );
        assert_eq!(
            report("data.xyz", "value.unwrap()").file_type,
            FileType::Other
        );
        assert_eq!(report("data.xyz", "value.unwrap()").language(), "");
    }
}
