//! The terminal view entity: owns one session's engine handle, focus, palette and frame stats.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use std::ops::Range;

use gpui::{
    div, px, Bounds, ClipboardItem, Context, EntityInputHandler, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton as GpuiButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollWheelEvent, SharedString, Styled,
    Task, UTF16Selection, Window,
};
use rwk_core::sessions::{SessionId, TerminalHost, TerminalSize};
use rwk_core::terminal::{
    encode_key, encode_mouse, CellSide, Modes, MouseButton, MouseKind, MouseReport, Palette, Rgb,
    ScrollRequest, SelectionKind, Surface, TerminalEngine,
};

use super::element::TerminalElement;
use super::gate::FrameGate;
use super::input::{
    chord_from_keystroke, decide_paste, input_failure_message, is_reserved, PasteDecision,
};

/// Lines moved by one wheel notch.
const WHEEL_LINES: i32 = 3;

/// Surface of the terminal: a plain dark or light tone, or colours taken from the active theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    /// Light text on a dark surface.
    Dark,
    /// Dark text on a light surface.
    Light,
    /// Text and background colours of the active skin; `dark` picks the ANSI palette family.
    Themed {
        /// True for the dark ANSI palette.
        dark: bool,
        /// Default text colour, 0xRRGGBB.
        foreground: u32,
        /// Surface colour, 0xRRGGBB.
        background: u32,
        /// Selection colour, 0xRRGGBB.
        selection: u32,
    },
}

impl Appearance {
    /// The palette of this tone.
    pub const fn palette(self) -> Palette {
        match self {
            Appearance::Dark => Palette::dark(Surface {
                foreground: Rgb::from_hex(0xd4d4d4),
                background: Rgb::from_hex(0x1e1e1e),
                cursor: Rgb::from_hex(0xffffff),
                selection: Rgb::from_hex(0x264f78),
            }),
            Appearance::Light => Palette::light(Surface {
                foreground: Rgb::from_hex(0x1f2328),
                background: Rgb::from_hex(0xffffff),
                cursor: Rgb::from_hex(0x000000),
                selection: Rgb::from_hex(0xadd6ff),
            }),
            Appearance::Themed {
                dark,
                foreground,
                background,
                selection,
            } => {
                let surface = Surface {
                    foreground: Rgb::from_hex(foreground),
                    background: Rgb::from_hex(background),
                    cursor: Rgb::from_hex(foreground),
                    selection: Rgb::from_hex(selection),
                };
                if dark {
                    Palette::dark(surface)
                } else {
                    Palette::light(surface)
                }
            }
        }
    }
}

/// Counters the harness and tests read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Paint calls that took a fresh snapshot (painted, not necessarily presented).
    pub painted_frames: u64,
    /// Painted frames whose paint took longer than 33 ms.
    pub slow_frames: u64,
    /// Duration of the last paint, in microseconds.
    pub last_paint_micros: u64,
    /// Duration of the last snapshot, in microseconds.
    pub last_snapshot_micros: u64,
    /// Resizes that failed on the engine or the PTY.
    pub resize_errors: u64,
    /// When the last paint whose screen text differed from the one before finished.
    pub last_change_at: Option<std::time::Instant>,
}

/// State shared between the view and its element. UI thread only.
pub(crate) struct ViewShared {
    pub(crate) engine: Arc<TerminalEngine>,
    pub(crate) host: Arc<dyn TerminalHost>,
    pub(crate) session: SessionId,
    pub(crate) gate: Arc<FrameGate>,
    pub(crate) appearance: Cell<Appearance>,
    pub(crate) stats: Cell<FrameStats>,
    pub(crate) grid: Cell<TerminalSize>,
    /// Cell width and line height in pixels, measured at paint time.
    pub(crate) cell: Cell<(f32, f32)>,
    /// Input modes of the last painted snapshot.
    pub(crate) modes: Cell<Modes>,
    /// Plain text of the last painted snapshot, for tests and accessibility.
    pub(crate) screen_text: RefCell<String>,
    /// Bounds of the painted element in window coordinates.
    pub(crate) bounds: Cell<Bounds<Pixels>>,
    /// Cursor cell of the last painted snapshot, for the IME popup.
    pub(crate) cursor: Cell<Option<(u16, u16)>>,
    /// A one-line message for the user (refused paste, program not reading input).
    pub(crate) status: RefCell<Option<SharedString>>,
    /// A multiline paste waiting for the user's confirmation.
    pub(crate) pending_paste: RefCell<Option<String>>,
    /// IME pre-edit text, drawn at the cursor until it is committed or cancelled.
    pub(crate) marked: RefCell<Option<String>>,
    /// A left-button selection drag is in progress.
    pub(crate) dragging: Cell<bool>,
}

