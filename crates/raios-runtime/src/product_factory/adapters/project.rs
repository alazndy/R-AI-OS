use raios_core::product_factory::{
    FactoryInvariantError, FlutterCapabilities, ProjectInspection, ProjectRuntimeKind,
    ReactNativeCapabilities, ReactNativeWorkflow, RustCapabilities, WebCapabilities,
};
use std::path::Path;

pub trait ProjectAdapter {
    fn inspect_project(
        &self,
        project_ref: &str,
    ) -> Result<ProjectInspection, FactoryInvariantError>;
}

/// Local, read-only project detector. It never executes package scripts, reads
/// secrets, downloads dependencies, or infers signing readiness from credential files.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalProjectDetector;

impl LocalProjectDetector {
    pub fn inspect(project_ref: &str) -> Result<ProjectInspection, FactoryInvariantError> {
        let root = Path::new(project_ref);
        if !root.exists() {
            return Err(FactoryInvariantError::RepositoryNotInstalled);
        }

        // 1. Rust project
        if root.join("Cargo.toml").is_file() {
            let content = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
            let is_workspace = content.contains("[workspace]");
            let edition = if content.contains("edition = \"2024\"") {
                "2024".into()
            } else if content.contains("edition = \"2018\"") {
                "2018".into()
            } else {
                "2021".into()
            };
            let mut targets = Vec::new();
            if root.join("src").join("main.rs").is_file() || root.join("src").join("bin").is_dir() {
                targets.push("bin".into());
            }
            if root.join("src").join("lib.rs").is_file() {
                targets.push("lib".into());
            }

            return Ok(ProjectInspection {
                project_ref: project_ref.into(),
                runtime_kind: Some(ProjectRuntimeKind::Rust),
                react_native: None,
                flutter: None,
                web: None,
                rust: Some(RustCapabilities {
                    has_cargo_toml: true,
                    cargo_toolchain: which::which("cargo").is_ok(),
                    edition,
                    is_workspace,
                    has_clippy: which::which("cargo-clippy").is_ok()
                        || which::which("cargo").is_ok(),
                    targets,
                }),
            });
        }

        // 2. Flutter project
        if root.join("pubspec.yaml").is_file() {
            return Ok(ProjectInspection {
                project_ref: project_ref.into(),
                runtime_kind: Some(ProjectRuntimeKind::Flutter),
                react_native: None,
                flutter: Some(FlutterCapabilities {
                    has_pubspec: true,
                    flutter_toolchain: which::which("flutter").is_ok(),
                    dart_toolchain: which::which("dart").is_ok(),
                    has_android_project: root.join("android").is_dir(),
                    has_ios_project: root.join("ios").is_dir(),
                    has_web_project: root.join("web").is_dir(),
                    signing_readiness: "not_assessed".into(),
                }),
                web: None,
                rust: None,
            });
        }

        // 3. Node-based projects (React Native / Expo vs Web)
        let package_path = root.join("package.json");
        if package_path.is_file() {
            let manifest = std::fs::read_to_string(&package_path)
                .map_err(|_| FactoryInvariantError::RepositoryNotInstalled)?;
            let package: serde_json::Value = serde_json::from_str(&manifest)
                .map_err(|_| FactoryInvariantError::RepositoryNotInstalled)?;

            let has_dependency = |name: &str| {
                ["dependencies", "devDependencies", "peerDependencies"]
                    .iter()
                    .any(|section| {
                        package
                            .get(section)
                            .and_then(|value| value.get(name))
                            .is_some()
                    })
            };

            let has_expo = has_dependency("expo");
            let has_react_native = has_dependency("react-native");

            let package_manager = if root.join("pnpm-lock.yaml").is_file() {
                "pnpm"
            } else if root.join("yarn.lock").is_file() {
                "yarn"
            } else if root.join("bun.lockb").is_file() || root.join("bun.lock").is_file() {
                "bun"
            } else if root.join("package-lock.json").is_file() {
                "npm"
            } else {
                "unknown"
            };

            if has_expo || has_react_native {
                let has_android_project = root.join("android").is_dir();
                let has_ios_project = root.join("ios").is_dir();
                let workflow = match (has_expo, has_android_project || has_ios_project) {
                    (true, false) => ReactNativeWorkflow::ExpoManaged,
                    (true, true) => ReactNativeWorkflow::ExpoPrebuild,
                    (false, true) => ReactNativeWorkflow::Bare,
                    (false, false) => ReactNativeWorkflow::Bare,
                };
                let eas_configured = root.join("eas.json").is_file();

                return Ok(ProjectInspection {
                    project_ref: project_ref.into(),
                    runtime_kind: Some(ProjectRuntimeKind::ReactNative),
                    react_native: Some(ReactNativeCapabilities {
                        workflow,
                        has_android_project,
                        has_ios_project,
                        package_manager: package_manager.into(),
                        typescript: root.join("tsconfig.json").is_file(),
                        eas_configured,
                        local_android_toolchain: which::which("adb").is_ok(),
                        local_macos_capability: cfg!(target_os = "macos"),
                        signing_readiness: "not_assessed".into(),
                    }),
                    flutter: None,
                    web: None,
                    rust: None,
                });
            }

            // Web project
            let framework = if has_dependency("next") {
                "Next.js"
            } else if has_dependency("vite") {
                "Vite"
            } else if has_dependency("react") {
                "React"
            } else if has_dependency("vue") {
                "Vue"
            } else if has_dependency("svelte") {
                "Svelte"
            } else {
                "Node.js"
            };

            let has_build_script = package
                .get("scripts")
                .and_then(|scripts| scripts.get("build"))
                .is_some();

            return Ok(ProjectInspection {
                project_ref: project_ref.into(),
                runtime_kind: Some(ProjectRuntimeKind::Web),
                react_native: None,
                flutter: None,
                web: Some(WebCapabilities {
                    framework: framework.into(),
                    package_manager: package_manager.into(),
                    typescript: root.join("tsconfig.json").is_file(),
                    has_build_script,
                }),
                rust: None,
            });
        }

        Ok(ProjectInspection {
            project_ref: project_ref.into(),
            runtime_kind: None,
            react_native: None,
            flutter: None,
            web: None,
            rust: None,
        })
    }
}

