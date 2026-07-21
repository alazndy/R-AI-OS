use std::path::Path;

pub mod android;
pub mod common;
pub mod cpp;
pub mod dotnet;
pub mod embedded;
pub mod flutter;
pub mod go;
pub mod iac;
pub mod ios;
pub mod node;
pub mod python;
pub mod rust;

pub use common::{CveIssue, DepsReport, OutdatedDep};

pub fn check(dir: &Path) -> DepsReport {
    use raios_core::core::build::detect_type;
    use raios_core::core::build::ProjectType;

    match detect_type(dir) {
        ProjectType::Rust => rust::check_rust(dir),
        ProjectType::Node => node::check_node(dir),
        ProjectType::ReactNative => node::check_node(dir),
        ProjectType::Python => python::check_python(dir),
        ProjectType::Go => go::check_go(dir),
        ProjectType::Flutter => flutter::check_flutter(dir),
        ProjectType::Ios => ios::check_ios(dir),
        ProjectType::Android => android::check_android(dir),
        ProjectType::Embedded => embedded::check_embedded(dir),
        ProjectType::Iac => iac::check_iac(dir),
        ProjectType::DotNet => dotnet::check_dotnet(dir),
        ProjectType::Cpp => cpp::check_cpp(dir),
        ProjectType::Unknown => {
            let mut r = DepsReport::empty("Unknown");
            r.tool_missing.push("Cannot detect project type".into());
            r
        }
    }
}

#[cfg(test)]
mod tests;
