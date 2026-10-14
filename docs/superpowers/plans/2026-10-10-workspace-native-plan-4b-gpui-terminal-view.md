# Workspace Native — Plan 4b: GPUI Terminal View and UI-bound Gates (P2b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **Tasks 0-9 are for the executor. Task 10 is run by the planner or the owner on the reference machine (KDE Wayland, real GPU session), never by an unattended executor.**

**Status: DRAFT for owner review.** Nothing in this plan has been executed. The decisions table below lists what the owner must confirm before Task 0.

**Goal:** Put the finished headless terminal engine of Plan 4 on screen with GPUI: a `TerminalView` that renders `Snapshot`s with batched text runs, encodes keys, mouse, paste and IME input, resizes the PTY, selects and copies, and a harness that measures the UI-bound performance gates (cold start, frame rate under a flood, four concurrent floods, keystroke latency, idle memory). The measured result decides whether P3 (editor, workbench) may start.

**Architecture:** The pure parts of the view live in `rwk-core` where they stay testable without a window: key encoding, mouse encoding, colour palette and a `PaintList` (plain rectangles and text items derived from a `Snapshot`). `rwk-ui` is the only crate that depends on GPUI: it turns a `PaintList` into quads and shaped lines inside a custom `Element`, owns focus, the wake-up bridge from the parser thread to the UI thread, and the input handlers. A new `rwk-ui-bench` crate opens real windows and prints one verdict per gate, the same way `term-bench` does for the headless gates.

**Tech Stack:** Rust 1.96.0; `gpui-pre =0.3.8` (imported as `gpui`), already verified by the supply-chain gate of Plan 2 (decision A, 2026-10-06); `rwk-core` from Plans 1-4. No `gpui-terminal`, no `gpui-component`, no `gpui-base` in this plan.

**Spec:** `docs/superpowers/specs/2026-10-05-raios-workspace-native-design.md`, section 6 (rendering, parity features), section 9 (gates), section 10 (P2 stop criterion), section 12 (risks 1-3). Builds on Plan 4 (`feat/workspace-native-p2a` must be present; its gate report is `docs/superpowers/reports/2026-10-terminal-engine-gates.md`).

## Plan series

| Plan | Phase | Status |
|---|---|---|
| 1 | P0 daemon contract | done (`feat/workspace-native-p0`) |
| 2 | P1a supply-chain gate + `rwk-core` skeleton | done (`feat/workspace-native-p1a`) |
| 3 | P1b `rwk-core` logic | done (`feat/workspace-native-p1b`) |
| 3b | P0b `ValidateAgentLaunch` | done (`feat/workspace-native-p1c`) |
| 4 | P2a terminal engine + headless gates | done (`feat/workspace-native-p2a`); G1-G7 pass, G8 recorded as a known risk |
| **4b (this)** | P2b GPUI terminal view + UI-bound gates + the P2 stop decision | draft |
| 5-7 | UI plans (P3-P5): editor, workbench, inspector, handoff, memory view | only after Task 10 allows it |

## What Plan 4 left open

1. **G8** (snapshot p99 with 4 concurrent floods, limit 8.3 ms) measured 8.6-9.9 ms on a quiet machine. Two engine-side fixes (parse slice 2 KiB, parser `nice +5`) did not move it, so it is a CPU-contention proxy. The owner decided (2026-10-10) to keep the limit and re-measure on the real view. **Gate U3 below is that re-measurement; if U3 passes, the G8 risk is closed; if it fails, Task 10 reopens the 4 ms time-slice fallback of spec section 6.**
2. The spec lists "editor open" among the UI-bound gates. It measures `gpui-base`'s `EditorState`, whose API this plan has not verified, and it needs none of the terminal view. **Decision D5 moves it to P3.** Task 10 states this.

## Decisions in this plan (owner review before execution)

The plan implements the recommendation of each decision; say so if you want another option.

| # | Decision | Recommendation (implemented) | Alternative |
|---|---|---|---|
| D1 | Vendoring from `gpui-terminal` (spec section 6 names `colors.rs`, `box_drawing.rs`, `input.rs`, `mouse.rs`) | Vendor **nothing**. Palette, key and mouse encoding are small, pure and written test-first in `rwk-core` (Tasks 2-4). The package is not in the local registry, targets `gpui 0.2.2`, and the spike called it demo grade. Box-drawing glyphs come from the font first | Fetch `gpui-terminal 0.1.0`, port `box_drawing.rs`: only worth it if Task 10's visual check shows seams in box-drawing characters |
| D2 | Renderer | Per row, one background quad per merged run of non-default background, one `ShapedLine` per text item positioned at `col * cell_width`. Positioning every run at its own column bounds font-fallback width drift to one run | One shaped line per row with fixed advance (`force_width`): fewer calls, but one wide glyph shifts the rest of the row |
| D3 | Wake-up bridge | The parser thread calls a wake closure; an `AtomicBool` coalesces calls and a single async task per view calls `cx.notify()`. At most one notify per rendered frame is queued | Notify on every wake: floods then queue thousands of notifies |
| D4 | Mouse reporting | SGR (1006) encoding only. A program that asks for mouse reports without SGR gets none; the view falls back to selection. The spec's parity list says "SGR mouse reporting" | Add legacy X10/UTF-8 encodings: more code for programs that are rare today |
| D5 | Editor-open gate | Moved to P3 (see above) | Measure it here with a throwaway `EditorState`: depends on an API nobody verified |
| D6 | Frame-rate measurement | Count **painted** frames (paint callbacks that took a fresh snapshot) per second, and report that this is not a presented-frame count. GPUI exposes no present timestamp; the spec already says compositor and display are excluded | Instrument the Wayland frame callback: needs platform internals, not public API |
| D7 | Pure code location | `rwk-core::terminal::{palette, keys, mouse, paint}` (no GPUI, no new dependencies) | Put them in `rwk-ui`: loses headless tests |
| D8 | New crates | `rwk-ui` (library, GPUI) and `rwk-ui-bench` (binary). `rwk-app` waits for P3 (YAGNI) | Create `rwk-app` now with an empty `main` |

## Public API (the skeleton for review)

```text
rwk_core::terminal::palette
  Rgb(u8, u8, u8)                        // Copy, Eq; Rgb::from_hex(0xRRGGBB)
  Palette { ansi: [Rgb; 16], foreground, background, cursor, selection: Rgb }
  Palette::dark(surface: Surface) / Palette::light(surface: Surface)   // Surface { foreground, background, cursor, selection: Rgb }
  Palette::resolve(&self, TermColor, default: Rgb) -> Rgb              // indexed 0-15 palette, 16-231 cube, 232-255 greys, RGB unchanged
  Palette::dim(&self, fg: Rgb, bg: Rgb) -> Rgb

rwk_core::terminal::keys
  KeyChord { key: String, text: Option<String>, control, alt, shift: bool }
  encode_key(&KeyChord, &Modes) -> Option<Vec<u8>>                     // None: the chord has no terminal meaning

rwk_core::terminal::mouse
  MouseButton { Left, Middle, Right }
  MouseKind { Press(MouseButton), Release(MouseButton), Drag(MouseButton), Move, WheelUp, WheelDown }
  MouseReport { kind, row: u16, col: u16, shift, alt, control: bool }
  encode_mouse(&MouseReport, &Modes) -> Option<Vec<u8>>                // SGR only (D4)

rwk_core::terminal::paint
  BgRect { row, col, cells: u16, color: Rgb }
  TextItem { row, col, cells: u16, text: String, color: Rgb, bold, italic, underline, strikeout: bool }
  CursorPaint { row, col: u16, shape: CursorShape, color: Rgb }
  PaintList { cols, rows: u16, backgrounds: Vec<BgRect>, texts: Vec<TextItem>, cursor: Option<CursorPaint>, selection: Vec<RowSpan> }
  build_paint_list(&Snapshot, &Palette) -> PaintList

rwk_ui::terminal
  TerminalView::new(engine: Arc<TerminalEngine>, host: Arc<dyn TerminalHost>, session: SessionId,
                    appearance: Appearance, cx: &mut Context<Self>) -> Self       // Entity<TerminalView>, Render
  TerminalView::set_appearance(&mut self, Appearance, cx)                         // swaps the palette; PTY and engine untouched
  TerminalView::frame_stats(&self) -> FrameStats                                  // painted frames, last paint duration, snapshot duration
  TerminalElement                                                                 // the custom Element that paints a PaintList
  FrameGate::new() / request() -> bool / take()                                   // wake coalescing (D3)
  Appearance { Dark, Light }

rwk_ui_bench (binary `ui-bench`)
  ui-bench cold-start | flood | flood4 | keystroke | idle    // each prints its gate line(s) and exits 0 (pass), 1 (fail), 3 (inconclusive)
  ui-bench all --enforce --runs 5
```

## Gates (UI-bound; reference machine, real Wayland session)

| Id | Gate | Limit | Spec source |
|---|---|---|---|
| U1 | process spawn to first painted frame of one terminal window | <= 300 ms | cold start |
| U2 | painted frames per second during a 4M-line flood in one pane (median of runs) | >= 45 fps | frame rate |
| U3 | painted frames per second with 4 panes each flooding 1M lines (median) | >= 30 fps | 4 concurrent floods; closes G8 |
| U4 | keystroke event to the paint that shows the echoed character, p99 over 200 keystrokes | <= 16 ms | keystroke-to-photon (event to painted frame; excludes compositor and display) |
| U5 | idle PSS, one window with 4 live terminals, 10 s after the last output | <= 150 MB | idle PSS (editor excluded until P3, D5) |

