pub mod setup;
pub mod components;
pub mod health;
pub mod search;
pub mod mempalace;
pub mod projects;
pub mod filebrowser;
pub mod dashboard;

pub use setup::*;
pub use components::*;
pub use health::*;
pub use search::*;
pub use mempalace::*;
pub use projects::*;
pub use filebrowser::*;
pub use dashboard::*;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect, Alignment, Direction, Margin},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, Paragraph, Wrap, Cell, Row, Table, Clear},
};

use crate::app::{App, AppState, MENU_ITEMS, filtered_palette};
use crate::filebrowser::FileEntry;

// ─── Colour palette ──────────────────────────────────────────────────────────

const GREEN: Color = Color::Rgb(0, 255, 136);
const CYAN: Color = Color::Rgb(0, 220, 220);
const DIM: Color = Color::Rgb(80, 80, 80);
const MID: Color = Color::Rgb(170, 170, 170);
const AMBER: Color = Color::Rgb(255, 170, 0);
const RED: Color = Color::Rgb(255, 80, 80);
const PANEL_BG: Color = Color::Rgb(8, 12, 16);
const HEADER_BG: Color = Color::Rgb(0, 20, 12);

// ─── Spinner ─────────────────────────────────────────────────────────────────

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn spinner_char(tick: u64) -> char {
    SPINNER[(tick as usize) % SPINNER.len()]
}

// ─── Banner (6 lines) ────────────────────────────────────────────────────────

const BANNER: &str = "\
  ██████╗       █████╗ ██╗      ██████╗ ███████╗\n\
  ██╔══██╗     ██╔══██╗██║     ██╔═══██╗██╔════╝\n\
  ██████╔╝     ███████║██║     ██║   ██║███████╗\n\
  ██╔══██╗     ██╔══██║██║     ██║   ██║╚════██║\n\
  ██║  ██║     ██║  ██║██║     ╚██████╔╝███████║\n\
  ╚═╝  ╚═╝     ╚═╝  ╚═╝╚═╝      ╚═════╝ ╚══════╝";

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn render(frame: &mut Frame, app: &App) {
    match app.state {
        AppState::Booting        => render_boot(frame, app),
        AppState::Setup          => render_setup(frame, app),
        AppState::Search         => render_search(frame, app),
        AppState::Dashboard      => render_dashboard(frame, app),
        AppState::FileView       => render_file_view(frame, app),
        AppState::FileEdit       => render_file_edit(frame, app),
        AppState::ProjectDetail  => render_project_detail(frame, app),
        AppState::HealthView     => render_health_view(frame, app),
        AppState::MemPalaceView  => render_mempalace_view(frame, app),
        AppState::GraphReport    => render_graph_report(frame, app),
        AppState::GitDiffView    => render_git_diff_view(frame, app),
        AppState::HelpView       => render_dashboard(frame, app), // Just use dashboard for now as it's a menu item
    }
    // Overlays rendered after everything else
    if app.show_launcher {
        render_launcher_modal(frame, app);
    }
    if app.command_mode && !app.command_buf.is_empty() {
        render_command_palette(frame, app);
    }
    if app.file_changed_externally {
        render_file_changed_badge(frame, app);
    }
    if app.bouncing_alert {
        render_bouncing_alert(frame, app);
    }
}

// ─── Boot screen ─────────────────────────────────────────────────────────────



// ─── Setup / First-run ───────────────────────────────────────────────────────


// ─── Search Modal (Ctrl+P) ──────────────────────────────────────────────────


// ─── Dashboard ───────────────────────────────────────────────────────────────

















fn project_status_color(status: &str) -> Color {
    match status {
        "production" => GREEN,
        "active" => CYAN,
        "early" => AMBER,
        "legacy" => DIM,
        _ => MID,
    }
}





// ─── File viewer ─────────────────────────────────────────────────────────────


// ─── Syntax highlighting ──────────────────────────────────────────────────────

const PURPLE: Color = Color::Rgb(180, 100, 220);
const TEAL: Color   = Color::Rgb(100, 200, 200);
const OLIVE: Color  = Color::Rgb(100, 180, 100);

pub fn update_code_block_state(line: &str, ext: &str, in_block: &mut bool) {
    if ext == "md" {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            *in_block = !*in_block;
        }
    }
}

