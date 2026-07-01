use super::common::{DepsReport, OutdatedDep};
use raios_core::core::build::iac::{detect_iac_kind, IacKind};
use std::path::Path;
use std::process::Command;

pub fn check_iac(dir: &Path) -> DepsReport {
    match detect_iac_kind(dir) {
        Some(IacKind::Terraform) => check_terraform_deps(dir),
        Some(IacKind::DockerCompose) | Some(IacKind::Dockerfile) | None => {
            DepsReport::empty("IaC/Docker")
        }
    }
}

fn check_terraform_deps(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("IaC/Terraform");
    if let Ok(content) = std::fs::read_to_string(dir.join(".terraform.lock.hcl")) {
        report.has_lockfile = true;
        let deps = parse_terraform_lock(&content);
        report.outdated_count = deps.len();
        report.outdated = deps;
    }
    if Command::new("terraform").arg("version").output().is_err() {
        report.tool_missing.push(
            "terraform (install from https://developer.hashicorp.com/terraform/install)".into(),
        );
    }
    report
}

pub(crate) fn parse_terraform_lock(content: &str) -> Vec<OutdatedDep> {
    let mut deps = Vec::new();
    let mut current_provider = String::new();
    let mut current_version = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("provider \"registry.terraform.io/") {
            current_provider = trimmed
                .trim_start_matches("provider \"registry.terraform.io/")
                .split('"')
                .next()
                .unwrap_or("")
                .to_string();
            current_version.clear();
        }
        if trimmed.starts_with("version") && trimmed.contains('=') {
            current_version = trimmed
                .split_once('=')
                .map(|x| x.1)
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
        }
        if trimmed == "}" && !current_provider.is_empty() && !current_version.is_empty() {
            deps.push(OutdatedDep {
                name: current_provider.clone(),
                current: current_version.clone(),
                latest: "?".into(),
                kind: "provider".into(),
            });
            current_provider.clear();
            current_version.clear();
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_terraform_lock_hcl() {
        let content = r#"provider "registry.terraform.io/hashicorp/aws" {
  version     = "5.0.0"
  constraints = ">= 4.0.0"
}
provider "registry.terraform.io/hashicorp/random" {
  version = "3.5.1"
}
"#;
        let deps = parse_terraform_lock(content);
        assert_eq!(deps.len(), 2);
        assert!(deps
            .iter()
            .any(|d| d.name == "hashicorp/aws" && d.current == "5.0.0"));
        assert!(deps
            .iter()
            .any(|d| d.name == "hashicorp/random" && d.current == "3.5.1"));
    }

    #[test]
    fn parse_terraform_lock_empty() {
        let deps = parse_terraform_lock("# This file is maintained automatically\n");
        assert_eq!(deps.len(), 0);
    }
}