Rule inherited from Plan 4 (P7): the harness measures how much of the machine other processes use; if they used more than 25% before or during the run and a gate failed, the verdict is INCONCLUSIVE (exit 3). A pass always counts. U2, U3 and U4 additionally record the paint count of frames that exceeded 33 ms so a reader sees stalls, not only the average.

## Global Constraints

- Rust toolchain 1.96.0, edition 2021. `cargo fmt` and `cargo clippy --all-targets -- -D warnings` clean for `tools/raios-workspace-native`.
- GPUI is imported as `gpui = { package = "gpui-pre", version = "=0.3.8" }`. Exact pin, committed `Cargo.lock`. Adding or changing any `gpui*` dependency re-runs `scripts/verify_gpui_supply_chain.py` (Plan 2) before the commit.
- **Dependency boundary:** `rwk-core` keeps its Plan 4 direct dependency list unchanged (no `gpui*`, `wgpu`, `tokio`, `raios-core`, `raios-runtime`, `raios-surface-*`, `tauri`). `rwk-ui` may depend on `rwk-core` and `gpui` only (plus `async-channel` if Task 6 needs it). `rwk-ui-bench` depends on `rwk-ui`, `rwk-core` and `libc`. The existing dependency-boundary test is extended in Task 1, not weakened.
- No `unwrap()`/`expect()` in non-test code; every public item has a doc comment; every `unsafe` block has a `// SAFETY:` comment.
- Terminal and clipboard content never enters a log. The view writes nothing to disk. OSC 52 stays disabled (Plan 4 P2); copying happens only on the user's explicit action.
- Paste rules (spec section 6): multiline paste shows a preview confirmation first; 16 KiB cap (`MAX_PASTE_BYTES`); bracketed when the program asked.
- Soft wrap does not apply here. The terminal grid never reflows in the view; resize goes to the engine and the PTY.
- Commits require the owner's explicit go-ahead; steps marked **(OWNER GO-AHEAD)** must not run without it. Commit trailer: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` only when Claude Sonnet 5.5 executes this plan; any other executor writes its own accurate line.
- Shell commands avoid pipes, `cd`, `tee`, `sed` and `awk`: run cargo from the repository root with `--manifest-path tools/raios-workspace-native/Cargo.toml`, use `-q`. Window tests and the bench need `WAYLAND_DISPLAY`; tests that need a display are `#[ignore]` and are run explicitly with `-- --ignored` in Task 9 and Task 10.
- **Long builds:** the first GPUI build compiles ~700 crates. Give each cargo command the largest timeout the shell tool allows. If it is cut off, run exactly the same command again.

## Review Focus

Failure modes most likely to bite a person using the terminal, each with a pinned test:

1. **A flood must not starve input or paint.** Typing while `yes` floods must still echo within the U4 limit, and the UI thread must never block on the engine lock longer than one snapshot (`a_keystroke_during_a_flood_reaches_the_screen`, gate U2/U4).
2. **Wrong bytes for a key silently corrupt a session.** Application-cursor arrows, Alt-prefixed characters, Ctrl-letter codes, modified arrows (`ESC [ 1 ; 5 A`) and function keys are table-tested (`arrows_follow_the_application_cursor_mode`, `control_letters_map_to_c0_codes`, `modified_arrows_carry_the_xterm_modifier_parameter`).
3. **Mouse reports must not leak when the program did not ask.** With reporting off, a click selects text and sends nothing; with it on, the SGR bytes are exact (`a_click_sends_nothing_unless_the_program_asked`, `sgr_press_and_release_use_m_and_uppercase_m`).
4. **A resize must reach both the engine and the PTY, and only when the cell grid changed.** Dragging a divider by one pixel must not send `SIGWINCH` storms (`a_sub_cell_resize_changes_nothing`).
5. **Paste must not run commands.** A multiline paste waits for the preview confirmation; a paste over 16 KiB is refused with a visible message; ESC and NUL never survive (`a_multiline_paste_waits_for_confirmation`, plus the Plan 4 paste tests).
6. **Colours must match the 0.1 palette and stay legible.** Indexed 16-255, 24-bit, inverse, dim and hidden cells resolve exactly as specified (`indexed_colours_follow_the_xterm_cube_and_greys`, `inverse_swaps_resolved_colours`, `hidden_cells_paint_no_text`).
7. **A closed or exited session must not leave a live view that paints stale output forever or panics.** (`a_finished_session_shows_its_last_screen_and_stops_requesting_frames`).

---

## File Structure

| File | Action | Task |
|---|---|---|
| `tools/raios-workspace-native/Cargo.toml` | modify (members) | 1, 8 |
| `.../crates/rwk-core/src/terminal/{palette,keys,mouse,paint}.rs` | create | 2, 3, 4, 5 |
| `.../crates/rwk-core/src/terminal/mod.rs` | modify (modules, re-exports) | 2-5 |
| `.../crates/rwk-core/tests/terminal_{palette,keys,mouse,paint}.rs` | create | 2-5 |
| `.../crates/rwk-ui/Cargo.toml`, `src/lib.rs` | create | 1 |
| `.../crates/rwk-ui/src/terminal/{mod,gate,metrics,element,view,input}.rs` | create | 6, 7 |
| `.../crates/rwk-ui/tests/api_probe.rs` | create | 1 |
| `.../crates/rwk-ui/tests/view_window.rs` | create (`#[ignore]`, needs a display) | 6, 7 |
| `.../crates/rwk-ui-bench/Cargo.toml`, `src/{main,gates,scenarios,machine}.rs` | create | 8 |
| `CHANGELOG.md`, `memory.md`, `tools/raios-workspace-native/README.md` | modify | 9 |
| `docs/superpowers/reports/2026-10-terminal-ui-gates.md` | create | 10 |
| `docs/security/gpui-supply-chain-2026-10-06.md` | append a "re-verified for Plan 4b" line only if a gpui version changed | 1 |

---

### Task 0: Worktree and baseline

**Files:** none changed.

- [ ] **Step 1: Create the branch from Plan 4's result**

Run from the repository root:

```bash
git worktree add .worktrees/workspace-native-p2b -b feat/workspace-native-p2b feat/workspace-native-p2a
```

Expected: a new worktree at `.worktrees/workspace-native-p2b` on `feat/workspace-native-p2b`. All following commands run with that worktree as the repository root.

- [ ] **Step 2: Baseline tests**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core
```

Expected: all `rwk-core` tests pass (the 6 terminal test binaries of Plan 4 included). If any fail, stop and report; do not build on a red baseline.

- [ ] **Step 3: Baseline lint and audit**

```bash
cargo clippy -q --all-targets --manifest-path tools/raios-workspace-native/Cargo.toml -- -D warnings
cargo audit --file tools/raios-workspace-native/Cargo.lock
```

Expected: no warnings, no advisories (Plan 4 recorded 168 clean crates).

---

### Task 1: The `rwk-ui` crate and the GPUI API probe

The API of `gpui-pre` changes with every daily snapshot. This task pins the exact signatures the rest of the plan uses by compiling against them, so a later task cannot rest on a remembered signature.

**Files:**
- Modify: `tools/raios-workspace-native/Cargo.toml`
- Create: `tools/raios-workspace-native/crates/rwk-ui/Cargo.toml`, `src/lib.rs`, `tests/api_probe.rs`

**Interfaces:**
- Produces: a compiling `rwk-ui` crate with the exact `gpui` dependency line every later task reuses.

- [ ] **Step 1: Add the workspace member**

In `tools/raios-workspace-native/Cargo.toml` set `members = ["crates/rwk-core", "crates/rwk-bench", "crates/rwk-ui"]`.

- [ ] **Step 2: Create `crates/rwk-ui/Cargo.toml`**

```toml
[package]
name = "rwk-ui"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
rwk-core = { path = "../rwk-core" }
gpui = { package = "gpui-pre", version = "=0.3.8" }
```

- [ ] **Step 3: Create `src/lib.rs`**

```rust
//! The GPUI user interface of the native workspace. Only this crate (and the app crates above
//! it) may depend on GPUI; `rwk-core` stays headless.

pub mod terminal;
```

and an empty `src/terminal/mod.rs` containing only the line `//! The terminal view.`

- [ ] **Step 4: Write the API probe (the failing test)**

Create `crates/rwk-ui/tests/api_probe.rs`. It only has to compile; each function uses a signature the later tasks rely on, copied from `gpui-pre-0.3.8`:

```rust
//! Compile-time pin of every GPUI 0.3.8 signature Plan 4b uses. If a GPUI bump changes one, this
//! file stops compiling and the plan's tasks must be re-read before the bump is accepted.

use gpui::{
    fill, App, Bounds, Context, Element, ElementId, Entity, FocusHandle, GlobalElementId,
    InspectorElementId, IntoElement, KeyDownEvent, LayoutId, Pixels, Render, SharedString,
    TextAlign, TextRun, Window,
};

struct Probe;

impl IntoElement for Probe {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Probe {
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
        (window.request_layout(gpui::Style::default(), [], cx), ())
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
        window.paint_quad(fill(bounds, gpui::black()));
        let runs: [TextRun; 0] = [];
        let line = window
            .text_system()
            .shape_line(SharedString::from("x"), Pixels::from(14.0), &runs, None);
        let _ = line.paint(
            bounds.origin,
            Pixels::from(18.0),
            TextAlign::Left,
            None,
            window,
            cx,
        );
        window.request_animation_frame();
        window.on_next_frame(|_window, _cx| {});
    }
}

struct ProbeView {
    focus: FocusHandle,
}

impl Render for ProbeView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Probe
    }
}

#[allow(dead_code)]
fn pins(cx: &mut App, window: &mut Window, entity: &Entity<ProbeView>, event: &KeyDownEvent) {
    // Keystroke fields used by the key mapper (Task 7).
    let _ = (&event.keystroke.key, &event.keystroke.key_char);
    let _ = (
        event.keystroke.modifiers.control,
        event.keystroke.modifiers.alt,
        event.keystroke.modifiers.shift,
    );
    // Window-level calls used by the harness (Task 8).
    let _ = window.dispatch_keystroke(gpui::Keystroke::parse("a").unwrap_or_default(), cx);
    entity.update(cx, |_view, cx| cx.notify());
}

#[test]
fn the_gpui_surface_this_plan_uses_still_compiles() {}
```