pub fn highlight_line<'a>(line: &'a str, in_block: &mut bool, ext: &str) -> Vec<Span<'a>> {
    match ext {
        "md" => {
            let t = line.trim();
            if t.starts_with("```") || t.starts_with("~~~") {
                *in_block = !*in_block;
                return vec![Span::styled(line, Style::new().fg(AMBER))];
            }
            if *in_block {
                return vec![Span::styled(line, Style::new().fg(AMBER))];
            }
            highlight_markdown(line)
        }
        "rs" => highlight_rust(line),
        "ts" | "tsx" | "js" | "jsx" => highlight_typescript(line),
        "py" => highlight_python(line),
        "toml" => highlight_toml(line),
        "yaml" | "yml" => highlight_yaml(line),
        _ => vec![Span::styled(line, Style::new().fg(MID))],
    }
}

fn highlight_markdown(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with("# ") || t == "#" {
        return vec![Span::styled(line, Style::new().fg(GREEN).bold())];
    }
    if t.starts_with("## ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    if t.starts_with("### ") {
        return vec![Span::styled(line, Style::new().fg(AMBER).bold())];
    }
    if t.starts_with("####") {
        return vec![Span::styled(line, Style::new().fg(MID).bold())];
    }
    if t.trim() == "---" || t.trim() == "===" || t.trim() == "___" {
        return vec![Span::styled(line, Style::new().fg(DIM))];
    }
    if t.starts_with("> ") {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with("- [x] ") || t.starts_with("- [X] ") {
        return vec![Span::styled(line, Style::new().fg(GREEN))];
    }
    if t.starts_with("- [ ] ") {
        return vec![Span::styled(line, Style::new().fg(AMBER))];
    }
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        let prefix_len = line.len() - t.len() + 2;
        return vec![
            Span::styled(&line[..prefix_len], Style::new().fg(GREEN)),
            Span::styled(&line[prefix_len..], Style::new().fg(MID)),
        ];
    }
    if t.starts_with('|') {
        return vec![Span::styled(line, Style::new().fg(CYAN))];
    }
    if t.starts_with("    ") || t.starts_with('\t') {
        return vec![Span::styled(line, Style::new().fg(AMBER))];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_rust(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with("///") || t.starts_with("//!") {
        return vec![Span::styled(line, Style::new().fg(GREEN).italic())];
    }
    if t.starts_with("//") {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with("#[") || t.starts_with("#![") {
        return vec![Span::styled(line, Style::new().fg(PURPLE))];
    }
    if t.contains("fn ") && (t.starts_with("fn ") || t.starts_with("pub ") || t.starts_with("async ")) {
        return vec![Span::styled(line, Style::new().fg(GREEN))];
    }
    if t.starts_with("pub struct ") || t.starts_with("struct ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    if t.starts_with("pub enum ") || t.starts_with("enum ") {
        return vec![Span::styled(line, Style::new().fg(CYAN))];
    }
    if t.starts_with("impl") {
        return vec![Span::styled(line, Style::new().fg(AMBER))];
    }
    if t.starts_with("pub trait ") || t.starts_with("trait ") {
        return vec![Span::styled(line, Style::new().fg(TEAL))];
    }
    if t.starts_with("use ") || t.starts_with("mod ") || t.starts_with("pub mod ") {
        return vec![Span::styled(line, Style::new().fg(DIM))];
    }
    if t.starts_with("type ") || t.starts_with("pub type ") || t.starts_with("const ") || t.starts_with("pub const ") || t.starts_with("static ") {
        return vec![Span::styled(line, Style::new().fg(OLIVE))];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_typescript(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with("//") {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with("import ") || t.starts_with("export {") || t.starts_with("export * ") {
        return vec![Span::styled(line, Style::new().fg(DIM))];
    }
    if t.starts_with("interface ") || t.starts_with("export interface ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    if t.starts_with("type ") || t.starts_with("export type ") {
        return vec![Span::styled(line, Style::new().fg(CYAN))];
    }
    if t.contains("function ") {
        return vec![Span::styled(line, Style::new().fg(GREEN))];
    }
    if t.starts_with("class ") || t.starts_with("export class ") || t.starts_with("abstract class ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_python(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with("def ") || t.starts_with("async def ") {
        return vec![Span::styled(line, Style::new().fg(GREEN))];
    }
    if t.starts_with("class ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    if t.starts_with("import ") || t.starts_with("from ") {
        return vec![Span::styled(line, Style::new().fg(DIM))];
    }
    if t.starts_with('@') {
        return vec![Span::styled(line, Style::new().fg(PURPLE))];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_toml(line: &str) -> Vec<Span<'_>> {
    let t = line.trim();
    if t.starts_with('#') {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with('[') && t.ends_with(']') {
        return vec![Span::styled(line, Style::new().fg(GREEN).bold())];
    }
    if let Some(pos) = line.find(" = ") {
        return vec![
            Span::styled(&line[..pos], Style::new().fg(CYAN)),
            Span::styled(&line[pos..], Style::new().fg(AMBER)),
        ];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_yaml(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if let Some(pos) = line.find(": ") {
        return vec![
            Span::styled(&line[..pos + 1], Style::new().fg(CYAN)),
            Span::styled(&line[pos + 1..], Style::new().fg(MID)),
        ];
    }
    if t.ends_with(':') {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

// ─── File editor ─────────────────────────────────────────────────────────────


// ─── Helpers ─────────────────────────────────────────────────────────────────

// ─── MemPalace full-screen view ───────────────────────────────────────────────


fn build_preview(app: &App) -> Vec<Line<'static>> {
    let room = match app.mp_rooms.get(app.mp_room_cursor) {
        Some(r) => r,
        None => return vec![Line::from(Span::styled("  No rooms loaded", Style::new().fg(DIM)))],
    };

    // If on a specific project, show its status
    if let Some(pi) = app.mp_proj_cursor {
        if let Some(proj) = room.projects.get(pi) {
            let mut lines = vec![
                Line::from(Span::styled(proj.name.clone(), Style::new().fg(GREEN).bold())),
                Line::from(Span::styled(
                    format!("  {}", room.folder_name),
                    Style::new().fg(DIM),
                )),
                Line::from(""),
            ];

            let mem_icon = if proj.has_memory { "✓ memory.md" } else { "✗ no memory.md" };
            let mem_color = if proj.has_memory { GREEN } else { RED };
            lines.push(Line::from(Span::styled(mem_icon, Style::new().fg(mem_color))));

            if proj.date != "—" {
                lines.push(Line::from(Span::styled(
                    format!("  Last update: {}", proj.date),
                    Style::new().fg(DIM),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Status:", Style::new().fg(DIM))));

            let status = proj.status.clone();
            let truncated: String = status.chars().take(120).collect();
            for chunk in truncated.chars().collect::<Vec<_>>().chunks(55) {
                let s: String = chunk.iter().collect();
                lines.push(Line::from(Span::styled(
                    format!("  {}", s),
                    Style::new().fg(MID),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  [Enter] open detail",
                Style::new().fg(DIM).italic(),
            )));

            return lines;
        }
    }

    // Room-level preview: list all projects
    let mut lines = vec![
        Line::from(vec![
            Span::styled(room.icon, Style::new()),
            Span::styled(format!("  {}", room.folder_name), Style::new().fg(CYAN).bold()),
        ]),
        Line::from(Span::styled(
            format!("  {} projects", room.projects.len()),
            Style::new().fg(DIM),
        )),
        Line::from(""),
    ];

    let with_mem = room.projects.iter().filter(|p| p.has_memory).count();
    lines.push(Line::from(Span::styled(
        format!("  ✓ memory.md: {}/{}", with_mem, room.projects.len()),
        Style::new().fg(if with_mem == room.projects.len() { GREEN } else { AMBER }),
    )));
    lines.push(Line::from(""));

    // Latest projects
    lines.push(Line::from(Span::styled("  Recent:", Style::new().fg(DIM))));
    for proj in room.projects.iter().take(6) {
        let color = if proj.has_memory { MID } else { DIM };
        lines.push(Line::from(Span::styled(
            format!("  · {}", proj.name),
            Style::new().fg(color),
        )));
    }

    lines
}

// ─── Health Dashboard ─────────────────────────────────────────────────────────


// ─── Command palette overlay ──────────────────────────────────────────────────


// ─── File-changed notification badge ─────────────────────────────────────────


// ─── Agent launcher modal ─────────────────────────────────────────────────────





