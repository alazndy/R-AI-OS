use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::app::route::Route;
use crate::app::store::Store;
use crate::ui::routes::render_route_view;

fn get_rendered_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<&str>>()
        .join("")
}

#[test]
fn golden_render_now_route() {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut store = Store::new();
    store.current_route = Route::Now;

    terminal
        .draw(|f| {
            render_route_view(f, f.area(), &store);
        })
        .unwrap();

    let rendered = get_rendered_text(&terminal);
    assert!(
        rendered.contains("Approvals")
            || rendered.contains("Blockers")
            || rendered.contains("Active"),
        "Now route output missing expected titles"
    );
}

#[test]
fn golden_render_work_route() {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut store = Store::new();
    store.current_route = Route::Work;

    terminal
        .draw(|f| {
            render_route_view(f, f.area(), &store);
        })
        .unwrap();

    let rendered = get_rendered_text(&terminal);
    assert!(
        rendered.contains("Projects")
            || rendered.contains("Tasks")
            || rendered.contains("Artifacts"),
        "Work route output missing expected titles"
    );
}

#[test]
fn golden_render_explore_route() {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut store = Store::new();
    store.current_route = Route::Explore;

    terminal
        .draw(|f| {
            render_route_view(f, f.area(), &store);
        })
        .unwrap();

    let rendered = get_rendered_text(&terminal);
    assert!(
        rendered.contains("Search") || rendered.contains("Traces") || rendered.contains("Logs"),
        "Explore route output missing expected titles"
    );
}

#[test]
fn golden_render_govern_route() {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut store = Store::new();
    store.current_route = Route::Govern;

    terminal
        .draw(|f| {
            render_route_view(f, f.area(), &store);
        })
        .unwrap();

    let rendered = get_rendered_text(&terminal);
    assert!(
        rendered.contains("Policy") || rendered.contains("Audit") || rendered.contains("Cron"),
        "Govern route output missing expected titles"
    );
}