> The probe is allowed to use `unwrap_or_default` and a trivial test body because it is a compile pin, not behaviour. If a pinned item is not exactly as written (for example `Keystroke::parse` returns another error type, or `request_layout` takes another argument list), fix the probe to the real signature from `~/.cargo/registry/src/*/gpui-pre-0.3.8/src` and **record the corrected signature in this plan's Task 6 and 7 code before continuing**: the later code blocks are derived from this one.

- [ ] **Step 5: Run it to see the first failure, then make it compile**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui --test api_probe
```

Expected first run: the dependency build (long), then either PASS or compile errors that name the real signatures. Fix the probe until it passes.

- [ ] **Step 6: Re-run the supply-chain gate and the dependency boundary**

```bash
python3 tools/raios-workspace-native/scripts/verify_gpui_supply_chain.py docs/security/gpui-0.3.8.Cargo.lock --expected-owner huacnlee --allowlist tools/raios-workspace-native/scripts/gpui-supply-chain-allowlist.toml
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core --test dependency_boundary
```

Expected: the script reports 0 findings for the committed lockfile; the boundary test passes, proving `rwk-core` gained no GPUI dependency. Then run `cargo audit --file tools/raios-workspace-native/Cargo.lock`; any new advisory in the GPUI tree is reported to the owner, not silenced.

- [ ] **Step 7: Commit (OWNER GO-AHEAD)**

```bash
git add tools/raios-workspace-native/Cargo.toml tools/raios-workspace-native/Cargo.lock tools/raios-workspace-native/crates/rwk-ui
git commit -m "feat: add the rwk-ui crate and pin the GPUI API surface"
```

---

### Task 2: Palette and colour resolution (`rwk-core`)

**Files:**
- Create: `crates/rwk-core/src/terminal/palette.rs`, `crates/rwk-core/tests/terminal_palette.rs`
- Modify: `crates/rwk-core/src/terminal/mod.rs`

**Interfaces:**
- Consumes: `TermColor` from `terminal::snapshot`.
- Produces: `Rgb`, `Surface`, `Palette` with `dark`, `light`, `resolve`, `dim` (signatures in the skeleton above).

- [ ] **Step 1: Write the failing tests** in `tests/terminal_palette.rs`

```rust
use rwk_core::terminal::palette::{Palette, Rgb, Surface};
use rwk_core::terminal::TermColor;

fn surface() -> Surface {
    Surface {
        foreground: Rgb::from_hex(0xd4d4d4),
        background: Rgb::from_hex(0x1e1e1e),
        cursor: Rgb::from_hex(0xffffff),
        selection: Rgb::from_hex(0x264f78),
    }
}

#[test]
fn indexed_colours_follow_the_xterm_cube_and_greys() {
    let palette = Palette::dark(surface());
    let default = Rgb(1, 2, 3);
    // 196 = 16 + 180: r=5 (255), g=0, b=0.
    assert_eq!(
        palette.resolve(TermColor::Indexed(196), default),
        Rgb(255, 0, 0)
    );
    // 16 is the cube origin, 231 its far corner.
    assert_eq!(
        palette.resolve(TermColor::Indexed(16), default),
        Rgb(0, 0, 0)
    );
    assert_eq!(
        palette.resolve(TermColor::Indexed(231), default),
        Rgb(255, 255, 255)
    );
    // Greys: 8 + 10 * (index - 232).
    assert_eq!(
        palette.resolve(TermColor::Indexed(232), default),
        Rgb(8, 8, 8)
    );
    assert_eq!(
        palette.resolve(TermColor::Indexed(255), default),
        Rgb(238, 238, 238)
    );
}

#[test]
fn the_sixteen_ansi_entries_come_from_the_workspace_0_1_palette() {
    let dark = Palette::dark(surface());
    let light = Palette::light(surface());
    let default = Rgb(0, 0, 0);
    assert_eq!(
        dark.resolve(TermColor::Indexed(1), default),
        Rgb::from_hex(0xcd3131)
    );
    assert_eq!(
        dark.resolve(TermColor::Indexed(15), default),
        Rgb::from_hex(0xffffff)
    );
    assert_eq!(
        light.resolve(TermColor::Indexed(1), default),
        Rgb::from_hex(0xa12622)
    );
    assert_eq!(
        light.resolve(TermColor::Indexed(15), default),
        Rgb::from_hex(0x7d8792)
    );
}

#[test]
fn the_default_colour_and_24_bit_colours_pass_through() {
    let palette = Palette::dark(surface());
    assert_eq!(
        palette.resolve(TermColor::Default, Rgb(9, 9, 9)),
        Rgb(9, 9, 9)
    );
    assert_eq!(
        palette.resolve(TermColor::Rgb(10, 20, 30), Rgb(9, 9, 9)),
        Rgb(10, 20, 30)
    );
}

#[test]
fn dim_moves_a_colour_a_third_of_the_way_to_the_background() {
    let palette = Palette::dark(surface());
    assert_eq!(
        palette.dim(Rgb(255, 255, 255), Rgb(0, 0, 0)),
        Rgb(170, 170, 170)
    );
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core --test terminal_palette
```

Expected: FAIL to compile (`palette` module missing).

- [ ] **Step 3: Implement** `src/terminal/palette.rs`

```rust
//! Terminal colours: the 16 ANSI entries of Workspace 0.1, the xterm 6x6x6 cube and the grey
//! ramp. Surface colours (foreground, background, cursor, selection) follow the skin; the ANSI
//! entries stay a terminal palette, as in 0.1's `terminal-rendering.ts`.

use super::snapshot::TermColor;

/// An opaque 24-bit colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Builds a colour from `0xRRGGBB`.
    pub const fn from_hex(value: u32) -> Rgb {
        Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
    }
}

/// The skin-dependent colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surface {
    /// Default text colour.
    pub foreground: Rgb,
    /// Default background colour.
    pub background: Rgb,
    /// Cursor colour.
    pub cursor: Rgb,
    /// Selection highlight colour.
    pub selection: Rgb,
}

const DARK_ANSI: [Rgb; 16] = [
    Rgb::from_hex(0x1e1e1e),
    Rgb::from_hex(0xcd3131),
    Rgb::from_hex(0x0dbc79),
    Rgb::from_hex(0xe5e510),
    Rgb::from_hex(0x2472c8),
    Rgb::from_hex(0xbc3fbc),
    Rgb::from_hex(0x11a8cd),
    Rgb::from_hex(0xe5e5e5),
    Rgb::from_hex(0x666666),
    Rgb::from_hex(0xf14c4c),
    Rgb::from_hex(0x23d18b),
    Rgb::from_hex(0xf5f543),
    Rgb::from_hex(0x3b8eea),
    Rgb::from_hex(0xd670d6),
    Rgb::from_hex(0x29b8db),
    Rgb::from_hex(0xffffff),
];

const LIGHT_ANSI: [Rgb; 16] = [
    Rgb::from_hex(0x141a23),
    Rgb::from_hex(0xa12622),
    Rgb::from_hex(0x276338),
    Rgb::from_hex(0x80651a),
    Rgb::from_hex(0x245da8),
    Rgb::from_hex(0x853f8f),
    Rgb::from_hex(0x216a75),
    Rgb::from_hex(0x49515a),
    Rgb::from_hex(0x69727c),
    Rgb::from_hex(0xcf3631),
    Rgb::from_hex(0x31824b),
    Rgb::from_hex(0x9c7a1f),
    Rgb::from_hex(0x3478ce),
    Rgb::from_hex(0xaa55b8),
    Rgb::from_hex(0x278593),
    Rgb::from_hex(0x7d8792),
];

/// Levels of one channel of the 6x6x6 colour cube.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// A complete terminal palette for one tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// ANSI colours 0-15.
    pub ansi: [Rgb; 16],
    /// Default text colour.
    pub foreground: Rgb,
    /// Default background colour.
    pub background: Rgb,
    /// Cursor colour.
    pub cursor: Rgb,
    /// Selection highlight colour.
    pub selection: Rgb,
}

impl Palette {
    /// The dark-tone palette on the given surface.
    pub const fn dark(surface: Surface) -> Palette {
        Palette::with_ansi(DARK_ANSI, surface)
    }

    /// The light-tone palette on the given surface.
    pub const fn light(surface: Surface) -> Palette {
        Palette::with_ansi(LIGHT_ANSI, surface)
    }

    const fn with_ansi(ansi: [Rgb; 16], surface: Surface) -> Palette {
        Palette {
            ansi,
            foreground: surface.foreground,
            background: surface.background,
            cursor: surface.cursor,
            selection: surface.selection,
        }
    }

