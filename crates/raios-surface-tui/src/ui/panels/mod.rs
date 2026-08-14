//! Panel rendering modules for individual dashboard components.

/// Agents and tools inventory panel.
pub mod agents;
/// System constitution sections editor panel.
pub mod constitution;
/// Main dashboard layout assembly.
pub mod dashboard_main;
/// Installed extensions list panel.
pub mod extensions;
/// Git diff preview panel.
pub mod git_diff;
/// Top status header bar panel.
pub mod header;
/// Keyboard shortcuts and commands reference help panel.
pub mod help;
/// Inbox and handover messages panel.
pub mod inbox;
/// Live daemon logs replay panel.
pub mod logs;
/// Left navigation menu panel.
pub mod menu;
/// Security policy rules panel.
pub mod policies;
/// Recent projects panel.
pub mod recent;
/// Background job scheduler panel.
pub mod scheduler;
/// Quick system statistics summary panel.
pub mod stats;
/// Task list management panel.
pub mod tasks;
/// Activity timeline feed panel.
pub mod timeline;

pub use agents::*;
pub use constitution::*;
pub use dashboard_main::*;
pub use extensions::*;
pub use git_diff::*;
pub use header::*;
pub use help::*;
pub use inbox::*;
pub use logs::*;
pub use menu::*;
pub use policies::*;
pub use recent::*;
pub use scheduler::*;
pub use stats::*;
pub use tasks::*;
pub use timeline::*;
