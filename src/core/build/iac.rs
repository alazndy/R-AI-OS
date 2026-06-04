use std::path::Path;
use std::process::Command;
use std::time::Instant;
use super::common::{failed_result, failed_test, BuildResult, TestResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IacKind {
    Terraform,
    DockerCompose,
    Dockerfile,
}

pub fn detect_iac_kind(dir: &Path) -> Option<IacKind> {
    if std::fs::read_dir(dir).ok().is_some_and(|entries| {
        entries
            .flatten()
            .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("tf"))
    }) {
        return Some(IacKind::Terraform);
    }
    if dir.join("docker-compose.yml").exists() || dir.join("docker-compose.yaml").exists() {
        return Some(IacKind::DockerCompose);
    }
    if dir.join("Dockerfile").exists() {
        return Some(IacKind::Dockerfile);
    }
    None
}

fn parse_terraform_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Plan:")
        || output.contains("No changes.")
        || output.contains("Apply complete!");
    let errors = output
        .lines()
        .filter(|l| l.trim_start().starts_with("Error:"))
        .count();
    (ok && errors == 0, errors)
}

pub(crate) fn parse_terraform_validate_output(output: &str) -> (bool, usize) {
    let ok = output.contains("The configuration is valid");
    let errors = output
        .lines()
        .filter(|l| l.trim_start().starts_with("Error:"))
        .count();
    (ok && errors == 0, errors)
}

fn parse_docker_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Successfully built")
        || output.contains("writing image sha256:")
        || output.contains("Use 'docker scan'");
    let errors = output
        .lines()
        .filter(|l| {
            l.trim_start().starts_with("error:") || l.trim_start().starts_with("ERROR:")
        })
        .count();
    (ok && errors == 0, errors)
}

pub fn build_iac(dir: &Path) -> BuildResult {
    match detect_iac_kind(dir) {
        Some(IacKind::Terraform) => build_terraform(dir),
        Some(IacKind::DockerCompose) => build_docker_compose(dir),
        Some(IacKind::Dockerfile) => build_dockerfile(dir),
        None => failed_result(
            "IaC",
            "—",
            std::time::Duration::ZERO,
            "No IaC toolchain found (*.tf, docker-compose.yml, or Dockerfile)".into(),
        ),
    }
}