    /// Resolves a terminal colour; `default` is used for [`TermColor::Default`].
    pub fn resolve(&self, color: TermColor, default: Rgb) -> Rgb {
        match color {
            TermColor::Default => default,
            TermColor::Rgb(r, g, b) => Rgb(r, g, b),
            TermColor::Indexed(index) if index < 16 => self.ansi[usize::from(index)],
            TermColor::Indexed(index) if index < 232 => {
                let offset = usize::from(index - 16);
                Rgb(
                    CUBE_LEVELS[offset / 36],
                    CUBE_LEVELS[(offset / 6) % 6],
                    CUBE_LEVELS[offset % 6],
                )
            }
            TermColor::Indexed(index) => {
                let grey = 8 + 10 * (index - 232);
                Rgb(grey, grey, grey)
            }
        }
    }

    /// The colour of dimmed text: a third of the way from `fg` to `bg`.
    pub fn dim(&self, fg: Rgb, bg: Rgb) -> Rgb {
        let mix = |a: u8, b: u8| ((u16::from(a) * 2 + u16::from(b)) / 3) as u8;
        Rgb(mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2))
    }
}
```

Add `pub mod palette;` to `terminal/mod.rs`.

- [ ] **Step 4: Run to verify they pass**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core --test terminal_palette
```

Expected: 4 passed.

- [ ] **Step 5: Commit (OWNER GO-AHEAD)**

```bash
git add tools/raios-workspace-native/crates/rwk-core
git commit -m "feat: add the terminal palette and colour resolution"
```

---

### Task 3: Key encoding (`rwk-core`)

**Files:**
- Create: `crates/rwk-core/src/terminal/keys.rs`, `crates/rwk-core/tests/terminal_keys.rs`
- Modify: `crates/rwk-core/src/terminal/mod.rs`

**Interfaces:**
- Consumes: `Modes` from `terminal::snapshot` (`application_cursor`, `application_keypad` are the fields used).
- Produces: `KeyChord`, `encode_key`.

- [ ] **Step 1: Write the failing tests** in `tests/terminal_keys.rs`

```rust
use rwk_core::terminal::keys::{encode_key, KeyChord};
use rwk_core::terminal::Modes;

fn chord(key: &str) -> KeyChord {
    KeyChord {
        key: key.to_owned(),
        text: None,
        control: false,
        alt: false,
        shift: false,
    }
}
fn typed(key: &str, text: &str) -> KeyChord {
    KeyChord {
        text: Some(text.to_owned()),
        ..chord(key)
    }
}
fn bytes(chord: &KeyChord, modes: &Modes) -> Vec<u8> {
    encode_key(chord, modes).expect("the chord has a terminal meaning")
}

#[test]
fn printable_text_is_sent_as_utf8() {
    assert_eq!(bytes(&typed("a", "a"), &Modes::default()), b"a");
    assert_eq!(bytes(&typed("ü", "ü"), &Modes::default()), "ü".as_bytes());
}

#[test]
fn control_letters_map_to_c0_codes() {
    let mut c = chord("c");
    c.control = true;
    assert_eq!(bytes(&c, &Modes::default()), vec![0x03]);
    c.key = "[".into();
    assert_eq!(bytes(&c, &Modes::default()), vec![0x1b]);
    c.key = "space".into();
    assert_eq!(bytes(&c, &Modes::default()), vec![0x00]);
}

#[test]
fn alt_prefixes_escape() {
    let mut c = typed("x", "x");
    c.alt = true;
    assert_eq!(bytes(&c, &Modes::default()), b"\x1bx");
}

#[test]
fn arrows_follow_the_application_cursor_mode() {
    let normal = Modes::default();
    let app = Modes {
        application_cursor: true,
        ..Modes::default()
    };
    assert_eq!(bytes(&chord("up"), &normal), b"\x1b[A");
    assert_eq!(bytes(&chord("up"), &app), b"\x1bOA");
    assert_eq!(bytes(&chord("left"), &normal), b"\x1b[D");
}

#[test]
fn modified_arrows_carry_the_xterm_modifier_parameter() {
    let mut c = chord("right");
    c.control = true; // 1 + 4
    assert_eq!(bytes(&c, &Modes::default()), b"\x1b[1;5C");
    c.shift = true; // 1 + 1 + 4
    assert_eq!(bytes(&c, &Modes::default()), b"\x1b[1;6C");
    c.control = false;
    c.shift = false;
    c.alt = true; // 1 + 2
    assert_eq!(bytes(&c, &Modes::default()), b"\x1b[1;3C");
}

#[test]
fn editing_and_function_keys_use_their_standard_sequences() {
    let m = Modes::default();
    assert_eq!(bytes(&chord("enter"), &m), b"\r");
    assert_eq!(bytes(&chord("tab"), &m), b"\t");
    assert_eq!(bytes(&chord("escape"), &m), b"\x1b");
    assert_eq!(bytes(&chord("backspace"), &m), vec![0x7f]);
    assert_eq!(bytes(&chord("delete"), &m), b"\x1b[3~");
    assert_eq!(bytes(&chord("pageup"), &m), b"\x1b[5~");
    assert_eq!(bytes(&chord("home"), &m), b"\x1b[H");
    assert_eq!(bytes(&chord("f1"), &m), b"\x1bOP");
    assert_eq!(bytes(&chord("f5"), &m), b"\x1b[15~");
    assert_eq!(bytes(&chord("f12"), &m), b"\x1b[24~");
    let mut back = chord("tab");
    back.shift = true;
    assert_eq!(bytes(&back, &m), b"\x1b[Z");
}

#[test]
fn a_chord_without_a_terminal_meaning_encodes_to_nothing() {
    let mut c = chord("f20");
    assert!(encode_key(&c, &Modes::default()).is_none());
    c = chord("shift");
    assert!(encode_key(&c, &Modes::default()).is_none());
}
```

- [ ] **Step 2: Run to verify they fail** (module missing)

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core --test terminal_keys
```

- [ ] **Step 3: Implement** `src/terminal/keys.rs`

```rust
//! Keyboard input to PTY bytes. Pure: the UI maps its key event to a [`KeyChord`] and the same
//! table serves every toolkit. Reserved workbench shortcuts are filtered before this runs.

use super::snapshot::Modes;

/// One key press as the terminal needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    /// Lower-case key name: a character, or `enter`, `tab`, `escape`, `backspace`, `delete`,
    /// `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `insert`, `space`,
    /// `f1`..`f12`.
    pub key: String,
    /// The text the key produced under the active layout, when any (IME and AltGr output).
    pub text: Option<String>,
    /// Control is held.
    pub control: bool,
    /// Alt is held.
    pub alt: bool,
    /// Shift is held.
    pub shift: bool,
}

impl KeyChord {
    /// The xterm modifier parameter: 1 + shift + 2*alt + 4*control.
    fn modifier_parameter(&self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control)
    }

    fn has_modifier(&self) -> bool {
        self.shift || self.alt || self.control
    }
}

/// Encodes a key press, or `None` when it has no terminal meaning (a bare modifier, an unknown
/// function key, a control chord with no C0 code).
pub fn encode_key(chord: &KeyChord, modes: &Modes) -> Option<Vec<u8>> {
    if let Some(sequence) = named_key(chord, modes) {
        return Some(sequence);
    }
    if chord.control {
        return control_code(&chord.key).map(|code| with_alt(chord, vec![code]));
    }
    let text = chord.text.as_deref().filter(|text| !text.is_empty())?;
    Some(with_alt(chord, text.as_bytes().to_vec()))
}

fn with_alt(chord: &KeyChord, mut bytes: Vec<u8>) -> Vec<u8> {
    if chord.alt {
        bytes.insert(0, 0x1b);
    }
    bytes
}

fn control_code(key: &str) -> Option<u8> {
    let mut chars = key.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return (key == "space").then_some(0x00);
    }
    match first {
        'a'..='z' => Some(first as u8 - b'a' + 1),
        '@' | ' ' => Some(0x00),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' | '/' => Some(0x1f),
        _ => None,
    }
}

fn named_key(chord: &KeyChord, modes: &Modes) -> Option<Vec<u8>> {
    let key = chord.key.as_str();
    let modified = chord.has_modifier();
    let parameter = chord.modifier_parameter();
    let cursor = |letter: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{parameter}{letter}").into_bytes()
        } else if modes.application_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{code};{parameter}~").into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let sequence = match key {
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "enter" => with_alt(chord, vec![b'\r']),
        "tab" if chord.shift => b"\x1b[Z".to_vec(),
        "tab" => with_alt(chord, vec![b'\t']),
        "escape" => with_alt(chord, vec![0x1b]),
        "backspace" => with_alt(chord, vec![0x7f]),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        _ => return None,
    };
    Some(sequence)
}
```

> `control_code("space")` is reached because `space` is not in `named_key`. Add `pub mod keys;` to `terminal/mod.rs`. `chord.text` for `"a"` with `control=false` takes the last branch.

- [ ] **Step 4: Run to verify they pass**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core --test terminal_keys
```

Expected: 7 passed. If a test and the table disagree, the table is the specification (xterm control sequences); fix the test only if it contradicts xterm's documented sequence.

- [ ] **Step 5: Commit (OWNER GO-AHEAD)** — `feat: encode keyboard input for the terminal`

---

### Task 4: Mouse encoding (`rwk-core`)

**Files:**
- Create: `crates/rwk-core/src/terminal/mouse.rs`, `crates/rwk-core/tests/terminal_mouse.rs`
- Modify: `crates/rwk-core/src/terminal/mod.rs`

