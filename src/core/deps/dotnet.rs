use std::path::Path;
use std::process::Command;
use super::common::{DepsReport, OutdatedDep};

pub fn check_dotnet(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty(".NET");
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("csproj") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let deps = parse_csproj_packages(&content);
                    report.outdated_count = deps.len();
                    report.outdated = deps;
                    report.has_lockfile = dir.join("packages.lock.json").exists();
                }
                break;
            }
        }
    }
    if Command::new("dotnet").arg("--version").output().is_err() {
        report.tool_missing.push(
            "dotnet (.NET SDK not found; install from https://dotnet.microsoft.com)".into(),
        );
    }
    report
}

pub(crate) fn parse_csproj_packages(xml: &str) -> Vec<OutdatedDep> {
    let mut deps = Vec::new();
    for line in xml.lines() {
        let t = line.trim();
        if t.starts_with("<PackageReference") {
            let name = extract_xml_attr(t, "Include").unwrap_or_default();
            let version = extract_xml_attr(t, "Version").unwrap_or_default();
            if !name.is_empty() {
                deps.push(OutdatedDep {
                    name,
                    current: version,
                    latest: "?".into(),
                    kind: "nuget".into(),
                });
            }
        }
    }
    deps
}

fn extract_xml_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dotnet_csproj_packages() {
        let xml = r#"<Project Sdk="Microsoft.NET.Sdk">
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
    <PackageReference Include="Serilog" Version="3.1.1" />
  </ItemGroup>
</Project>"#;
        let deps = parse_csproj_packages(xml);
        assert_eq!(deps.len(), 2);
        assert!(deps
            .iter()
            .any(|d| d.name == "Newtonsoft.Json" && d.current == "13.0.3"));
        assert!(deps.iter().any(|d| d.name == "Serilog" && d.current == "3.1.1"));
    }

    #[test]
    fn parse_csproj_no_packages() {
        let xml = "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <Version>1.0.0</Version>\n  </PropertyGroup>\n</Project>";
        let deps = parse_csproj_packages(xml);
        assert_eq!(deps.len(), 0);
    }
}
