use crate::app::route::{dashboard_header_height, LAUNCHER_HEIGHT, TABS_HEIGHT};
use crate::app::App;
use crate::ui::routes::render_route_view;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout},
    Frame,
};

pub fn render_dashboard(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let header_height = dashboard_header_height(area.height);

    let [header_area, tabs_area, content_area, launcher_area] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Length(TABS_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(LAUNCHER_HEIGHT),
    ])
    .areas(area);

    render_header(frame, header_area, app);
    render_menu(frame, tabs_area, app);
    render_route_view(frame, content_area, &app.store);
    render_launcher(frame, launcher_area, app);
}