**Interfaces:**
- Consumes: `Modes` (`mouse_report`, `mouse_drag`, `mouse_motion`, `sgr_mouse`).
- Produces: `MouseButton`, `MouseKind`, `MouseReport`, `encode_mouse`.

- [ ] **Step 1: Write the failing tests** in `tests/terminal_mouse.rs`

```rust
use rwk_core::terminal::mouse::{encode_mouse, MouseButton, MouseKind, MouseReport};
use rwk_core::terminal::Modes;

fn report(kind: MouseKind, row: u16, col: u16) -> MouseReport {
    MouseReport {
        kind,
        row,
        col,
        shift: false,
        alt: false,
        control: false,
    }
}
fn reporting() -> Modes {
    Modes {
        mouse_report: true,
        sgr_mouse: true,
        ..Modes::default()
    }
}

#[test]
fn a_click_sends_nothing_unless_the_program_asked() {
    let off = Modes::default();
    assert!(encode_mouse(&report(MouseKind::Press(MouseButton::Left), 0, 0), &off).is_none());
    // Asking for reports without the SGR encoding is not supported (decision D4).
    let legacy = Modes {
        mouse_report: true,
        ..Modes::default()
    };
    assert!(encode_mouse(&report(MouseKind::Press(MouseButton::Left), 0, 0), &legacy).is_none());
}

#[test]
fn sgr_press_and_release_use_m_and_uppercase_m() {
    let modes = reporting();
    // Coordinates are one-based on the wire.
    assert_eq!(
        encode_mouse(&report(MouseKind::Press(MouseButton::Left), 4, 9), &modes).unwrap(),
        b"\x1b[<0;10;5M"
    );
    assert_eq!(
        encode_mouse(&report(MouseKind::Release(MouseButton::Left), 4, 9), &modes).unwrap(),
        b"\x1b[<0;10;5m"
    );
    assert_eq!(
        encode_mouse(&report(MouseKind::Press(MouseButton::Right), 0, 0), &modes).unwrap(),
        b"\x1b[<2;1;1M"
    );
}

#[test]
fn modifiers_and_wheel_set_the_button_bits() {
    let modes = reporting();
    let mut press = report(MouseKind::Press(MouseButton::Left), 0, 0);
    press.control = true; // +16
    press.shift = true; // +4
    assert_eq!(encode_mouse(&press, &modes).unwrap(), b"\x1b[<20;1;1M");
    assert_eq!(
        encode_mouse(&report(MouseKind::WheelUp, 0, 0), &modes).unwrap(),
        b"\x1b[<64;1;1M"
    );
    assert_eq!(
        encode_mouse(&report(MouseKind::WheelDown, 0, 0), &modes).unwrap(),
        b"\x1b[<65;1;1M"
    );
}

#[test]
fn drag_and_motion_need_their_own_modes() {
    let drag = report(MouseKind::Drag(MouseButton::Left), 1, 1);
    let motion = report(MouseKind::Move, 1, 1);
    assert!(encode_mouse(&drag, &reporting()).is_none());
    let with_drag = Modes {
        mouse_drag: true,
        ..reporting()
    };
    assert_eq!(encode_mouse(&drag, &with_drag).unwrap(), b"\x1b[<32;2;2M");
    assert!(encode_mouse(&motion, &with_drag).is_none());
    let with_motion = Modes {
        mouse_motion: true,
        ..reporting()
    };
    assert_eq!(
        encode_mouse(&motion, &with_motion).unwrap(),
        b"\x1b[<35;2;2M"
    );
}
```

- [ ] **Step 2: Run to verify they fail** (module missing) — same command pattern with `--test terminal_mouse`.

- [ ] **Step 3: Implement** `src/terminal/mouse.rs`

```rust
//! Mouse input to PTY bytes, SGR (1006) encoding only (decision D4).

use super::snapshot::Modes;

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Primary button.
    Left,
    /// Wheel button.
    Middle,
    /// Secondary button.
    Right,
}

impl MouseButton {
    fn code(self) -> u16 {
        match self {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }
    }
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    /// A button went down.
    Press(MouseButton),
    /// A button went up.
    Release(MouseButton),
    /// The pointer moved with a button held.
    Drag(MouseButton),
    /// The pointer moved with no button held.
    Move,
    /// The wheel turned away from the user.
    WheelUp,
    /// The wheel turned toward the user.
    WheelDown,
}

/// One mouse event in terminal cell coordinates (zero based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseReport {
    /// The event.
    pub kind: MouseKind,
    /// Visible row.
    pub row: u16,
    /// Visible column.
    pub col: u16,
    /// Shift is held.
    pub shift: bool,
    /// Alt is held.
    pub alt: bool,
    /// Control is held.
    pub control: bool,
}

/// Encodes a mouse event for the program, or `None` when the program did not ask for it.
pub fn encode_mouse(report: &MouseReport, modes: &Modes) -> Option<Vec<u8>> {
    if !modes.mouse_report || !modes.sgr_mouse {
        return None;
    }
    let (base, release) = match report.kind {
        MouseKind::Press(button) => (button.code(), false),
        MouseKind::Release(button) => (button.code(), true),
        MouseKind::Drag(button) if modes.mouse_drag || modes.mouse_motion => {
            (button.code() + 32, false)
        }
        MouseKind::Move if modes.mouse_motion => (35, false),
        MouseKind::WheelUp => (64, false),
        MouseKind::WheelDown => (65, false),
        MouseKind::Drag(_) | MouseKind::Move => return None,
    };
    let code = base
        + 4 * u16::from(report.shift)
        + 8 * u16::from(report.alt)
        + 16 * u16::from(report.control);
    let final_byte = if release { 'm' } else { 'M' };
    Some(
        format!(
            "\x1b[<{code};{};{}{final_byte}",
            report.col + 1,
            report.row + 1
        )
        .into_bytes(),
    )
}
```

Add `pub mod mouse;`.

- [ ] **Step 4: Run to verify they pass** — expected 4 passed.

- [ ] **Step 5: Commit (OWNER GO-AHEAD)** — `feat: encode mouse reports for the terminal`

---

### Task 5: The paint list (`rwk-core`)

The renderer's logic without a window: which rectangles and which text items a `Snapshot` produces.

**Files:**
- Create: `crates/rwk-core/src/terminal/paint.rs`, `crates/rwk-core/tests/terminal_paint.rs`
- Modify: `crates/rwk-core/src/terminal/mod.rs`

**Interfaces:**
- Consumes: `Snapshot`, `Run`, `Style`, `Cursor`, `CursorShape`, `RowSpan` (Plan 4); `Palette`, `Rgb` (Task 2).
- Produces: `BgRect`, `TextItem`, `CursorPaint`, `PaintList`, `build_paint_list`.

- [ ] **Step 1: Write the failing tests** in `tests/terminal_paint.rs`. Build snapshots directly from the public types:

```rust
use rwk_core::terminal::paint::build_paint_list;
use rwk_core::terminal::palette::{Palette, Rgb, Surface};
use rwk_core::terminal::{Cursor, CursorShape, Line, Modes, Run, Snapshot, Style, TermColor};

fn palette() -> Palette {
    Palette::dark(Surface {
        foreground: Rgb::from_hex(0xd4d4d4),
        background: Rgb::from_hex(0x1e1e1e),
        cursor: Rgb::from_hex(0xffffff),
        selection: Rgb::from_hex(0x264f78),
    })
}

fn run(col: u16, text: &str, style: Style) -> Run {
    Run {
        col,
        cells: text.chars().count() as u16,
        text: text.to_owned(),
        style,
    }
}

fn snapshot(lines: Vec<Line>) -> Snapshot {
    Snapshot {
        cols: 20,
        rows: lines.len() as u16,
        lines,
        cursor: None,
        modes: Modes::default(),
        display_offset: 0,
        history_size: 0,
        selection: Vec::new(),
        title: String::new(),
    }
}

#[test]
fn plain_text_paints_no_background() {
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "hello", Style::PLAIN)],
        }]),
        &palette(),
    );
    assert!(list.backgrounds.is_empty());
    assert_eq!(list.texts.len(), 1);
    assert_eq!(list.texts[0].text, "hello");
    assert_eq!(list.texts[0].color, Rgb::from_hex(0xd4d4d4));
}

#[test]
fn adjacent_runs_with_the_same_background_merge_into_one_rectangle() {
    let red = Style {
        bg: TermColor::Indexed(1),
        ..Style::PLAIN
    };
    let bold_red = Style { bold: true, ..red };
    let line = Line {
        runs: vec![run(0, "ab", red), run(2, "cd", bold_red)],
    };
    let list = build_paint_list(&snapshot(vec![line]), &palette());
    assert_eq!(list.backgrounds.len(), 1);
    assert_eq!((list.backgrounds[0].col, list.backgrounds[0].cells), (0, 4));
    assert_eq!(list.texts.len(), 2);
}

#[test]
fn inverse_swaps_resolved_colours() {
    let inverse = Style {
        inverse: true,
        ..Style::PLAIN
    };
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "x", inverse)],
        }]),
        &palette(),
    );
    assert_eq!(list.backgrounds[0].color, Rgb::from_hex(0xd4d4d4));
    assert_eq!(list.texts[0].color, Rgb::from_hex(0x1e1e1e));
}

#[test]
fn hidden_cells_paint_no_text() {
    let hidden = Style {
        hidden: true,
        ..Style::PLAIN
    };
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "secret", hidden)],
        }]),
        &palette(),
    );
    assert!(list.texts.is_empty());
}

#[test]
fn blank_runs_without_decoration_are_not_shaped() {
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "   ", Style::PLAIN)],
        }]),
        &palette(),
    );
    assert!(list.texts.is_empty());
    let underlined = Style {
        underline: true,
        ..Style::PLAIN
    };
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "   ", underlined)],
        }]),
        &palette(),
    );
    assert_eq!(list.texts.len(), 1);
}

#[test]
fn the_cursor_and_selection_are_carried_through() {
    let mut s = snapshot(vec![Line::default()]);
    s.cursor = Some(Cursor {
        col: 3,
        row: 0,
        shape: CursorShape::Beam,
    });
    let list = build_paint_list(&s, &palette());
    let cursor = list.cursor.expect("the cursor is visible");
    assert_eq!((cursor.row, cursor.col), (0, 3));
    assert_eq!(cursor.color, Rgb::from_hex(0xffffff));
}
```