fn build_terraform(dir: &Path) -> BuildResult {
    let _ = Command::new("terraform")
        .args(["init", "-input=false"])
        .current_dir(dir)
        .output();
    let cmd_str = "terraform plan -input=false";
    let start = Instant::now();
    let output = Command::new("terraform")
        .args(["plan", "-input=false"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("IaC/Terraform", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_terraform_output(&raw);
            BuildResult {
                ok,
                project_type: "IaC/Terraform".into(),
                command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

fn build_docker_compose(dir: &Path) -> BuildResult {
    let cmd_str = "docker compose build";
    let start = Instant::now();
    let output = Command::new("docker")
        .args(["compose", "build"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("IaC/Docker", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_docker_output(&raw);
            BuildResult {
                ok,
                project_type: "IaC/Docker".into(),
                command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

fn build_dockerfile(dir: &Path) -> BuildResult {
    let tag = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "raios-build".into());
    let cmd_str = format!("docker build -t {} .", tag);
    let start = Instant::now();
    let output = Command::new("docker")
        .args(["build", "-t", &tag, "."])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("IaC/Dockerfile", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_docker_output(&raw);
            BuildResult {
                ok,
                project_type: "IaC/Dockerfile".into(),
                command: cmd_str,
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

pub fn test_iac(dir: &Path) -> TestResult {
    match detect_iac_kind(dir) {
        Some(IacKind::Terraform) => {
            let _ = Command::new("terraform")
                .args(["init", "-input=false"])
                .current_dir(dir)
                .output();
            let cmd_str = "terraform validate";
            let start = Instant::now();
            let out = Command::new("terraform")
                .arg("validate")
                .current_dir(dir)
                .output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("IaC/Terraform", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    let (ok, failed) = parse_terraform_validate_output(&raw);
                    TestResult {
                        ok,
                        project_type: "IaC/Terraform".into(),
                        command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64,
                        passed: if ok { 1 } else { 0 },
                        failed,
                        ignored: 0,
                        failures: raw
                            .lines()
                            .filter(|l| l.trim_start().starts_with("Error:"))
                            .map(|l| l.to_string())
                            .collect(),
                        raw_output: raw,
                    }
                }
            }
        }
        Some(IacKind::DockerCompose) => {
            let cmd_str = "docker compose config";
            let start = Instant::now();
            let out = Command::new("docker")
                .args(["compose", "config"])
                .current_dir(dir)
                .output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("IaC/Docker", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    let ok = o.status.success();
                    TestResult {
                        ok,
                        project_type: "IaC/Docker".into(),
                        command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64,
                        passed: if ok { 1 } else { 0 },
                        failed: if ok { 0 } else { 1 },
                        ignored: 0,
                        failures: if !ok { vec![raw.clone()] } else { vec![] },
                        raw_output: raw,
                    }
                }
            }
        }
        Some(IacKind::Dockerfile) | None => {
            let cmd_str = "docker build --check .";
            let start = Instant::now();
            let out = Command::new("docker")
                .args(["build", "--check", "."])
                .current_dir(dir)
                .output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("IaC/Dockerfile", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    let ok = o.status.success();
                    TestResult {
                        ok,
                        project_type: "IaC/Dockerfile".into(),
                        command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64,
                        passed: if ok { 1 } else { 0 },
                        failed: if ok { 0 } else { 1 },
                        ignored: 0,
                        failures: vec![],
                        raw_output: raw,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_terraform_plan_success() {
        let output = "Plan: 3 to add, 1 to change, 0 to destroy.\nTerraform will perform the following actions:\n";
        let (ok, errors) = parse_terraform_output(output);
        assert!(ok);
        assert_eq!(errors, 0);
    }

    #[test]
    fn parse_terraform_plan_error() {
        let output = "Error: Invalid expression\n  on main.tf line 5, in resource \"aws_instance\" \"example\":\n    5:   ami = invalid_var\n";
        let (ok, errors) = parse_terraform_output(output);
        assert!(!ok);
        assert!(errors >= 1);
    }

    #[test]
    fn parse_docker_compose_build_success() {
        let output = " => exporting to image\n => => writing image sha256:abc123\nSuccessfully built abc123\n";
        let (ok, _) = parse_docker_output(output);
        assert!(ok);
    }

    #[test]
    fn parse_terraform_validate_success() {
        let output = "Success! The configuration is valid.\n";
        let (ok, errors) = parse_terraform_validate_output(output);
        assert!(ok);
        assert_eq!(errors, 0);
    }

    #[test]
    fn parse_terraform_validate_error() {
        let output = "Error: Reference to undeclared resource\n  on main.tf line 12:\n";
        let (ok, errors) = parse_terraform_validate_output(output);
        assert!(!ok);
        assert!(errors >= 1);
    }

    #[test]
    fn detect_iac_kind_terraform() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.tf"), "terraform {}\n").unwrap();
        assert_eq!(detect_iac_kind(tmp.path()), Some(IacKind::Terraform));
    }

    #[test]
    fn detect_iac_kind_docker_compose() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("docker-compose.yml"), "version: '3'\n").unwrap();
        assert_eq!(detect_iac_kind(tmp.path()), Some(IacKind::DockerCompose));
    }

    #[test]
    fn detect_iac_kind_dockerfile() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Dockerfile"), "FROM ubuntu\n").unwrap();
        assert_eq!(detect_iac_kind(tmp.path()), Some(IacKind::Dockerfile));
    }

    #[test]
    fn terraform_beats_dockerfile() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.tf"), "terraform {}\n").unwrap();
        std::fs::write(tmp.path().join("Dockerfile"), "FROM ubuntu\n").unwrap();
        assert_eq!(detect_iac_kind(tmp.path()), Some(IacKind::Terraform));
    }
}
