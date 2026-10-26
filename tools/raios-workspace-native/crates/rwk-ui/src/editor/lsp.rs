//! Hover, go to definition and completion for one editor tab, answered by the project's
//! language server. The providers only translate: positions go out, protocol values come back.

use std::rc::Rc;

use anyhow::{anyhow, Result};
use gpui::{App, Task, Window};
use gpui_base::input::{
    CompletionProvider, DefinitionProvider, EditorState, HoverProvider, Rope, RopeExt,
};
use rwk_core::lsp::lsp_types::{
    CompletionContext, CompletionResponse, GotoDefinitionResponse, Hover, Location, LocationLink,
};
use rwk_core::lsp::Requester;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Opens a project file at a 1-based line.
pub type OpenLocation = Rc<dyn Fn(String, usize, &mut App)>;

/// The language server of one open file.
#[derive(Clone)]
pub struct ServerProvider {
    path: String,
    requester: Requester,
}

impl ServerProvider {
    /// The provider for the file at project-relative `path`.
    pub fn new(path: &str, requester: &Requester) -> Self {
        Self {
            path: path.to_owned(),
            requester: requester.clone(),
        }
    }

    /// Asks the server `method` at `offset` of `text` and decodes the answer; `null` is `None`.
    fn ask<T: DeserializeOwned + Send + 'static>(
        &self,
        method: &'static str,
        text: &Rope,
        offset: usize,
        extra: Value,
        cx: &mut App,
    ) -> Task<Result<Option<T>>> {
        let position = text.offset_to_position(offset);
        let (sender, receiver) = async_channel::bounded(1);
        self.requester.request(
            &self.path,
            method,
            position.line,
            position.character,
            extra,
            Box::new(move |answer| {
                let _ = sender.send_blocking(answer);
            }),
        );
        cx.background_executor().spawn(async move {
            let answer = receiver
                .recv()
                .await
                .map_err(|_| anyhow!("The language server did not answer."))?
                .map_err(|error| anyhow!(error.message))?;
            if answer.is_null() {
                return Ok(None);
            }
            Ok(Some(serde_json::from_value(answer)?))
        })
    }
}

impl HoverProvider for ServerProvider {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Option<Hover>>> {
        self.ask("textDocument/hover", text, offset, Value::Null, cx)
    }
}

impl DefinitionProvider for ServerProvider {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<LocationLink>>> {
        let task = self.ask::<GotoDefinitionResponse>(
            "textDocument/definition",
            text,
            offset,
            Value::Null,
            cx,
        );
        cx.background_executor().spawn(async move {
            Ok(match task.await? {
                None => Vec::new(),
                Some(response) => links(response),
            })
        })
    }
}

impl CompletionProvider for ServerProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let context = serde_json::to_value(&trigger).unwrap_or(Value::Null);
        let task = self.ask::<CompletionResponse>(
            "textDocument/completion",
            text,
            offset,
            serde_json::json!({ "context": context }),
            cx,
        );
        cx.background_executor()
            .spawn(async move { Ok(task.await?.unwrap_or(CompletionResponse::Array(Vec::new()))) })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        new_text.ends_with('.') || new_text.ends_with(':')
    }
}

/// Every shape a definition answer can take, as links.
fn links(response: GotoDefinitionResponse) -> Vec<LocationLink> {
    let from_location = |location: Location| LocationLink {
        origin_selection_range: None,
        target_uri: location.uri,
        target_range: location.range,
        target_selection_range: location.range,
    };
    match response {
        GotoDefinitionResponse::Scalar(location) => vec![from_location(location)],
        GotoDefinitionResponse::Array(locations) => {
            locations.into_iter().map(from_location).collect()
        }
        GotoDefinitionResponse::Link(links) => links,
    }
}

/// Connects the tab's editor to the server. A definition in another project file is opened as
/// a tab (through `open`, which gets the relative path and the 1-based line); one in the same
/// file moves the cursor as the editor does by itself.
pub fn attach(state: &mut EditorState, path: &str, requester: &Requester, open: OpenLocation) {
    let provider = Rc::new(ServerProvider::new(path, requester));
    let lsp = state.lsp_mut();
    lsp.hover_provider = Some(provider.clone());
    lsp.definition_provider = Some(provider.clone());
    lsp.completion_provider = Some(provider);
    let (own, requester) = (path.to_owned(), requester.clone());
    lsp.show_document = Some(Rc::new(move |params, _window, cx| {
        let Some(target) = requester.relative_path(&params.uri.to_string()) else {
            return false;
        };
        if target == own {
            return false;
        }
        let line = params
            .selection
            .map_or(1, |range| range.start.line as usize + 1);
        open(target, line, cx);
        true
    }));
}

/// Takes the language features away again (the project has no servers).
pub fn detach(state: &mut EditorState) {
    let lsp = state.lsp_mut();
    lsp.hover_provider = None;
    lsp.definition_provider = None;
    lsp.completion_provider = None;
    lsp.show_document = None;
}