/// One terminal session on screen.
pub struct TerminalView {
    pub(crate) shared: Rc<ViewShared>,
    pub(crate) focus: FocusHandle,
    _wake_task: Task<()>,
}

/// The wake-up closure to hand to `TerminalEngine::start` plus the receiver the view listens on.
/// The closure runs on the parser thread: it flips the gate and, for the first request after a
/// frame, sends one message (decision D3).
pub fn wake_channel(
    gate: &Arc<FrameGate>,
) -> (Arc<dyn Fn() + Send + Sync>, async_channel::Receiver<()>) {
    let (sender, receiver) = async_channel::bounded::<()>(1);
    let gate = Arc::clone(gate);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if gate.request() {
            let _ = sender.try_send(());
        }
    });
    (wake, receiver)
}

impl TerminalView {
    /// Creates the view. `wake` is the receiver from [`wake_channel`], built with the same `gate`.
    pub fn new(
        engine: Arc<TerminalEngine>,
        host: Arc<dyn TerminalHost>,
        session: SessionId,
        appearance: Appearance,
        gate: Arc<FrameGate>,
        wake: async_channel::Receiver<()>,
        cx: &mut Context<Self>,
    ) -> Self {
        let palette = appearance.palette();
        engine.set_theme_colors(hex(palette.foreground), hex(palette.background));
        let shared = Rc::new(ViewShared {
            engine,
            host,
            session,
            gate,
            appearance: Cell::new(appearance),
            stats: Cell::new(FrameStats::default()),
            grid: Cell::new(TerminalSize { cols: 0, rows: 0 }),
            cell: Cell::new((0.0, 0.0)),
            modes: Cell::new(Modes::default()),
            screen_text: RefCell::new(String::new()),
            bounds: Cell::new(Bounds::default()),
            cursor: Cell::new(None),
            status: RefCell::new(None),
            pending_paste: RefCell::new(None),
            marked: RefCell::new(None),
            dragging: Cell::new(false),
        });
        let wake_task = cx.spawn(async move |this, cx| {
            while wake.recv().await.is_ok() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            shared,
            focus: cx.focus_handle(),
            _wake_task: wake_task,
        }
    }

    /// Swaps the palette. The PTY and the engine are untouched.
    pub fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        let palette = appearance.palette();
        self.shared
            .engine
            .set_theme_colors(hex(palette.foreground), hex(palette.background));
        self.shared.appearance.set(appearance);
        cx.notify();
    }

    /// Frame counters.
    pub fn frame_stats(&self) -> FrameStats {
        self.shared.stats.get()
    }

    /// The text of the last painted screen.
    pub fn screen_text(&self) -> String {
        self.shared.screen_text.borrow().clone()
    }

    /// The grid the view last sent to the engine and the PTY.
    pub fn grid(&self) -> TerminalSize {
        self.shared.grid.get()
    }
}

fn hex(color: Rgb) -> u32 {
    (u32::from(color.0) << 16) | (u32::from(color.1) << 8) | u32::from(color.2)
}

impl TerminalView {
    /// The message currently shown to the user, if any.
    pub fn status(&self) -> Option<String> {
        self.shared.status.borrow().as_ref().map(|s| s.to_string())
    }

    /// The handle the view takes keyboard focus with.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Input modes of the last painted screen.
    pub fn modes(&self) -> Modes {
        self.shared.modes.get()
    }

