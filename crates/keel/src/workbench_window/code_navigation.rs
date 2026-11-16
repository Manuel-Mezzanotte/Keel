//! Moving around the code with the language servers: definitions,
//! implementations and types (F12, Cmd+click), references (Shift+F12),
//! renaming (F2), the symbols of a file (Cmd+Shift+O) and of the project
//! (Cmd+T), and the edits servers make across files.

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui_component::Rope;
use gpui_component::input::{DefinitionProvider, Input, InputState, ShowDocumentHandler};
use gpui_component::{Sizable as _, WindowExt as _};
use lsp::lsp_types::{
    self, DocumentChangeOperation, DocumentChanges, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Location, LocationLink, OneOf,
    PrepareRenameResponse, ReferenceContext, ReferenceParams, RenameParams, ResourceOp, SymbolKind,
    TextDocumentIdentifier, TextDocumentPositionParams, TextEdit, WorkspaceEdit,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
    request::{
        DocumentSymbolRequest, GotoDefinition, GotoImplementation, GotoTypeDefinition,
        PrepareRenameRequest, References, Rename, WorkspaceSymbolRequest,
    },
};

use super::language_features::{LanguageFeatures, apply_text_edits};
use super::language_servers::RunningServer;
use super::problems::LineIndex;
use super::*;
use crate::quick_pick::{self, PickItem};

actions!(
    keel,
    [
        RevealDefinition,
        GoToImplementation,
        GoToTypeDefinition,
        GoToReferences,
        RenameSymbol,
        GoToSymbolInFile,
        GoToSymbolInWorkspace
    ]
);

/// Where F12 and its siblings go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Definition,
    Implementation,
    TypeDefinition,
    References,
}

/// A place in a file, as servers give them.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Place {
    pub(super) uri: String,
    pub(super) range: lsp_types::Range,
}

fn places_of(response: Option<GotoDefinitionResponse>) -> Vec<Place> {
    match response {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(location)) => vec![place(location)],
        Some(GotoDefinitionResponse::Array(locations)) => {
            locations.into_iter().map(place).collect()
        }
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Place {
                uri: super::language_servers::normalize_uri(link.target_uri.as_str()),
                range: link.target_selection_range,
            })
            .collect(),
    }
}

fn place(location: Location) -> Place {
    Place {
        uri: super::language_servers::normalize_uri(location.uri.as_str()),
        range: location.range,
    }
}

impl DefinitionProvider for LanguageFeatures {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<LocationLink>>> {
        let text = text.to_string();
        let position = workbench::position::position_of(&text, offset.min(text.len()));
        self.state.sync(&self.uri, &text);
        let running = self
            .state
            .servers_for(&self.uri)
            .into_iter()
            .find(|running| {
                running
                    .server
                    .capabilities()
                    .is_some_and(|capabilities| capabilities.definition_provider.is_some())
            });
        let (Some(running), Ok(uri)) = (running, self.uri.parse()) else {
            return Task::ready(Ok(Vec::new()));
        };
        let request = running
            .server
            .request::<GotoDefinition>(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams::new(
                    TextDocumentIdentifier::new(uri),
                    lsp_types::Position::new(position.line, position.character),
                ),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            });
        cx.background_spawn(async move {
            let places = places_of(request.await?);
            Ok(places
                .into_iter()
                .filter_map(|place| {
                    let uri = place.uri.parse().ok()?;
                    Some(LocationLink {
                        origin_selection_range: None,
                        target_uri: uri,
                        target_range: place.range,
                        target_selection_range: place.range,
                    })
                })
                .collect())
        })
    }
}

