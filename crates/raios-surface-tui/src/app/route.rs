use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Route {
    #[default]
    Now,
    Work,
    Explore,
    Govern,
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

    pub fn icon(&self) -> &'static str {
        match self {
            Route::Now => "⚡",
            Route::Work => "🎯",
            Route::Explore => "🔍",
            Route::Govern => "🛡️",
        }
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