- [ ] **Step 2: Run to verify they fail** (module missing).

- [ ] **Step 3: Implement** `src/terminal/paint.rs`

```rust
//! The renderer's decisions without a window: from a [`Snapshot`] and a [`Palette`] to plain
//! rectangles and text items. A toolkit paints them one-to-one.

use super::palette::{Palette, Rgb};
use super::snapshot::{CursorShape, RowSpan, Snapshot};

/// A filled background rectangle covering whole cells of one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BgRect {
    /// Visible row.
    pub row: u16,
    /// First column.
    pub col: u16,
    /// Width in cells.
    pub cells: u16,
    /// Fill colour.
    pub color: Rgb,
}

/// Text drawn from one column, with its decorations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextItem {
    /// Visible row.
    pub row: u16,
    /// Column of the first cell.
    pub col: u16,
    /// Cells covered.
    pub cells: u16,
    /// The characters.
    pub text: String,
    /// Foreground colour after inverse and dim.
    pub color: Rgb,
    /// Bold weight.
    pub bold: bool,
    /// Italic slant.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Strike-through.
    pub strikeout: bool,
}

/// Where and how the cursor is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPaint {
    /// Visible row.
    pub row: u16,
    /// Column.
    pub col: u16,
    /// Shape requested by the program.
    pub shape: CursorShape,
    /// Cursor colour.
    pub color: Rgb,
}

/// Everything a toolkit paints for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintList {
    /// Columns of the grid.
    pub cols: u16,
    /// Rows of the grid.
    pub rows: u16,
    /// Background rectangles, merged along rows.
    pub backgrounds: Vec<BgRect>,
    /// Text items.
    pub texts: Vec<TextItem>,
    /// The cursor, when visible.
    pub cursor: Option<CursorPaint>,
    /// Selected cells (painted with [`Palette::selection`]).
    pub selection: Vec<RowSpan>,
}

/// Builds the paint list for one snapshot.
pub fn build_paint_list(snapshot: &Snapshot, palette: &Palette) -> PaintList {
    let mut backgrounds: Vec<BgRect> = Vec::new();
    let mut texts = Vec::new();
    for (row_index, line) in snapshot.lines.iter().enumerate() {
        let row = row_index as u16;
        for run in &line.runs {
            let style = run.style;
            let mut fg = palette.resolve(style.fg, palette.foreground);
            let mut bg = palette.resolve(style.bg, palette.background);
            if style.inverse {
                std::mem::swap(&mut fg, &mut bg);
            }
            if style.dim {
                fg = palette.dim(fg, bg);
            }
            if bg != palette.background {
                push_background(&mut backgrounds, row, run.col, run.cells, bg);
            }
            let blank = run.text.chars().all(char::is_whitespace);
            let decorated = style.underline || style.strikeout;
            if style.hidden || (blank && !decorated) {
                continue;
            }
            texts.push(TextItem {
                row,
                col: run.col,
                cells: run.cells,
                text: run.text.clone(),
                color: fg,
                bold: style.bold,
                italic: style.italic,
                underline: style.underline,
                strikeout: style.strikeout,
            });
        }
    }
    PaintList {
        cols: snapshot.cols,
        rows: snapshot.rows,
        backgrounds,
        texts,
        cursor: snapshot.cursor.map(|cursor| CursorPaint {
            row: cursor.row,
            col: cursor.col,
            shape: cursor.shape,
            color: palette.cursor,
        }),
        selection: snapshot.selection.clone(),
    }
}

fn push_background(list: &mut Vec<BgRect>, row: u16, col: u16, cells: u16, color: Rgb) {
    if let Some(last) = list.last_mut() {
        if last.row == row && last.color == color && last.col + last.cells == col {
            last.cells += cells;
            return;
        }
    }
    list.push(BgRect {
        row,
        col,
        cells,
        color,
    });
}
```

Add `pub mod paint;` and re-export `pub use paint::{build_paint_list, BgRect, CursorPaint, PaintList, TextItem};`, `pub use palette::{Palette, Rgb, Surface};`, `pub use keys::{encode_key, KeyChord};`, `pub use mouse::{encode_mouse, MouseButton, MouseKind, MouseReport};` in `terminal/mod.rs`.

- [ ] **Step 4: Run all four new test binaries plus the lint**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core
cargo clippy -q --all-targets --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core -- -D warnings
```

Expected: everything passes, no warnings. Parse and snapshot throughput must be unaffected (nothing in Plan 4's engine changed).

- [ ] **Step 5: Commit (OWNER GO-AHEAD)** — `feat: derive a toolkit-neutral paint list from terminal snapshots`

---

### Task 6: The terminal element and view (`rwk-ui`)

Renders one session: wake bridge, cell metrics, resize, paint. Input comes in Task 7.

**Files:**
- Create: `crates/rwk-ui/src/terminal/{gate,metrics,element,view}.rs`, `crates/rwk-ui/tests/view_window.rs`
- Modify: `crates/rwk-ui/src/terminal/mod.rs`, `crates/rwk-ui/Cargo.toml` (add `async-channel = "2"` only if Step 3 needs it; record the exact version resolved)

**Interfaces:**
- Consumes: `TerminalEngine` (`snapshot`, `resize`, `set_theme_colors`, `set_cell_pixels`, `is_output_closed`), `TerminalHost::{resize, is_running}`, `SessionId`, `TerminalSize`, `Palette`, `build_paint_list` (Tasks 2, 5), and the pinned GPUI signatures of Task 1.
- Produces: `FrameGate`, `FrameStats`, `Appearance`, `TerminalElement`, `TerminalView`.

- [ ] **Step 1: `gate.rs` — the wake coalescer (test first, pure)**

```rust
//! Coalesces wake-ups from the parser thread into at most one pending UI notification.

use std::sync::atomic::{AtomicBool, Ordering};

/// One pending-notification flag shared by the parser thread and the UI.
#[derive(Debug, Default)]
pub struct FrameGate {
    pending: AtomicBool,
}

impl FrameGate {
    /// A gate with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a notification as wanted. Returns true only for the call that must schedule it.
    pub fn request(&self) -> bool {
        !self.pending.swap(true, Ordering::AcqRel)
    }

