use std::path::Path;

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

pub use common::{OutdatedDep, CveIssue, DepsReport};

pub fn check(dir: &Path) -> DepsReport {
    use crate::core::build::detect_type;
    use crate::core::build::ProjectType;

    match detect_type(dir) {
        ProjectType::Rust => rust::check_rust(dir),
        ProjectType::Node => node::check_node(dir),
        ProjectType::Python => python::check_python(dir),
        ProjectType::Go => go::check_go(dir),
        ProjectType::Flutter => flutter::check_flutter(dir),
        ProjectType::Ios => ios::check_ios(dir),
        ProjectType::Android => android::check_android(dir),
        ProjectType::Embedded => embedded::check_embedded(dir),
        ProjectType::Iac => iac::check_iac(dir),
        ProjectType::Unknown => {
            let mut r = DepsReport::empty("Unknown");
            r.tool_missing.push("Cannot detect project type".into());
            r
        }
    }
}

#[cfg(test)]
mod tests;
