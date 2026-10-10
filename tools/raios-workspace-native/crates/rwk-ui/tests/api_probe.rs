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
        let line = window.text_system().shape_line(
            SharedString::from("x"),
            Pixels::from(14.0),
            &runs,
            None,
        );
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
    _focus: FocusHandle,
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