    /// Cell width and line height in pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        self.shared.cell.get()
    }

    /// True while a multiline paste waits for confirmation.
    pub fn has_pending_paste(&self) -> bool {
        self.shared.pending_paste.borrow().is_some()
    }

    fn set_status(&self, message: &'static str, cx: &mut Context<Self>) {
        *self.shared.status.borrow_mut() = Some(SharedString::from(message));
        cx.notify();
    }

    fn clear_status(&self) {
        *self.shared.status.borrow_mut() = None;
    }

    /// Sends bytes the user produced; a program that does not read its input is reported.
    pub fn send_bytes(&self, bytes: &[u8], cx: &mut Context<Self>) {
        self.send(bytes, cx);
    }

    fn send(&self, bytes: &[u8], cx: &mut Context<Self>) {
        match self.shared.engine.send_input(bytes) {
            Ok(()) => self.clear_status(),
            Err(error) => self.set_status(input_failure_message(error.code), cx),
        }
    }

    /// The cell under a window position, clamped to the grid, and which half of it.
    fn cell_at(&self, position: Point<Pixels>) -> (u16, u16, CellSide) {
        let (cell_w, cell_h) = self.shared.cell.get();
        let grid = self.shared.grid.get();
        let origin = self.shared.bounds.get().origin;
        let x = f32::from(position.x - origin.x).max(0.0);
        let y = f32::from(position.y - origin.y).max(0.0);
        let column = if cell_w > 0.0 { x / cell_w } else { 0.0 };
        let row = if cell_h > 0.0 { y / cell_h } else { 0.0 };
        let col = (column.floor() as u16).min(grid.cols.saturating_sub(1));
        let row = (row.floor() as u16).min(grid.rows.saturating_sub(1));
        let side = if column.fract() < 0.5 {
            CellSide::Left
        } else {
            CellSide::Right
        };
        (row, col, side)
    }

    fn reporting_mouse(&self, shift: bool) -> bool {
        let modes = self.shared.modes.get();
        modes.mouse_report && modes.sgr_mouse && !shift
    }

    fn report_mouse(
        &self,
        kind: MouseKind,
        position: Point<Pixels>,
        m: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let (row, col, _) = self.cell_at(position);
        let report = MouseReport {
            kind,
            row,
            col,
            shift: m.shift,
            alt: m.alt,
            control: m.control,
        };
        if let Some(bytes) = encode_mouse(&report, &self.shared.modes.get()) {
            self.send(&bytes, cx);
        }
    }

    /// Copies the selection to the clipboard (the user's explicit action).
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.shared.engine.selection_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Pastes the clipboard: single lines go straight in, multiline pastes wait for Enter.
    pub fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        match decide_paste(&text) {
            PasteDecision::Send => self.send_paste(&text, cx),
            PasteDecision::Confirm { .. } => {
                *self.shared.pending_paste.borrow_mut() = Some(text);
                cx.notify();
            }
            PasteDecision::TooLarge => {
                self.set_status("The paste is larger than 16 KiB and was refused.", cx)
            }
        }
    }

    fn send_paste(&mut self, text: &str, cx: &mut Context<Self>) {
        match self.shared.engine.paste(text) {
            Ok(()) => self.clear_status(),
            Err(error) => self.set_status(input_failure_message(error.code), cx),
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_str();
        // A pending multiline paste owns the keyboard until it is confirmed or cancelled.
        let pending = self.shared.pending_paste.borrow_mut().take();
        if let Some(text) = pending {
            match key {
                "enter" => self.send_paste(&text, cx),
                "escape" => {}
                _ => *self.shared.pending_paste.borrow_mut() = Some(text),
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if is_reserved(key, modifiers.control, modifiers.shift) {
            match key {
                "c" => self.copy_selection(cx),
                _ => self.paste_from_clipboard(cx),
            }
            cx.stop_propagation();
            return;
        }
        let chord = chord_from_keystroke(
            key,
            keystroke.key_char.as_deref(),
            modifiers.control,
            modifiers.alt,
            modifiers.shift,
        );
        if let Some(bytes) = encode_key(&chord, &self.shared.modes.get()) {
            // Typing returns the view to the live screen, as terminals do.
            self.shared.engine.scroll(ScrollRequest::Bottom);
            self.send(&bytes, cx);
            // Handled: the platform text-input path must not send the same character again.
            cx.stop_propagation();
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if self.reporting_mouse(event.modifiers.shift) {
            if let Some(button) = report_button(event.button) {
                self.report_mouse(
                    MouseKind::Press(button),
                    event.position,
                    event.modifiers,
                    cx,
                );
            }
            return;
        }
        if event.button != GpuiButton::Left {
            return;
        }
        let (row, col, side) = self.cell_at(event.position);
        let kind = match event.click_count {
            0 | 1 => SelectionKind::Simple,
            2 => SelectionKind::Word,
            _ => SelectionKind::Line,
        };
        self.shared.engine.start_selection(kind, row, col, side);
        self.shared.dragging.set(true);
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.reporting_mouse(event.modifiers.shift) {
            let kind = match event.pressed_button.and_then(report_button) {
                Some(button) => MouseKind::Drag(button),
                None => MouseKind::Move,
            };
            self.report_mouse(kind, event.position, event.modifiers, cx);
            return;
        }
        if self.shared.dragging.get() && event.pressed_button == Some(GpuiButton::Left) {
            let (row, col, side) = self.cell_at(event.position);
            self.shared.engine.update_selection(row, col, side);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.reporting_mouse(event.modifiers.shift) {
            if let Some(button) = report_button(event.button) {
                self.report_mouse(
                    MouseKind::Release(button),
                    event.position,
                    event.modifiers,
                    cx,
                );
            }
            return;
        }
        self.shared.dragging.set(false);
    }

    fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (_, cell_h) = self.shared.cell.get();
        let delta = event.delta.pixel_delta(px(cell_h.max(1.0)));
        let notches = (f32::from(delta.y) / cell_h.max(1.0)).round() as i32;
        if notches == 0 {
            return;
        }
        let modes = self.shared.modes.get();
        if self.reporting_mouse(event.modifiers.shift) {
            let kind = if notches > 0 {
                MouseKind::WheelUp
            } else {
                MouseKind::WheelDown
            };
            self.report_mouse(kind, event.position, event.modifiers, cx);
        } else if modes.alt_screen && modes.alternate_scroll {
            let arrow: &[u8] = match (notches > 0, modes.application_cursor) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            for _ in 0..notches.unsigned_abs().min(10) {
                self.send(arrow, cx);
            }
        } else {
            self.shared
                .engine
                .scroll(ScrollRequest::Lines(notches * WHEEL_LINES));
            cx.notify();
        }
    }
}

fn report_button(button: GpuiButton) -> Option<MouseButton> {
    match button {
        GpuiButton::Left => Some(MouseButton::Left),
        GpuiButton::Middle => Some(MouseButton::Middle),
        GpuiButton::Right => Some(MouseButton::Right),
        _ => None,
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.shared
            .marked
            .borrow()
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.shared.marked.borrow_mut().take().is_some() {
            cx.notify();
        }
    }

    /// Committed text (IME commit, AltGr output, dead-key results) goes to the program.
    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A commit ends the composition: the pre-edit text is replaced by what was committed.
        if self.shared.marked.borrow_mut().take().is_some() {
            cx.notify();
        }
        if !text.is_empty() {
            self.send(text.as_bytes(), cx);
        }
    }

    /// Pre-edit text is kept (and drawn at the cursor) until it is committed or cancelled; it
    /// is never sent to the program.
    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        *self.shared.marked.borrow_mut() = (!new_text.is_empty()).then(|| new_text.to_owned());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (cell_w, cell_h) = self.shared.cell.get();
        let (row, col) = self.shared.cursor.get()?;
        Some(Bounds::new(
            element_bounds.origin
                + gpui::point(px(f32::from(col) * cell_w), px(f32::from(row) * cell_h)),
            gpui::size(px(cell_w), px(cell_h)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let overlay = self.overlay_text();
        let mut root = div()
            .size_full()
            .relative()
            .track_focus(&self.focus)
            .key_context("Terminal")
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(GpuiButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(GpuiButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_down(GpuiButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_up(GpuiButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(GpuiButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(GpuiButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(TerminalElement::new(
                Rc::clone(&self.shared),
                cx.entity(),
                self.focus.clone(),
            ));
        if let Some(text) = overlay {
            root = root.child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .p_2()
                    .bg(gpui::rgb(0x2d2d2d))
                    .text_color(gpui::rgb(0xffffff))
                    .child(text),
            );
        }
        root
    }
}

impl TerminalView {
    /// The text of the paste preview or the status message, whichever applies.
    fn overlay_text(&self) -> Option<SharedString> {
        if let Some(text) = self.shared.pending_paste.borrow().as_ref() {
            let preview: Vec<&str> = text.lines().take(5).collect();
            let more = text.lines().count().saturating_sub(preview.len());
            let mut shown = format!(
                "Paste {} lines? Enter to confirm, Esc to cancel\n{}",
                text.lines().count(),
                preview.join("\n")
            );
            if more > 0 {
                shown.push_str(&format!("\n... and {more} more"));
            }
            return Some(SharedString::from(shown));
        }
        self.shared.status.borrow().clone()
    }
}
