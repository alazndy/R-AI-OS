pub mod explore;
pub mod govern;
pub mod now;
pub mod work;

use ratatui::{layout::Rect, Frame};

use crate::app::route::Route;
use crate::app::store::Store;

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
