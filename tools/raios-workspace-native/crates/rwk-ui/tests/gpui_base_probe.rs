//! Compile-time pin of the gpui-base 0.7.1 surface Plan 5 uses. If a bump changes one of these,
//! this file stops compiling and the plan's tasks must be re-read before the bump is accepted.
//! The functions are never called: compiling them is the test.
#![allow(dead_code)]

use std::rc::Rc;

use gpui::{px, App, AppContext, Context, Entity, PathPromptOptions, SharedString, Window};
use gpui_base::input::{
    Editor, EditorState, InputEvent, InputHighlighter, InputHighlighterFactory,
};
use gpui_base::Root;
use gpui_base::{h_resizable, resizable_panel};
use gpui_base::{Tree, TreeEvent, TreeItem, TreeState};

fn editor_surface(window: &mut Window, cx: &mut Context<EditorState>) {
    // Builders: soft wrap off, line numbers and folding on, language by name.
    let mut state = EditorState::new(window, cx)
        .soft_wrap(false)
        .line_number(true)
        .folding(true)
        .language("rust");
    let _text: SharedString = state.value();
    state.set_value("fn main() {}", window, cx);
    state.set_disabled(true, cx);
    let factory: InputHighlighterFactory =
        Rc::new(|_language: &str| -> Option<Box<dyn InputHighlighter>> { None });
    state.set_highlighter_factory(factory, cx);
    let _ = InputEvent::Change;
}

fn editor_element(state: &Entity<EditorState>) -> Editor {
    Editor::new(state)
}

fn tree_surface(cx: &mut App) {
    let item = TreeItem::new("src", "src")
        .child(TreeItem::new("src/lib.rs", "lib.rs"))
        .expanded(true);
    let state = cx.new(|cx| TreeState::new(cx).items(vec![item]));
    let _element = Tree::new(&state);
    let _ = (
        TreeEvent::Expanded("a".into()),
        TreeEvent::Collapsed("a".into()),
    );
    state.update(cx, |tree, cx| {
        tree.set_items(vec![TreeItem::new("x", "x")], cx);
        let _selected = tree.selected_item().map(|item| item.id.clone());
    });
}

fn resizable_surface() {
    let _group = h_resizable("workbench").child(resizable_panel().size(px(240.0)));
}

fn startup_surface(cx: &mut App) {
    gpui_base::init(cx);
    let _receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Proje klasörü seç".into()),
    });
}

fn root_surface(window: &mut Window, cx: &mut App, view: gpui::AnyView) {
    let _root = cx.new(|cx| Root::new(view, window, cx));
}

#[test]
fn the_gpui_base_surface_this_plan_uses_still_compiles() {}
