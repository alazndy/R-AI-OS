//! The custom element that paints a [`PaintList`] with quads and shaped text runs (decision D2).

use std::rc::Rc;
use std::time::Instant;

use gpui::{
    fill, point, px, relative, size, App, Bounds, Element, ElementId, ElementInputHandler,
    FocusHandle, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, Pixels,
    SharedString, Size, Style, TextAlign, TextRun, UnderlineStyle, Window,
};
use rwk_core::terminal::{build_paint_list, CursorShape, Rgb, TextItem};

use super::metrics::grid_for;
use super::view::{TerminalView, ViewShared};

/// Font size in pixels for the first slice (configurable fonts are P3).
const FONT_SIZE: f32 = 14.0;
/// Line height in pixels.
const LINE_HEIGHT: f32 = 18.0;
/// Painted frames slower than this many microseconds count as slow.
const SLOW_FRAME_MICROS: u64 = 33_000;
/// Thickness of underline and beam cursors.
const CURSOR_LINE_PX: f32 = 2.0;

/// Paints one terminal session.
pub struct TerminalElement {
    shared: Rc<ViewShared>,
    view: gpui::Entity<TerminalView>,
    focus: FocusHandle,
}

impl TerminalElement {
    pub(crate) fn new(
        shared: Rc<ViewShared>,
        view: gpui::Entity<TerminalView>,
        focus: FocusHandle,
    ) -> Self {
        Self {
            shared,
            view,
            focus,
        }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

fn hsla(color: Rgb, alpha: f32) -> Hsla {
    let hex = (u32::from(color.0) << 16) | (u32::from(color.1) << 8) | u32::from(color.2);
    let mut value: Hsla = gpui::rgb(hex).into();
    value.a = alpha;
    value
}

fn text_run(item: &TextItem, base: &gpui::Font) -> TextRun {
    let mut font = base.clone();
    if item.bold {
        font = font.bold();
    }
    if item.italic {
        font = font.italic();
    }
    let color = hsla(item.color, 1.0);
    TextRun {
        len: item.text.len(),
        font,
        color,
        background_color: None,
        underline: item.underline.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: item.strikeout.then_some(gpui::StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        }),
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: Size {
                width: relative(1.0).into(),
                height: relative(1.0).into(),
            },
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let started = Instant::now();
        let shared = &self.shared;
        shared.bounds.set(bounds);
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        let font_size = px(FONT_SIZE);
        let line_height = px(LINE_HEIGHT);
        let base_font = gpui::font(crate::fonts::monospace_family(window));

        // Cell metrics: the advance of "M" in the base font.
        let font_id = window.text_system().resolve_font(&base_font);
        let cell_width = window
            .text_system()
            .advance(font_id, font_size, 'M')
            .map(|advance| f32::from(advance.width))
            .unwrap_or(FONT_SIZE * 0.6);
        let cell_height = LINE_HEIGHT;
        if shared.cell.get() != (cell_width, cell_height) {
            shared.cell.set((cell_width, cell_height));
            shared
                .engine
                .set_cell_pixels(cell_width.round() as u16, cell_height.round() as u16);
        }

        // A resize reaches the engine and the PTY only when the whole-cell grid changed.
        let grid = grid_for(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            cell_width,
            cell_height,
        );
        if grid != shared.grid.get() {
            let mut stats = shared.stats.get();
            if shared.engine.resize(grid).is_err()
                || shared.host.resize(&shared.session, grid).is_err()
            {
                stats.resize_errors += 1;
                shared.stats.set(stats);
            }
            shared.grid.set(grid);
        }

        // Cleared before the snapshot, so output arriving during the copy re-arms the wake-up.
        shared.gate.take();
        let snapshot_started = Instant::now();
        let snapshot = shared.engine.snapshot();
        let snapshot_micros = snapshot_started.elapsed().as_micros() as u64;
        let palette = shared.appearance.get().palette();
        let list = build_paint_list(&snapshot, &palette);
        shared.modes.set(snapshot.modes);
        shared
            .cursor
            .set(snapshot.cursor.map(|cursor| (cursor.row, cursor.col)));
        let screen_text = snapshot.screen_text();
        let screen_changed = *shared.screen_text.borrow() != screen_text;
        *shared.screen_text.borrow_mut() = screen_text;

        window.paint_quad(fill(bounds, hsla(palette.background, 1.0)));
        let cell_origin = |row: u16, col: u16| {
            bounds.origin
                + point(
                    px(f32::from(col) * cell_width),
                    px(f32::from(row) * cell_height),
                )
        };
        for rect in &list.backgrounds {
            let area = Bounds::new(
                cell_origin(rect.row, rect.col),
                size(px(f32::from(rect.cells) * cell_width), px(cell_height)),
            );
            window.paint_quad(fill(area, hsla(rect.color, 1.0)));
        }
        for span in &list.selection {
            let cells = span.end_col.saturating_sub(span.start_col) + 1;
            let area = Bounds::new(
                cell_origin(span.row, span.start_col),
                size(px(f32::from(cells) * cell_width), px(cell_height)),
            );
            window.paint_quad(fill(area, hsla(palette.selection, 0.6)));
        }
        for item in &list.texts {
            let run = text_run(item, &base_font);
            let shaped = window.text_system().shape_line(
                SharedString::from(item.text.clone()),
                font_size,
                &[run],
                None,
            );
            // A paint failure of one run must not abort the frame; the next frame retries.
            let _ = shaped.paint(
                cell_origin(item.row, item.col),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        if let Some(cursor) = list.cursor {
            let origin = cell_origin(cursor.row, cursor.col);
            let area = match cursor.shape {
                CursorShape::Block => Bounds::new(origin, size(px(cell_width), px(cell_height))),
                CursorShape::Underline => Bounds::new(
                    origin + point(px(0.0), px(cell_height - CURSOR_LINE_PX)),
                    size(px(cell_width), px(CURSOR_LINE_PX)),
                ),
                CursorShape::Beam => Bounds::new(origin, size(px(CURSOR_LINE_PX), px(cell_height))),
            };
            window.paint_quad(fill(area, hsla(cursor.color, 0.9)));
        }

        let paint_micros = started.elapsed().as_micros() as u64;
        let mut stats = shared.stats.get();
        stats.painted_frames += 1;
        stats.last_paint_micros = paint_micros;
        stats.last_snapshot_micros = snapshot_micros;
        if screen_changed {
            stats.last_change_at = Some(Instant::now());
        }
        if paint_micros > SLOW_FRAME_MICROS {
            stats.slow_frames += 1;
        }
        shared.stats.set(stats);
    }
}
