//! Hover, go to definition, completion, code actions and formatting for one editor tab, answered by the project's
//! language server. The providers only translate: positions go out, protocol values come back.

use std::rc::Rc;

use anyhow::{anyhow, Result};
use gpui::{App, Entity, Task, Window};
use gpui_base::input::{
    CodeActionProvider, CompletionProvider, DefinitionProvider, EditorState, HoverProvider, Rope,
    RopeExt,
};
use rwk_core::lsp::lsp_types::{
    CodeAction, CodeActionOrCommand, CompletionContext, CompletionResponse, DocumentChanges,
    GotoDefinitionResponse, Hover, Location, LocationLink, OneOf, Range, TextEdit,
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

impl CodeActionProvider for ServerProvider {
    fn id(&self) -> gpui::SharedString {
        "language-server".into()
    }

    fn code_actions(
        &self,
        state: Entity<EditorState>,
        range: std::ops::Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<CodeAction>>> {
        let text = state.read(cx).text().clone();
        let range = Range::new(
            text.offset_to_position(range.start),
            text.offset_to_position(range.end),
        );
        let (sender, receiver) = async_channel::bounded(1);
        self.requester.request_document(
            &self.path,
            "textDocument/codeAction",
            serde_json::json!({"range": range, "context": {"diagnostics": []}}),
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
            let items: Option<Vec<CodeActionOrCommand>> = serde_json::from_value(answer)?;
            // A bare command would need the server to run it; only edits are offered.
            Ok(items
                .unwrap_or_default()
                .into_iter()
                .filter_map(|item| match item {
                    CodeActionOrCommand::CodeAction(action) if action.edit.is_some() => {
                        Some(action)
                    }
                    _ => None,
                })
                .collect())
        })
    }

    fn perform_code_action(
        &self,
        state: Entity<EditorState>,
        action: CodeAction,
        _push_to_history: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<()>> {
        let result = edits_for_file(&action, &self.path, &self.requester).map(|edits| {
            state.update(cx, |state, cx| state.apply_lsp_edits(&edits, window, cx));
        });
        Task::ready(result)
    }
}

/// The edits of a code action that fall in the file at `path`. An action that also changes
/// other files is refused whole: applying half of a refactoring would leave the project broken.
fn edits_for_file(action: &CodeAction, path: &str, requester: &Requester) -> Result<Vec<TextEdit>> {
    let Some(edit) = &action.edit else {
        return Err(anyhow!("This action has no edits."));
    };
    let mut mine = Vec::new();
    let mut take = |uri: &str, edits: Vec<TextEdit>| -> Result<()> {
        if requester.relative_path(uri).as_deref() == Some(path) {
            mine.extend(edits);
            Ok(())
        } else {
            Err(anyhow!(
                "This action changes other files, which is not supported yet."
            ))
        }
    };
    if let Some(changes) = &edit.changes {
        for (uri, edits) in changes {
            take(&uri.to_string(), edits.clone())?;
        }
    }
    match &edit.document_changes {
        Some(DocumentChanges::Edits(documents)) => {
            for document in documents {
                let edits = document
                    .edits
                    .iter()
                    .map(|edit| match edit {
                        OneOf::Left(edit) => edit.clone(),
                        OneOf::Right(annotated) => annotated.text_edit.clone(),
                    })
                    .collect();
                take(&document.text_document.uri.to_string(), edits)?;
            }
        }
        Some(DocumentChanges::Operations(_)) => {
            return Err(anyhow!(
                "This action creates or renames files, which is not supported yet."
            ))
        }
        None => {}
    }
    rwk_core::lsp::edits::in_apply_order(mine).map_err(|error| anyhow!(error.message))
}

/// Asks the server to format the whole file; the answer is the edits, last first.
pub fn format_request(
    requester: &Requester,
    path: &str,
) -> async_channel::Receiver<rwk_core::error::Result<Value>> {
    let (sender, receiver) = async_channel::bounded(1);
    requester.request_document(
        path,
        "textDocument/formatting",
        serde_json::json!({"options": {"tabSize": 4, "insertSpaces": true}}),
        Box::new(move |answer| {
            let _ = sender.send_blocking(answer);
        }),
    );
    receiver
}

/// Reads a formatting answer into edits ready to apply.
pub fn formatting_edits(answer: Value) -> rwk_core::error::Result<Vec<TextEdit>> {
    let edits: Option<Vec<TextEdit>> = serde_json::from_value(answer).map_err(|_| {
        rwk_core::error::AppError::new(
            rwk_core::error::ErrorCode::InvalidInput,
            "The language server's formatting answer is not understood.",
        )
    })?;
    rwk_core::lsp::edits::in_apply_order(edits.unwrap_or_default())
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
    lsp.code_action_providers = vec![provider.clone()];
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
    lsp.code_action_providers.clear();
    lsp.show_document = None;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use rwk_core::lsp::lsp_types::{Position, WorkspaceEdit};
    use rwk_core::lsp::uri::path_to_uri;
    use rwk_core::lsp::{default_launcher, LanguageServers};
    use std::collections::HashMap;

    fn edit(text: &str) -> TextEdit {
        TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            new_text: text.to_owned(),
        }
    }

    #[allow(clippy::mutable_key_type)]
    fn action(files: &[(&str, &str)], root: &std::path::Path) -> CodeAction {
        let changes: HashMap<_, _> = files
            .iter()
            .map(|(path, text)| {
                (
                    path_to_uri(root, path).parse().expect("uri"),
                    vec![edit(text)],
                )
            })
            .collect();
        CodeAction {
            title: "fix".into(),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            }),
            ..CodeAction::default()
        }
    }

    #[test]
    fn a_code_action_is_applied_only_when_every_edit_is_in_this_file() {
        let dir = tempfile::tempdir().unwrap();
        let (servers, _events) = LanguageServers::new(dir.path().to_owned(), default_launcher());
        let requester = servers.requester();

        let own = edits_for_file(&action(&[("a.rs", "x")], dir.path()), "a.rs", &requester);
        assert_eq!(own.unwrap()[0].new_text, "x");

        let both = action(&[("a.rs", "x"), ("b.rs", "y")], dir.path());
        assert!(edits_for_file(&both, "a.rs", &requester).is_err());

        let no_edit = CodeAction::default();
        assert!(edits_for_file(&no_edit, "a.rs", &requester).is_err());
    }
}
