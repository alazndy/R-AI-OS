use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout},
    Frame,
};

pub fn render_dashboard(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let [header_area, main_area, launcher_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .areas(area);

    let [menu_area, content_area] =
        Layout::horizontal([Constraint::Length(28), Constraint::Min(0)]).areas(main_area);

    render_header(frame, header_area, app);
    render_menu(frame, menu_area, app);
    render_content(frame, content_area, app);
    render_launcher(frame, launcher_area, app);
}