impl Workbench {
    /// Gives an editor Cmd+click on definitions, with Keel opening the
    /// file: the editor alone could only move within its own text.
    pub(super) fn install_definitions(
        &self,
        editor: &Entity<EditorState>,
        features: Rc<LanguageFeatures>,
        cx: &mut Context<Self>,
    ) {
        let workbench = cx.weak_entity();
        let show: ShowDocumentHandler = Rc::new(move |params, window, cx| {
            let uri = super::language_servers::normalize_uri(params.uri.as_str());
            let Some(range) = params.selection else {
                return false;
            };
            let workbench = workbench.clone();
            // The editor is busy with the click: open the file right after.
            window.defer(cx, move |window, cx| {
                workbench
                    .update(cx, |this, cx| {
                        this.open_lsp_range(&uri, range, OpenAs::Normal, window, cx)
                    })
                    .ok();
            });
            true
        });
        editor.update(cx, |editor, _| {
            let lsp = editor.lsp_mut();
            lsp.definition_provider = Some(features);
            lsp.show_document = Some(show);
        });
    }

    /// The servers, document and position for a request at the cursor.
    fn at_cursor(
        &self,
        cx: &App,
    ) -> Option<(
        Vec<Rc<RunningServer>>,
        TextDocumentIdentifier,
        lsp_types::Position,
    )> {
        let pane = self.panes.get(self.active)?;
        let editor = pane.editor.read(cx);
        let text = editor.value().to_string();
        let offset = editor.selected_range().start;
        self.languages.sync(&pane.uri, &text);
        let position = workbench::position::position_of(&text, offset);
        Some((
            self.languages.servers_for(&pane.uri),
            TextDocumentIdentifier::new(pane.uri.parse().ok()?),
            lsp_types::Position::new(position.line, position.character),
        ))
    }

