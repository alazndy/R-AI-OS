//! Control-plane route view rendering modules (Now, Work, Explore, Govern).

/// Explore route view for searching, traces, and daemon logs.
pub mod explore;
/// Govern route view for policy, audit ledger, and background job scheduling.
pub mod govern;
/// Now route view for immediate attention approvals, blockers, and active runs.
pub mod now;
/// Work route view for projects, tasks, artifacts, and Product Factory posture.
pub mod work;

use ratatui::{layout::Rect, Frame};

use crate::app::route::Route;
use crate::app::store::Store;

/// Renders the currently selected control-plane route view (Now, Work, Explore, or Govern).
pub fn render_route_view(f: &mut Frame, area: Rect, store: &Store) {
    match store.current_route {
        Route::Now => now::render_now_route(f, area, store),
        Route::Work => work::render_work_route(f, area, store),
        Route::Explore => explore::render_explore_route(f, area, store),
        Route::Govern => govern::render_govern_route(f, area, store),
    }
}

#[cfg(test)]
mod tests;
