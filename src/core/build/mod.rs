use std::path::Path;
use serde::{Deserialize, Serialize};

pub mod common;
pub mod rust;
pub mod node;
pub mod python;
pub mod go;
pub mod flutter;
pub mod ios;
pub mod android;
pub mod embedded;
pub mod iac;
pub mod dotnet;
pub mod cpp;

// Re-exports
pub use common::{BuildDiagnostic, BuildResult, TestResult};
pub use flutter::{build_flutter, build_flutter_release, build_flutter_check, test_flutter};
pub use ios::{build_ios, build_ios_release, build_ios_check, test_ios};
pub use android::{build_android, build_android_release, build_android_check, test_android_unit, test_android_instrumented};
pub use embedded::{build_embedded, test_embedded, detect_embedded_kind, EmbeddedKind};
pub use iac::{build_iac, test_iac, detect_iac_kind, IacKind};
pub use dotnet::{build_dotnet, test_dotnet};
pub use cpp::{build_cpp, test_cpp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Flutter,
    Ios,
    Android,
    Embedded,
    Iac,
    DotNet,
    Cpp,
    Unknown,
}

impl ProjectType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Node => "Node",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Flutter => "Flutter",
            Self::Ios => "iOS",
            Self::Android => "Android",
            Self::Embedded => "Embedded",
            Self::Iac => "IaC",
            Self::DotNet => ".NET",
            Self::Cpp => "C++",
            Self::Unknown => "Unknown",
        }
    }
}

pub fn detect_type(dir: &Path) -> ProjectType {
    if dir.join("Cargo.toml").exists() {
        return ProjectType::Rust;
    }
    if dir.join("package.json").exists() {
        return ProjectType::Node;
    }
    if dir.join("pyproject.toml").exists()
        || dir.join("setup.py").exists()
        || dir.join("requirements.txt").exists()
    {
        return ProjectType::Python;
    }
    if dir.join("go.mod").exists() {
        return ProjectType::Go;
    }
    if dir.join("pubspec.yaml").exists() {
        return ProjectType::Flutter;
    }
    if dir.join("Package.swift").exists()
        || std::fs::read_dir(dir).ok().is_some_and(|entries| {
            entries.flatten().any(|e| {
                matches!(
                    e.path().extension().and_then(|s| s.to_str()),
                    Some("xcodeproj" | "xcworkspace")
                )
            })
        })
    {
        return ProjectType::Ios;
    }
    if (dir.join("gradlew").exists() || dir.join("gradlew.bat").exists())
        && (dir.join("build.gradle").exists() || dir.join("settings.gradle").exists())
    {
        return ProjectType::Android;
    }
    if embedded::detect_embedded_kind(dir).is_some() {
        return ProjectType::Embedded;
    }
    if iac::detect_iac_kind(dir).is_some() {
        return ProjectType::Iac;
    }
    if std::fs::read_dir(dir).ok().is_some_and(|entries| {
        entries.flatten().any(|e| {
            matches!(
                e.path().extension().and_then(|s| s.to_str()),
                Some("csproj" | "sln")
            )
        })
    }) {
        return ProjectType::DotNet;
    }
    if dir.join("CMakeLists.txt").exists() {
        return ProjectType::Cpp;
    }
    ProjectType::Unknown
}

pub fn build(dir: &Path) -> BuildResult {
    match detect_type(dir) {
        ProjectType::Rust => rust::build_rust(dir),
        ProjectType::Node => node::build_node(dir),
        ProjectType::Python => python::build_python(dir),
        ProjectType::Go => go::build_go(dir),
        ProjectType::Flutter => build_flutter(dir),
        ProjectType::Ios => build_ios(dir),
        ProjectType::Android => build_android(dir),
        ProjectType::Embedded => build_embedded(dir),
        ProjectType::Iac => build_iac(dir),
        ProjectType::DotNet => build_dotnet(dir),
        ProjectType::Cpp => build_cpp(dir),
        ProjectType::Unknown => BuildResult {
            ok: false,
            project_type: "Unknown".into(),
            command: "—".into(),
            duration_ms: 0,
            warnings: 0,
            errors: 1,
            diagnostics: vec![],
            raw_output:
                "Cannot detect project type (no Cargo.toml, package.json, go.mod, pyproject.toml, gradlew)"
                    .into(),
        },
    }
}

pub fn test(dir: &Path) -> TestResult {
    match detect_type(dir) {
        ProjectType::Rust => rust::test_rust(dir),
        ProjectType::Node => node::test_node(dir),
        ProjectType::Python => python::test_python(dir),
        ProjectType::Go => go::test_go(dir),
        ProjectType::Flutter => test_flutter(dir),
        ProjectType::Ios => test_ios(dir),
        ProjectType::Android => android::test_android_unit(dir),
        ProjectType::Embedded => test_embedded(dir),
        ProjectType::Iac => test_iac(dir),
        ProjectType::DotNet => test_dotnet(dir),
        ProjectType::Cpp => test_cpp(dir),
        ProjectType::Unknown => TestResult {
            ok: false,
            project_type: "Unknown".into(),
            command: "—".into(),
            duration_ms: 0,
            passed: 0,
            failed: 0,
            ignored: 0,
            failures: vec!["Cannot detect project type".into()],
            raw_output: String::new(),
        },
    }
}

#[cfg(test)]
mod tests;