    /// Called by the UI right before it takes a snapshot: output arriving afterwards re-arms.
    pub fn take(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::FrameGate;

    #[test]
    fn only_the_first_request_after_a_take_schedules() {
        let gate = FrameGate::new();
        assert!(gate.request());
        assert!(!gate.request());
        assert!(!gate.request());
        gate.take();
        assert!(gate.request());
    }
}
```

Run `cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui gate` → PASS.

- [ ] **Step 2: `metrics.rs` — cell size and grid from pixels (test first, pure)**

```rust
//! Cell geometry: how many whole cells fit in a pixel area, and when a resize is a real change.

use rwk_core::sessions::TerminalSize;

/// Smallest and largest grid the engine accepts (Plan 4: MIN/MAX_COLS/ROWS).
pub const MIN_COLS: u16 = 2;
/// Largest column count.
pub const MAX_COLS: u16 = 500;
/// Smallest row count.
pub const MIN_ROWS: u16 = 2;
/// Largest row count.
pub const MAX_ROWS: u16 = 300;

/// The whole-cell grid that fits in `width` x `height` pixels, clamped to the engine's limits.
pub fn grid_for(width: f32, height: f32, cell_width: f32, cell_height: f32) -> TerminalSize {
    let fit = |extent: f32, cell: f32, min: u16, max: u16| -> u16 {
        if cell <= 0.0 || extent <= 0.0 {
            return min;
        }
        ((extent / cell).floor() as u32).clamp(u32::from(min), u32::from(max)) as u16
    };
    TerminalSize {
        cols: fit(width, cell_width, MIN_COLS, MAX_COLS),
        rows: fit(height, cell_height, MIN_ROWS, MAX_ROWS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_cells_only() {
        let size = grid_for(805.0, 400.0, 8.0, 16.0);
        assert_eq!((size.cols, size.rows), (100, 25));
    }

    #[test]
    fn a_sub_cell_resize_changes_nothing() {
        assert_eq!(grid_for(805.0, 400.0, 8.0, 16.0), grid_for(807.9, 401.0, 8.0, 16.0));
    }

    #[test]
    fn the_grid_is_clamped_to_the_engine_limits() {
        let tiny = grid_for(1.0, 1.0, 8.0, 16.0);
        assert_eq!((tiny.cols, tiny.rows), (MIN_COLS, MIN_ROWS));
        let huge = grid_for(100_000.0, 100_000.0, 8.0, 16.0);
        assert_eq!((huge.cols, huge.rows), (MAX_COLS, MAX_ROWS));
    }
}
```

`TerminalSize` must derive `PartialEq` for the second test; if it does not (check `sessions.rs:85`), add `PartialEq, Eq` to its derive list in `rwk-core` in this step. Run → PASS.

- [ ] **Step 3: The failing window test** in `tests/view_window.rs` (`#[ignore]`, needs `WAYLAND_DISPLAY`)

Build a `TerminalEngine` over an in-memory pipe pair (as `tests/terminal_engine.rs` of Plan 4 does), open a window containing a `TerminalView`, write `"hello\r\n"` into the engine's input side, wait for a painted frame, and assert `view.frame_stats().painted_frames >= 1` and that the last snapshot's `row_text(0) == "hello"`. Also:

```rust
#[test]
#[ignore = "needs a Wayland display"]
fn a_finished_session_shows_its_last_screen_and_stops_requesting_frames() { /* close the pipe, wait, assert painted_frames stops increasing over 300 ms and the screen text is unchanged */ }
```

Use `gpui::Application::new().run(...)` exactly as pinned by the probe; open the window with `cx.open_window(WindowOptions::default(), |window, cx| cx.new(|cx| TerminalView::new(...)))`. If `Application::new` or `open_window` differ in 0.3.8, add them to the Task 1 probe first.

Run `cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui --test view_window -- --ignored` → FAIL (view missing).

- [ ] **Step 4: `view.rs` and `element.rs` — implement**

`TerminalView` fields: `engine: Arc<TerminalEngine>`, `host: Arc<dyn TerminalHost>`, `session: SessionId`, `appearance: Appearance`, `gate: Arc<FrameGate>`, `focus: FocusHandle`, `last_grid: Cell<TerminalSize>`, `stats: Rc<RefCell<FrameStats>>`, `_wake_task: Task<()>`.

Wake bridge (decision D3): the engine was started with a `wake: Arc<dyn Fn() + Send + Sync>` that the **caller** builds from `view.wake_handle()`:

```rust
/// Returns the closure to pass to `TerminalEngine::start` as its wake-up. It runs on the parser
/// thread: it only flips the gate and, for the first request after a frame, sends one message.
pub fn wake_handle(gate: &Arc<FrameGate>, sender: async_channel::Sender<()>) -> Arc<dyn Fn() + Send + Sync> {
    let gate = Arc::clone(gate);
    Arc::new(move || {
        if gate.request() {
            let _ = sender.try_send(());
        }
    })
}
```

and `TerminalView::new` starts one task with `cx.spawn(async move |this, cx| { while receiver.recv().await.is_ok() { if this.update(cx, |_, cx| cx.notify()).is_err() { break; } } })` (signature pinned by the Task 1 probe: `AsyncFnOnce(WeakEntity<T>, &mut AsyncApp)`). The channel is bounded to 1.

`Render for TerminalView` returns `TerminalElement::new(entity_handle)` inside a `div().size_full().track_focus(&self.focus)`; Task 7 adds the handlers.

`TerminalElement::paint`:
1. `gate.take()`, then `engine.snapshot()` (timed into `FrameStats::last_snapshot_micros`).
2. Cell metrics: shape `"M"` once per font/size via `window.text_system().shape_line` and read its width as `cell_width`; `line_height` is the configured value (decision: font size 14 px, line height 18 px for the first slice). Cache both on the view; when they change call `engine.set_cell_pixels(w, h)`.
3. `let size = grid_for(bounds.size.width.into(), bounds.size.height.into(), cell_w, cell_h);` if `size != last_grid`: `engine.resize(size)` **and** `host.resize(&session, size)` (both, in that order), then `last_grid.set(size)`.
4. `build_paint_list(&snapshot, &palette)`; paint background quads with `window.paint_quad(fill(rect, color))`, selection quads with the selection colour at 0.5 alpha, then each `TextItem` as `shape_line(text, size, &[TextRun { len: text.len(), font, color, background_color: None, underline, strikethrough }], None).paint(origin, line_height, TextAlign::Left, None, window, cx)` with `origin = bounds.origin + (col * cell_w, row * line_height)`, then the cursor quad (block: full cell; underline: bottom 2 px; beam: left 2 px).
5. `stats.painted_frames += 1`; record `last_paint_micros`. If the engine reports `!is_output_closed()` do nothing more (the next wake notifies); if closed, do not request another frame.

Fonts: use one monospace `Font { family: "monospace".into(), ..Font::default() }` for the first slice and bold/italic variants for those flags; record in the gate report which family fontconfig resolved. Configurable fonts are P3.

- [ ] **Step 5: Run the pure tests, then the window test**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui --test view_window -- --ignored
```

Expected: the pure tests pass; both window tests pass on a Wayland session. On a machine with no display the second command is skipped and Task 9 records that.

- [ ] **Step 6: Lint and commit (OWNER GO-AHEAD)** — `feat: render a terminal session in a GPUI view`

---

### Task 7: Input — keys, IME, mouse, scroll, selection, copy, paste

**Files:**
- Create: `crates/rwk-ui/src/terminal/input.rs`
- Modify: `crates/rwk-ui/src/terminal/view.rs`, `crates/rwk-ui/tests/view_window.rs`

**Interfaces:**
- Consumes: `encode_key`, `KeyChord`, `encode_mouse`, `MouseReport` (Tasks 3, 4); `TerminalEngine::{send_input, paste, scroll, start_selection, update_selection, clear_selection, selection_text}`; the pinned `KeyDownEvent`, `EntityInputHandler`, `Window::handle_input`.
- Produces: input handling on `TerminalView`; `PasteDecision`.

- [ ] **Step 1: Failing tests (pure first)** in `input.rs`'s `#[cfg(test)]`:

```rust
#[test]
fn a_multiline_paste_waits_for_confirmation() {
    assert_eq!(decide_paste("one line"), PasteDecision::Send);
    assert_eq!(decide_paste("two\nlines"), PasteDecision::Confirm { lines: 2 });
}

#[test]
fn an_oversized_paste_is_refused() {
    let big = "x".repeat(rwk_core::terminal::MAX_PASTE_BYTES + 1);
    assert_eq!(decide_paste(&big), PasteDecision::TooLarge);
}

#[test]
fn a_key_event_maps_to_a_chord() {
    let chord = chord_from_keystroke("c", Some("c"), true, false, false);
    assert_eq!(chord.key, "c");
    assert!(chord.control && !chord.alt && !chord.shift);
}

#[test]
fn reserved_workbench_chords_are_not_forwarded() {
    // Ctrl+Shift+C copies, Ctrl+Shift+V pastes; neither reaches the program.
    assert!(is_reserved("c", true, true));
    assert!(is_reserved("v", true, true));
    assert!(!is_reserved("c", true, false)); // plain Ctrl+C is SIGINT for the shell
}
```

- [ ] **Step 2: Implement `input.rs`**

```rust
//! Input decisions of the terminal view that do not need a window.

use rwk_core::terminal::{KeyChord, MAX_PASTE_BYTES};

/// What to do with text the user wants to paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteDecision {
    /// A single line: send it now.
    Send,
    /// Several lines: show a preview and wait for the user (spec section 6).
    Confirm {
        /// Number of lines the paste contains.
        lines: usize,
    },
    /// Larger than the 16 KiB cap: refuse with a visible message.
    TooLarge,
}

/// Classifies a paste.
pub fn decide_paste(text: &str) -> PasteDecision {
    if text.len() > MAX_PASTE_BYTES {
        return PasteDecision::TooLarge;
    }
    let lines = text.lines().count();
    if lines > 1 {
        PasteDecision::Confirm { lines }
    } else {
        PasteDecision::Send
    }
}

/// Builds the chord for a key event.
pub fn chord_from_keystroke(
    key: &str,
    text: Option<&str>,
    control: bool,
    alt: bool,
    shift: bool,
) -> KeyChord {
    KeyChord {
        key: key.to_owned(),
        text: text.map(str::to_owned),
        control,
        alt,
        shift,
    }
}

/// Chords the workbench keeps for itself. Plain Ctrl+C/V and every unmodified key go to the
/// program (spec section 7).
pub fn is_reserved(key: &str, control: bool, shift: bool) -> bool {
    control && shift && matches!(key, "c" | "v")
}
```

Make sure `MAX_PASTE_BYTES` is re-exported from `rwk_core::terminal` (Plan 4 does it). Run the four unit tests → PASS.

- [ ] **Step 3: Wire the view**

In `view.rs`: register `on_key_down` on the focused div: build the chord from `event.keystroke` (key, `key_char`, modifiers), skip reserved chords (copy / paste actions run instead), call `encode_key(&chord, &snapshot.modes)` using the **last painted** snapshot's modes (cached on the view, not a fresh lock), and `engine.send_input(&bytes)`; on `Err(Conflict)` show the "program is not reading its input" message through the view's status line. Implement `EntityInputHandler` for `TerminalView` so IME commits (`replace_text_in_range`) go to `engine.send_input(text.as_bytes())` and `marked_text_range` returns `None` in the first slice (pre-edit rendering is P3 polish; record it as a known limit in the gate report). Install it with `window.handle_input(&self.focus, ElementInputHandler::new(bounds, entity), cx)` from `TerminalElement::paint`.

Mouse: translate positions to cells with the cached cell size; when `modes.mouse_report && modes.sgr_mouse` and Shift is not held, send `encode_mouse` bytes; otherwise run selection (`start_selection` on press with `SelectionKind::Simple`, `update_selection` on drag, double-click `Semantic`, triple-click `Lines`), and `Ctrl+Shift+C` copies `selection_text()` through the GPUI clipboard. Wheel: reported when the program asked, else `engine.scroll(ScrollRequest::Lines(±3))`; on the alternate screen with `alternate_scroll` the wheel becomes arrow keys.

Paste (`Ctrl+Shift+V`, and the platform paste event): read the clipboard text, `decide_paste`; `Send` → `engine.paste(&text)`; `Confirm` → set the view's pending-paste state, which `render` shows as an in-view overlay with the first 5 lines and `Enter` to confirm / `Escape` to cancel; `TooLarge` → status message, nothing sent.

- [ ] **Step 4: Window tests** (`#[ignore]`) in `view_window.rs`:

```rust
#[test]
#[ignore = "needs a Wayland display"]
fn a_keystroke_during_a_flood_reaches_the_screen() { /* flood `yes` through an in-memory engine, dispatch Keystroke "a" via window.dispatch_keystroke, assert the echo (a `cat`-like loop in the harness) appears within 100 ms of the next painted frame */ }

#[test]
#[ignore = "needs a Wayland display"]
fn a_click_sends_nothing_unless_the_program_asked_in_the_view() { /* click inside the view with modes off: no bytes arrive at the PTY side; selection exists */ }
```

Run all ignored tests; all PASS.

- [ ] **Step 5: Lint and commit (OWNER GO-AHEAD)** — `feat: handle keyboard, mouse, paste and copy in the terminal view`

---

### Task 8: The `ui-bench` harness and the gates

**Files:**
- Create: `crates/rwk-ui-bench/Cargo.toml`, `src/{main,gates,scenarios,machine}.rs`
- Modify: `tools/raios-workspace-native/Cargo.toml` (members)

**Interfaces:**
- Consumes: `rwk-ui` (`TerminalView`, `FrameStats`), `rwk-core` (`TerminalEngine`, `PtyTerminalHost`, `SessionKind`, `build_command`), the foreign-CPU measurement from `rwk-bench` (copy its small `machine.rs` logic; `rwk-bench` is a binary crate and is not a dependency).
- Produces: the `ui-bench` binary and its exit codes (0 pass, 1 fail, 3 inconclusive).

- [ ] **Step 1: Pure tests first** for the gate arithmetic in `gates.rs`: median of runs, p99 of microsecond latencies, fps from `(frames, duration)`, and the verdict rule (`verdict(gate_failed, foreign_cpu_percent) -> Pass | Fail | Inconclusive` using the 25% limit and "a pass always counts"). Mirror `rwk-bench/src/gates.rs` and its tests (`a_failure_on_a_busy_machine_is_inconclusive_but_a_pass_always_counts`); copy the logic, do not depend on the crate.

- [ ] **Step 2: Scenarios** in `scenarios.rs`, each opening a real window through `Application::new().run`:

| Command | What it does | Output |
|---|---|---|
| `cold-start` | the parent spawns `ui-bench window --first-frame` as a child and times spawn to the child's single stdout line `FIRST_FRAME`; the child prints it from `on_next_frame` after its first paint | U1 ms, median of runs |
| `flood` | one pane, `yes`-style 4M lines through a real PTY; counts painted frames and frames over 33 ms until the flood ends | U2 fps |
| `flood4` | four panes in one window, each 1M lines at the same time | U3 fps |
| `keystroke` | one pane running `cat`; 200 times: record `Instant`, `window.dispatch_keystroke`, wait for the paint whose snapshot contains the new character; record the delta | U4 p99 ms |
| `idle` | four live shells, no output; wait 10 s; read `Pss:` from `/proc/self/smaps_rollup` | U5 MB |

`flood*` and `keystroke` start the PTY with `PtyTerminalHost` and a `ProgramPaths` pointing at `/bin/sh` for the shell kind, so no agent wrapper or network is involved. Output drains are verified (all lines parsed) before the fps is computed, as in `term-bench`.

- [ ] **Step 3: Output format** identical to `term-bench`: one line per gate (`U2 frames per second during a 4M-line flood  measured 61.4 fps  limit 45.0 fps  PASS`), the foreign CPU before and during, `--runs N` with the median for rates and the worst for latencies, and a final `ALL GATES PASSED` / `AT LEAST ONE GATE FAILED ON A QUIET MACHINE` / `INCONCLUSIVE: ...` line. `--enforce` makes the process exit with the verdict code.

- [ ] **Step 4: Run the pure tests and a smoke run**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui-bench
cargo run -q --release --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui-bench --bin ui-bench -- cold-start --runs 1
```

Expected: tests pass; the smoke run prints a U1 line (any value; this step only proves the harness works).

- [ ] **Step 5: Commit (OWNER GO-AHEAD)** — `feat: add the UI-bound performance harness`

---

### Task 9: Verification gate and documentation

**Files:**
- Modify: `CHANGELOG.md`, `memory.md`, `tools/raios-workspace-native/README.md`, `SIGMAP.md` (via `sigmap`)

- [ ] **Step 1: Full verification**

```bash
cargo fmt --check --manifest-path tools/raios-workspace-native/Cargo.toml
cargo clippy -q --all-targets --manifest-path tools/raios-workspace-native/Cargo.toml -- -D warnings
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui -- --ignored
cargo audit --file tools/raios-workspace-native/Cargo.lock
python3 tools/raios-workspace-native/scripts/verify_gpui_supply_chain.py docs/security/gpui-0.3.8.Cargo.lock --expected-owner huacnlee --allowlist tools/raios-workspace-native/scripts/gpui-supply-chain-allowlist.toml
```

Expected: all green. The `--ignored` run needs a Wayland session; if the executor has none, record "window tests not run" in the commit body and hand them to Task 10.

- [ ] **Step 2: Documentation** — `CHANGELOG.md` entry for the terminal view; `README.md` of `raios-workspace-native` gets a "Terminal view" section (what is implemented, known limits: no IME pre-edit, SGR mouse only, no box-drawing special-casing, fixed font); `memory.md` change-log line under the executor's own identity; `sigmap`, then `raios pre-flight`.

- [ ] **Step 3: Commit (OWNER GO-AHEAD)** — `docs: record the native terminal view`

---

### Task 10: Authoritative gate run and the P2 stop decision (planner or owner, reference machine)

Not for an unattended executor.

- [ ] **Step 1:** Close other heavy work (Gradle daemons, indexers; Plan 4's G8 runs were polluted by exactly that). Confirm `uptime` load below 1.5.
- [ ] **Step 2:** `cargo run --release -p rwk-ui-bench --bin ui-bench -- all --enforce --runs 5`, three consecutive times. Save the raw output.
- [ ] **Step 3:** Look at the window by eye with `ls --color`, `htop`, `vim`, `git log --graph --color` and a box-drawing demo. Record: wrong colours, seams in box-drawing characters (reopens D1), misaligned wide characters, cursor shape errors.
- [ ] **Step 4:** Write `docs/superpowers/reports/2026-10-terminal-ui-gates.md` with the table of U1-U5 over the three runs, the verbatim output, the visual notes, and the stated exclusions (compositor and display latency not measured; frames are painted, not presented; editor-open moved to P3).
- [ ] **Step 5: The decision.**
  - U1-U5 pass (or only INCONCLUSIVE with a pass on the next quiet run): **P3 may start**; G8 is closed by U3.
  - U3 fails while U2 passes: reopen the time-sliced fallback of spec section 6 for multi-pane only, as a Plan 4c; P3 waits.
  - U4 fails: profile the path event -> `send_input` -> echo -> snapshot -> paint before anything else; P3 waits.
  - U1 or U5 fails by less than 20%: the owner decides whether to accept (these are hypotheses, per spec section 9) or fix.

### Task 11: Stop here

Do not start P3 inside this plan. The next plan (editor, file tree, workbench shell) is written after Task 10, using the measured numbers.

---

## Out of scope

IME pre-edit rendering; configurable fonts and font fallback tuning; box-drawing special glyphs (D1); legacy mouse encodings (D4); hyperlinks and OSC 8; image protocols; the editor-open gate (D5); daemon-hosted PTYs; Windows and macOS.

## Self-review

- **Spec coverage:** section 6 parity list — colours (Task 2, 5), `TERM`/`COLORTERM` (Plan 4 `build_command`), scrollback (Plan 4 engine, wheel in Task 7), bracketed paste with multiline preview and 16 KiB cap (Task 7), resize (Task 6), selection and copy (Task 7), SGR mouse (Tasks 4, 7), IME commit (Task 7; pre-edit listed as out of scope), AltGr (text carried in `KeyChord.text`). Section 9 gates — cold start, frame rate, four floods, keystroke, idle PSS (U1-U5); editor open moved (D5, flagged to the owner). Section 10 stop criterion — Task 10.
- **Type consistency:** `KeyChord`, `MouseReport`, `Palette`, `PaintList` are defined once (Tasks 2-5) and only consumed later; `FrameStats` fields (`painted_frames`, `last_paint_micros`, `last_snapshot_micros`) are named in Task 6 and read in Tasks 7 and 8; `TerminalSize` needs `PartialEq` (Task 6 Step 2 adds it if missing).
- **Placeholders:** Tasks 6-8 describe GPUI-bound code in prose plus the pure pieces in full code, because the GPUI call signatures are only trustworthy after Task 1's probe compiles; those tasks say exactly what to call and what the probe must already pin. This is a deliberate trade-off, not an omission, and the planner should expand Tasks 6-8 to full code blocks as soon as Task 1 has run and the signatures are real.
- **Open items for the owner:** decisions D1-D8; whether `rwk-app` should wait for P3 (D8); whether the executor for Tasks 6-8 needs a display (window tests are `#[ignore]` and are run by the planner in Task 10 if not earlier).