    fn go_to_target(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let Some((servers, document, position)) = self.at_cursor(cx) else {
            return;
        };
        let supports = |running: &&Rc<RunningServer>| {
            let Some(capabilities) = running.server.capabilities() else {
                return false;
            };
            match target {
                Target::Definition => capabilities.definition_provider.is_some(),
                Target::Implementation => capabilities.implementation_provider.is_some(),
                Target::TypeDefinition => capabilities.type_definition_provider.is_some(),
                Target::References => capabilities.references_provider.is_some(),
            }
        };
        let Some(running) = servers.iter().find(supports).cloned() else {
            self.status_message = Some(self.strings().lsp_not_supported.to_owned());
            cx.notify();
            return;
        };
        let at = TextDocumentPositionParams::new(document, position);
        let server = &running.server;
        let request: std::pin::Pin<Box<dyn std::future::Future<Output = Vec<Place>>>> = match target
        {
            Target::Definition => {
                let request = server.request::<GotoDefinition>(GotoDefinitionParams {
                    text_document_position_params: at,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                });
                Box::pin(async move { places_of(request.await.ok().flatten()) })
            }
            Target::Implementation => {
                let request = server.request::<GotoImplementation>(GotoDefinitionParams {
                    text_document_position_params: at,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                });
                Box::pin(async move { places_of(request.await.ok().flatten()) })
            }
            Target::TypeDefinition => {
                let request = server.request::<GotoTypeDefinition>(GotoDefinitionParams {
                    text_document_position_params: at,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                });
                Box::pin(async move { places_of(request.await.ok().flatten()) })
            }
            Target::References => {
                let request = server.request::<References>(ReferenceParams {
                    text_document_position: at,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                });
                Box::pin(async move {
                    request
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_default()
                        .into_iter()
                        .map(place)
                        .collect()
                })
            }
        };
        cx.spawn_in(window, async move |this, cx| {
            let places = request.await;
            this.update_in(cx, |this, window, cx| {
                this.show_places(target, places, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// Goes to the only place, or lists them all to choose from.
    fn show_places(
        &mut self,
        target: Target,
        places: Vec<Place>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        match places.as_slice() {
            [] => {
                self.status_message = Some(
                    match target {
                        Target::References => strings.lsp_no_references,
                        _ => strings.lsp_no_definition,
                    }
                    .to_owned(),
                );
                cx.notify();
            }
            [only] if target != Target::References => {
                let only = only.clone();
                self.open_lsp_range(&only.uri, only.range, OpenAs::Normal, window, cx);
            }
            _ => {
                let items = self.place_items(&places);
                let title = match target {
                    Target::References => strings.lsp_references_title(places.len()),
                    _ => strings.lsp_definitions_title(places.len()),
                };
                let chosen = quick_pick::show(Some(title), None, items, window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    let Some(index) = chosen.await.ok().flatten() else {
                        return;
                    };
                    let Some(place) = places.get(index).cloned() else {
                        return;
                    };
                    this.update_in(cx, |this, window, cx| {
                        this.open_lsp_range(&place.uri, place.range, OpenAs::Normal, window, cx)
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// Each place as a list entry: the line's text, then the file and line.
    fn place_items(&self, places: &[Place]) -> Vec<PickItem> {
        let mut texts: BTreeMap<String, Option<String>> = BTreeMap::new();
        places
            .iter()
            .map(|place| {
                let text = texts
                    .entry(place.uri.clone())
                    .or_insert_with(|| self.text_of(&place.uri));
                let line = text
                    .as_deref()
                    .and_then(|text| text.lines().nth(place.range.start.line as usize))
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                let mut item = PickItem::new(if line.is_empty() {
                    place.uri.clone()
                } else {
                    line
                });
                item.detail = Some(format!(
                    "{}:{}",
                    self.shown_path(&place.uri),
                    place.range.start.line + 1
                ));
                item
            })
            .collect()
    }

    /// The text of a file: from its editor when open, else from disk.
    fn text_of(&self, uri: &str) -> Option<String> {
        if let Some(document) = self.documents.get(uri) {
            return Some(document.synced_text.clone());
        }
        let path = Uri::parse(uri)?.file_path()?;
        std::fs::read_to_string(path).ok()
    }

    /// A file's path from the project's folder, for lists.
    fn shown_path(&self, uri: &str) -> String {
        let Some(path) = Uri::parse(uri).and_then(|parsed| parsed.file_path()) else {
            return uri.to_owned();
        };
        match &self.project.root {
            Some(root) => path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned(),
            None => path.to_string_lossy().into_owned(),
        }
    }

    // -----------------------------------------------------------------
    // Rename

    fn rename_symbol(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((servers, document, position)) = self.at_cursor(cx) else {
            return;
        };
        let Some(running) = servers.into_iter().find(|running| {
            running
                .server
                .capabilities()
                .is_some_and(|capabilities| capabilities.rename_provider.is_some())
        }) else {
            self.status_message = Some(self.strings().lsp_not_supported.to_owned());
            cx.notify();
            return;
        };
        let word = self.active_editor().map(|editor| {
            let editor = editor.read(cx);
            let text = editor.value().to_string();
            let offset = editor.selected_range().start.min(text.len());
            let is_word = |character: char| {
                character.is_alphanumeric() || character == '_' || character == '$'
            };
            let start = text[..offset]
                .char_indices()
                .rev()
                .take_while(|(_, character)| is_word(*character))
                .last()
                .map_or(offset, |(at, _)| at);
            let end = text[offset..]
                .char_indices()
                .find(|(_, character)| !is_word(*character))
                .map_or(text.len(), |(at, _)| offset + at);
            text[start..end].to_owned()
        });
        let at = TextDocumentPositionParams::new(document.clone(), position);
        let prepare = running.server.request::<PrepareRenameRequest>(at.clone());
        cx.spawn_in(window, async move |this, cx| {
            let placeholder = match prepare.await {
                Ok(Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. })) => {
                    Some(placeholder)
                }
                Ok(Some(_)) | Err(_) => word,
                // The server says nothing can be renamed here.
                Ok(None) => {
                    this.update(cx, |this, cx| {
                        this.status_message = Some(this.strings().lsp_cannot_rename.to_owned());
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            this.update_in(cx, |this, window, cx| {
                this.ask_new_name(running, at, placeholder.unwrap_or_default(), window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn ask_new_name(
        &mut self,
        running: Rc<RunningServer>,
        at: TextDocumentPositionParams,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current.clone()));
        let this = cx.weak_entity();
        // The dialog takes Enter as its OK, which reads the field.
        let shown = input.clone();
        let field_for_dialog = input.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let field = field_for_dialog.clone();
            let this = this.clone();
            let running = running.clone();
            let at = at.clone();
            let current = current.clone();
            dialog
                .w(px(420.))
                .title(strings.lsp_rename_title)
                .on_ok(move |_, window, cx| {
                    let name = field.read(cx).value().trim().to_owned();
                    if name.is_empty() || name == current {
                        return true;
                    }
                    let request = running.server.request::<Rename>(RenameParams {
                        text_document_position: at.clone(),
                        new_name: name,
                        work_done_progress_params: Default::default(),
                    });
                    let this = this.clone();
                    window
                        .spawn(cx, async move |cx| {
                            let result = request.await;
                            this.update_in(cx, |this, window, cx| match result {
                                Ok(Some(edit)) => {
                                    let (edits, files) =
                                        this.apply_workspace_edit(&edit, window, cx);
                                    this.status_message =
                                        Some(this.strings().lsp_renamed(edits, files));
                                    cx.notify();
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    this.status_message = Some(error.to_string());
                                    cx.notify();
                                }
                            })
                            .ok();
                        })
                        .detach();
                    true
                })
                .child(Input::new(&shown).small())
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    /// Applies a server's edit across files: the open ones in their
    /// editors, where Cmd+Z takes it back, the others on disk. Returns how
    /// many edits and files it changed.
    pub(super) fn apply_workspace_edit(
        &mut self,
        edit: &WorkspaceEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (usize, usize) {
        let mut by_file: Vec<(String, Vec<TextEdit>)> = Vec::new();
        let mut add = |uri: &lsp_types::Uri, edits: Vec<TextEdit>| {
            let uri = super::language_servers::normalize_uri(uri.as_str());
            match by_file.iter_mut().find(|(file, _)| *file == uri) {
                Some((_, existing)) => existing.extend(edits),
                None => by_file.push((uri, edits)),
            }
        };
        let mut operations = Vec::new();
        if let Some(changes) = &edit.changes {
            for (uri, edits) in changes {
                add(uri, edits.clone());
            }
        }
        match &edit.document_changes {
            Some(DocumentChanges::Edits(documents)) => {
                for document in documents {
                    add(&document.text_document.uri, plain_edits(&document.edits));
                }
            }
            Some(DocumentChanges::Operations(changes)) => {
                for change in changes {
                    match change {
                        DocumentChangeOperation::Edit(document) => {
                            add(&document.text_document.uri, plain_edits(&document.edits))
                        }
                        DocumentChangeOperation::Op(operation) => {
                            operations.push(operation.clone())
                        }
                    }
                }
            }
            None => {}
        }
        let mut edits = 0;
        let mut files = 0;
        for (uri, file_edits) in by_file {
            if file_edits.is_empty() {
                continue;
            }
            edits += file_edits.len();
            files += 1;
            let editors: Vec<Entity<EditorState>> = self
                .panes
                .iter()
                .filter(|pane| pane.uri == uri)
                .map(|pane| pane.editor.clone())
                .take(1)
                .collect();
            if let Some(editor) = editors.first() {
                apply_text_edits(editor, &file_edits, window, cx);
                continue;
            }
            let Some(path) = Uri::parse(&uri).and_then(|parsed| parsed.file_path()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let changed = apply_to_text(&text, &file_edits);
            if let Err(error) = std::fs::write(&path, changed) {
                self.log(format!("{}: {error}", path.display()));
            }
        }
        for operation in operations {
            self.apply_resource_operation(&operation, window, cx);
        }
        (edits, files)
    }

    fn apply_resource_operation(
        &mut self,
        operation: &ResourceOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path =
            |uri: &lsp_types::Uri| Uri::parse(uri.as_str()).and_then(|parsed| parsed.file_path());
        match operation {
            ResourceOp::Create(create) => {
                if let Some(target) = path(&create.uri)
                    && !target.exists()
                {
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(&target, "");
                }
            }
            ResourceOp::Rename(rename) => {
                if let (Some(from), Some(to)) = (path(&rename.old_uri), path(&rename.new_uri))
                    && std::fs::rename(&from, &to).is_ok()
                {
                    self.path_moved(&from, &to, window, cx);
                }
            }
            ResourceOp::Delete(delete) => {
                if let Some(target) = path(&delete.uri) {
                    let _ = files::trash(&target);
                }
            }
        }
    }

    // -----------------------------------------------------------------
    // Symbols

    fn go_to_symbol_in_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((servers, document, _)) = self.at_cursor(cx) else {
            return;
        };
        let Some(running) = servers.into_iter().find(|running| {
            running
                .server
                .capabilities()
                .is_some_and(|capabilities| capabilities.document_symbol_provider.is_some())
        }) else {
            self.status_message = Some(self.strings().lsp_not_supported.to_owned());
            cx.notify();
            return;
        };
        let uri = document.uri.to_string();
        let request = running
            .server
            .request::<DocumentSymbolRequest>(DocumentSymbolParams {
                text_document: document,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            });
        cx.spawn_in(window, async move |this, cx| {
            let symbols = flatten_symbols(request.await.ok().flatten());
            this.update_in(cx, |this, window, cx| {
                let items: Vec<PickItem> = symbols
                    .iter()
                    .map(|(depth, name, kind, detail, _)| {
                        let mut item = PickItem::new(format!("{}{name}", "  ".repeat(*depth)));
                        item.detail = Some(match detail {
                            Some(detail) => format!("{} · {detail}", kind_name(*kind)),
                            None => kind_name(*kind).to_owned(),
                        });
                        item.keywords = vec![name.clone()];
                        item
                    })
                    .collect();
                let placeholder = this.strings().lsp_symbols_in_file.to_owned();
                let chosen = quick_pick::show(None, Some(placeholder), items, window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    let Some(index) = chosen.await.ok().flatten() else {
                        return;
                    };
                    let Some((_, _, _, _, range)) = symbols.get(index).cloned() else {
                        return;
                    };
                    this.update_in(cx, |this, window, cx| {
                        this.open_lsp_range(&uri, range, OpenAs::Normal, window, cx)
                    })
                    .ok();
                })
                .detach();
            })
            .ok();
        })
        .detach();
    }

    fn go_to_symbol_in_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Any open file's servers know the project.
        let servers: Vec<Rc<RunningServer>> = {
            let mut seen = Vec::new();
            for pane in &self.panes {
                for running in self.languages.servers_for(&pane.uri) {
                    let supports = running.server.capabilities().is_some_and(|capabilities| {
                        capabilities.workspace_symbol_provider.is_some()
                    });
                    if supports
                        && !seen
                            .iter()
                            .any(|known: &Rc<RunningServer>| Rc::ptr_eq(known, &running))
                    {
                        seen.push(running);
                    }
                }
            }
            seen
        };
        if servers.is_empty() {
            self.status_message = Some(self.strings().lsp_not_supported.to_owned());
            cx.notify();
            return;
        }
        let root = self.project.root.clone();
        let fetch: quick_pick::Fetch = Rc::new(move |query: String, cx: &mut App| {
            let requests: Vec<_> = servers
                .iter()
                .map(|running| {
                    running
                        .server
                        .request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
                            query: query.clone(),
                            work_done_progress_params: Default::default(),
                            partial_result_params: Default::default(),
                        })
                })
                .collect();
            let root = root.clone();
            cx.background_spawn(async move {
                let mut items = Vec::new();
                for request in requests {
                    let Ok(Some(response)) = request.await else {
                        continue;
                    };
                    items.extend(workspace_symbol_items(response, root.as_deref()));
                }
                items.truncate(200);
                items
            })
        });
        let placeholder = self.strings().lsp_symbols_in_workspace.to_owned();
        let chosen = quick_pick::show_dynamic(Some(placeholder), fetch, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(item) = chosen.await.ok().flatten() else {
                return;
            };
            let Some(place) = item.keywords.first().and_then(|encoded| {
                serde_json::from_str::<(String, lsp_types::Range)>(encoded).ok()
            }) else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.open_lsp_range(&place.0, place.1, OpenAs::Normal, window, cx)
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn code_navigation_actions(
        &self,
        root: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &RevealDefinition, window, cx| {
            this.go_to_target(Target::Definition, window, cx)
        }))
        .on_action(cx.listener(|this, _: &GoToImplementation, window, cx| {
            this.go_to_target(Target::Implementation, window, cx)
        }))
        .on_action(cx.listener(|this, _: &GoToTypeDefinition, window, cx| {
            this.go_to_target(Target::TypeDefinition, window, cx)
        }))
        .on_action(cx.listener(|this, _: &GoToReferences, window, cx| {
            this.go_to_target(Target::References, window, cx)
        }))
        .on_action(cx.listener(|this, _: &RenameSymbol, window, cx| this.rename_symbol(window, cx)))
        .on_action(cx.listener(|this, _: &GoToSymbolInFile, window, cx| {
            this.go_to_symbol_in_file(window, cx)
        }))
        .on_action(cx.listener(|this, _: &GoToSymbolInWorkspace, window, cx| {
            this.go_to_symbol_in_workspace(window, cx)
        }))
    }
}

fn plain_edits(edits: &[OneOf<TextEdit, lsp_types::AnnotatedTextEdit>]) -> Vec<TextEdit> {
    edits
        .iter()
        .map(|edit| match edit {
            OneOf::Left(edit) => edit.clone(),
            OneOf::Right(annotated) => annotated.text_edit.clone(),
        })
        .collect()
}

/// Applies edits to a text, as servers mean them: every range refers to
/// the text before any edit.
pub(super) fn apply_to_text(text: &str, edits: &[TextEdit]) -> String {
    let lines = LineIndex::new(text);
    let mut ranges: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|edit| {
            (
                lines.offset(edit.range.start),
                lines.offset(edit.range.end),
                edit.new_text.as_str(),
            )
        })
        .collect();
    ranges.sort_by_key(|(start, end, _)| (*start, *end));
    let mut result = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end, new_text) in ranges {
        if start < at {
            continue;
        }
        result.push_str(&text[at..start]);
        result.push_str(new_text);
        at = end;
    }
    result.push_str(&text[at..]);
    result
}

/// A file's symbols in order, nested ones indented: depth, name, kind,
/// detail and the range to select.
type FlatSymbol = (usize, String, SymbolKind, Option<String>, lsp_types::Range);

fn flatten_symbols(response: Option<DocumentSymbolResponse>) -> Vec<FlatSymbol> {
    fn walk(symbols: &[lsp_types::DocumentSymbol], depth: usize, out: &mut Vec<FlatSymbol>) {
        for symbol in symbols {
            out.push((
                depth,
                symbol.name.clone(),
                symbol.kind,
                symbol.detail.clone().filter(|detail| !detail.is_empty()),
                symbol.selection_range,
            ));
            if let Some(children) = &symbol.children {
                walk(children, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    match response {
        Some(DocumentSymbolResponse::Nested(symbols)) => walk(&symbols, 0, &mut out),
        Some(DocumentSymbolResponse::Flat(symbols)) => {
            #[allow(deprecated)]
            out.extend(symbols.into_iter().map(|symbol| {
                (
                    usize::from(symbol.container_name.is_some()),
                    symbol.name,
                    symbol.kind,
                    None,
                    symbol.location.range,
                )
            }));
            out.sort_by_key(|(_, _, _, _, range)| range.start);
        }
        None => {}
    }
    out
}

/// The project's symbols as list entries; the place goes in the first
/// keyword, to come back when one is chosen.
fn workspace_symbol_items(response: WorkspaceSymbolResponse, root: Option<&Path>) -> Vec<PickItem> {
    let symbols: Vec<(String, SymbolKind, Option<String>, String, lsp_types::Range)> =
        match response {
            WorkspaceSymbolResponse::Flat(symbols) => symbols
                .into_iter()
                .map(|symbol| {
                    #[allow(deprecated)]
                    let container = symbol.container_name;
                    (
                        symbol.name,
                        symbol.kind,
                        container,
                        super::language_servers::normalize_uri(symbol.location.uri.as_str()),
                        symbol.location.range,
                    )
                })
                .collect(),
            WorkspaceSymbolResponse::Nested(symbols) => symbols
                .into_iter()
                .filter_map(|symbol| {
                    let OneOf::Left(location) = symbol.location else {
                        return None;
                    };
                    Some((
                        symbol.name,
                        symbol.kind,
                        symbol.container_name,
                        super::language_servers::normalize_uri(location.uri.as_str()),
                        location.range,
                    ))
                })
                .collect(),
        };
    symbols
        .into_iter()
        .map(|(name, kind, container, uri, range)| {
            let path = Uri::parse(&uri)
                .and_then(|parsed| parsed.file_path())
                .map(|path| match root {
                    Some(root) => path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                    None => path.to_string_lossy().into_owned(),
                })
                .unwrap_or_else(|| uri.clone());
            let mut item = PickItem::new(name);
            item.detail = Some(match container {
                Some(container) if !container.is_empty() => {
                    format!("{} · {container} · {path}", kind_name(kind))
                }
                _ => format!("{} · {path}", kind_name(kind)),
            });
            item.keywords = vec![serde_json::to_string(&(uri, range)).unwrap_or_default()];
            item
        })
        .collect()
}

/// VS Code's names for symbol kinds.
fn kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::FILE => "file",
        SymbolKind::MODULE => "module",
        SymbolKind::NAMESPACE => "namespace",
        SymbolKind::PACKAGE => "package",
        SymbolKind::CLASS => "class",
        SymbolKind::METHOD => "method",
        SymbolKind::PROPERTY => "property",
        SymbolKind::FIELD => "field",
        SymbolKind::CONSTRUCTOR => "constructor",
        SymbolKind::ENUM => "enum",
        SymbolKind::INTERFACE => "interface",
        SymbolKind::FUNCTION => "function",
        SymbolKind::VARIABLE => "variable",
        SymbolKind::CONSTANT => "constant",
        SymbolKind::STRING => "string",
        SymbolKind::NUMBER => "number",
        SymbolKind::BOOLEAN => "boolean",
        SymbolKind::ARRAY => "array",
        SymbolKind::OBJECT => "object",
        SymbolKind::KEY => "key",
        SymbolKind::NULL => "null",
        SymbolKind::ENUM_MEMBER => "enum member",
        SymbolKind::STRUCT => "struct",
        SymbolKind::EVENT => "event",
        SymbolKind::OPERATOR => "operator",
        SymbolKind::TYPE_PARAMETER => "type parameter",
        _ => "symbol",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextEdit {
        TextEdit::new(
            lsp_types::Range::new(
                lsp_types::Position::new(start.0, start.1),
                lsp_types::Position::new(end.0, end.1),
            ),
            text.to_owned(),
        )
    }

    #[test]
    fn edits_refer_to_the_text_before_them() {
        let text = "const nome = 1;\nconsole.log(nome, nome);\n";
        let renamed = apply_to_text(
            text,
            &[
                edit((1, 18), (1, 22), "titolo"),
                edit((0, 6), (0, 10), "titolo"),
                edit((1, 12), (1, 16), "titolo"),
            ],
        );
        assert_eq!(renamed, "const titolo = 1;\nconsole.log(titolo, titolo);\n");
        // Columns are UTF-16: the accent counts one.
        assert_eq!(apply_to_text("è x", &[edit((0, 2), (0, 3), "y")]), "è y");
    }

    #[test]
    fn nested_symbols_are_indented_in_order() {
        #[allow(deprecated)]
        let symbol = |name: &str, line: u32, children: Option<Vec<lsp_types::DocumentSymbol>>| {
            lsp_types::DocumentSymbol {
                name: name.to_owned(),
                detail: None,
                kind: SymbolKind::FUNCTION,
                tags: None,
                deprecated: None,
                range: lsp_types::Range::new(
                    lsp_types::Position::new(line, 0),
                    lsp_types::Position::new(line, 5),
                ),
                selection_range: lsp_types::Range::new(
                    lsp_types::Position::new(line, 0),
                    lsp_types::Position::new(line, 5),
                ),
                children,
            }
        };
        let flat = flatten_symbols(Some(DocumentSymbolResponse::Nested(vec![
            symbol("Card", 0, Some(vec![symbol("render", 1, None)])),
            symbol("helper", 9, None),
        ])));
        let names: Vec<(usize, &str)> = flat
            .iter()
            .map(|(depth, name, ..)| (*depth, name.as_str()))
            .collect();
        assert_eq!(names, [(0, "Card"), (1, "render"), (0, "helper")]);
    }
}

#[cfg(test)]
mod window_tests {
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        std::fs::write(
            base.join("saluti.ts"),
            "// utilità\nexport function saluta() {}\n",
        )
        .unwrap();
        std::fs::write(
            base.join("app.ts"),
            "import { saluta } from './saluti';\nsaluta();\n",
        )
        .unwrap();
        std::fs::write(base.join("chiuso.ts"), "const chiuso = 1;\n").unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(opening, settings, None, window, cx);
            workbench.languages.fake = true;
            workbench
        });
        for name in ["saluti.ts", "app.ts"] {
            let path = base.join(name);
            view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        }
        let app = Uri::file(&base.join("app.ts")).to_string();
        wait_until(cx, "the server to know both files", |cx| {
            view.read_with(cx, |view, _| view.languages.servers_for(&app).len() == 1)
        });
        (folder, base, view, cx)
    }

    #[gpui::test]
    fn f12_opens_the_definition_in_another_file(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        // The cursor on "saluta()" in app.ts.
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| editor.set_selected_range(37..37, cx));
            view.go_to_target(Target::Definition, window, cx);
        });
        wait_until(cx, "saluti.ts in front", |cx| {
            view.read_with(cx, |view, _| {
                view.panes[view.active].uri.ends_with("saluti.ts")
            })
        });
        view.read_with(cx, |view, cx| {
            let editor = view.active_editor().unwrap().read(cx);
            let text = editor.value().to_string();
            assert_eq!(&text[editor.selected_range()], "saluta");
        });
    }

    #[gpui::test]
    fn server_edits_reach_open_and_closed_files(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        let app = Uri::file(&base.join("app.ts")).to_string();
        // A server asks to edit an open file.
        view.read_with(cx, |view, _| {
            let running = view.languages.servers_for(&app);
            running[0].server.notify_raw(
                "fake/applyEdit",
                json!({ "uri": app, "text": "'use client';\n" }),
            );
        });
        wait_until(cx, "the edit in the editor", |cx| {
            view.read_with(cx, |view, cx| {
                view.active_editor()
                    .unwrap()
                    .read(cx)
                    .value()
                    .starts_with("'use client';\n")
            })
        });
        // A rename touching a file that is not open changes it on disk.
        let closed = Uri::file(&base.join("chiuso.ts")).to_string();
        let edit: WorkspaceEdit = serde_json::from_value(json!({
            "changes": { closed: [{
                "range": { "start": { "line": 0, "character": 6 }, "end": { "line": 0, "character": 12 } },
                "newText": "aperto",
            }] },
        }))
        .unwrap();
        let counted = view.update_in(cx, |view, window, cx| {
            view.apply_workspace_edit(&edit, window, cx)
        });
        assert_eq!(counted, (1, 1));
        assert_eq!(
            std::fs::read_to_string(base.join("chiuso.ts")).unwrap(),
            "const aperto = 1;\n"
        );
    }
}
