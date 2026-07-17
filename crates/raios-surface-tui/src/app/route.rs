use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Route {
    #[default]
    Now,
    Work,
    Explore,
    Govern,
}

pub const COMPACT_HEADER_HEIGHT: u16 = 3;
pub const BANNER_HEADER_HEIGHT: u16 = 8;
pub const TABS_HEIGHT: u16 = 3;
pub const LAUNCHER_HEIGHT: u16 = 3;

pub fn dashboard_header_height(screen_height: u16) -> u16 {
    if screen_height >= 32 {
        BANNER_HEADER_HEIGHT
    } else {
        COMPACT_HEADER_HEIGHT
    }
}

impl Route {
    pub fn all() -> &'static [Route] {
        &[Route::Now, Route::Work, Route::Explore, Route::Govern]
    }

    pub fn title(&self) -> &'static str {
        match self {
            Route::Now => "NOW — Attention & Approvals",
            Route::Work => "WORK — Projects, Tasks & Runs",
            Route::Explore => "EXPLORE — Search, Traces & Logs",
            Route::Govern => "GOVERN — Policy, Audit & System",
        }
    }

    pub fn tab_label(&self) -> &'static str {
        match self {
            Route::Now => "NOW",
            Route::Work => "WORK",
            Route::Explore => "EXPLORE",
            Route::Govern => "GOVERN",
        }
    }

    pub fn tab_at_column(column: u16) -> Option<Self> {
        let mut start = 0u16;

        for (idx, route) in Self::all().iter().enumerate() {
            let tab_width = 2 + route.tab_label().len() as u16;
            if (start..start + tab_width).contains(&column) {
                return Some(*route);
            }
            start += tab_width + 3; // " | " divider
            debug_assert!(idx < Self::all().len());
        }

        None
    }

    pub fn next(&self) -> Self {
        match self {
            Route::Now => Route::Work,
            Route::Work => Route::Explore,
            Route::Explore => Route::Govern,
            Route::Govern => Route::Now,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Route::Now => Route::Govern,
            Route::Work => Route::Now,
            Route::Explore => Route::Work,
            Route::Govern => Route::Explore,
        }
    }

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Route::Now,
            1 => Route::Work,
            2 => Route::Explore,
            3 => Route::Govern,
            _ => Route::Now,
        }
    }

    pub fn to_index(&self) -> usize {
        match self {
            Route::Now => 0,
            Route::Work => 1,
            Route::Explore => 2,
            Route::Govern => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Route;

    #[test]
    fn route_tabs_are_plain_text_and_stable() {
        let labels: Vec<&str> = Route::all().iter().map(Route::tab_label).collect();
        assert_eq!(labels, vec!["NOW", "WORK", "EXPLORE", "GOVERN"]);
        assert!(labels.iter().all(|label| label.is_ascii()));
    }

    #[test]
    fn tab_hit_regions_follow_the_visible_tab_order() {
        assert_eq!(Route::tab_at_column(0), Some(Route::Now));
        assert_eq!(Route::tab_at_column(8), Some(Route::Work));
        assert_eq!(Route::tab_at_column(17), Some(Route::Explore));
        assert_eq!(Route::tab_at_column(29), Some(Route::Govern));
        assert_eq!(Route::tab_at_column(6), None);
    }
}