impl ProjectAdapter for LocalProjectDetector {
    fn inspect_project(
        &self,
        project_ref: &str,
    ) -> Result<ProjectInspection, FactoryInvariantError> {
        Self::inspect(project_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(root: &Path, package: &str) {
        std::fs::write(root.join("package.json"), package).unwrap();
    }

    #[test]
    fn detects_expo_managed_typescript_project_before_generic_node() {
        let temp = tempfile::tempdir().unwrap();
        write_manifest(
            temp.path(),
            r#"{"dependencies":{"expo":"~53.0.0","react-native":"0.79.0"}}"#,
        );
        std::fs::write(temp.path().join("package-lock.json"), "{}").unwrap();
        std::fs::write(temp.path().join("tsconfig.json"), "{}").unwrap();
        std::fs::write(temp.path().join("eas.json"), "{}").unwrap();

        let inspection = LocalProjectDetector::inspect(temp.path().to_str().unwrap()).unwrap();
        assert_eq!(
            inspection.runtime_kind,
            Some(ProjectRuntimeKind::ReactNative)
        );
        let capabilities = inspection.react_native.unwrap();
        assert_eq!(capabilities.workflow, ReactNativeWorkflow::ExpoManaged);
        assert_eq!(capabilities.package_manager, "npm");
        assert!(capabilities.typescript);
        assert!(capabilities.eas_configured);
        assert!(!capabilities.has_android_project);
        assert!(!capabilities.has_ios_project);
        assert_eq!(capabilities.signing_readiness, "not_assessed");
    }

    #[test]
    fn distinguishes_expo_prebuild_and_bare_projects() {
        let expo = tempfile::tempdir().unwrap();
        write_manifest(expo.path(), r#"{"dependencies":{"expo":"~53.0.0"}}"#);
        std::fs::create_dir(expo.path().join("android")).unwrap();
        assert_eq!(
            LocalProjectDetector::inspect(expo.path().to_str().unwrap())
                .unwrap()
                .react_native
                .unwrap()
                .workflow,
            ReactNativeWorkflow::ExpoPrebuild
        );

        let bare = tempfile::tempdir().unwrap();
        write_manifest(bare.path(), r#"{"dependencies":{"react-native":"0.79.0"}}"#);
        std::fs::create_dir(bare.path().join("ios")).unwrap();
        assert_eq!(
            LocalProjectDetector::inspect(bare.path().to_str().unwrap())
                .unwrap()
                .react_native
                .unwrap()
                .workflow,
            ReactNativeWorkflow::Bare
        );
    }

    #[test]
    fn detects_rust_project_adapter() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src").join("main.rs"), "fn main() {}").unwrap();

        let inspection = LocalProjectDetector::inspect(temp.path().to_str().unwrap()).unwrap();
        assert_eq!(inspection.runtime_kind, Some(ProjectRuntimeKind::Rust));
        let rust = inspection.rust.unwrap();
        assert!(rust.has_cargo_toml);
        assert_eq!(rust.edition, "2021");
        assert_eq!(rust.targets, vec!["bin"]);
    }

    #[test]
    fn detects_flutter_project_adapter() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("pubspec.yaml"), "name: flutter_demo\n").unwrap();
        std::fs::create_dir(temp.path().join("android")).unwrap();
        std::fs::create_dir(temp.path().join("ios")).unwrap();

        let inspection = LocalProjectDetector::inspect(temp.path().to_str().unwrap()).unwrap();
        assert_eq!(inspection.runtime_kind, Some(ProjectRuntimeKind::Flutter));
        let flutter = inspection.flutter.unwrap();
        assert!(flutter.has_pubspec);
        assert!(flutter.has_android_project);
        assert!(flutter.has_ios_project);
    }

    #[test]
    fn detects_web_project_adapter() {
        let temp = tempfile::tempdir().unwrap();
        write_manifest(
            temp.path(),
            r#"{"dependencies":{"next":"14.0.0","react":"18.0.0"},"scripts":{"build":"next build"}}"#,
        );
        std::fs::write(temp.path().join("tsconfig.json"), "{}").unwrap();
        std::fs::write(temp.path().join("pnpm-lock.yaml"), "").unwrap();

        let inspection = LocalProjectDetector::inspect(temp.path().to_str().unwrap()).unwrap();
        assert_eq!(inspection.runtime_kind, Some(ProjectRuntimeKind::Web));
        let web = inspection.web.unwrap();
        assert_eq!(web.framework, "Next.js");
        assert_eq!(web.package_manager, "pnpm");
        assert!(web.typescript);
        assert!(web.has_build_script);
    }
}
