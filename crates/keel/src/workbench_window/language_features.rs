//! What the language servers give the editor while you write: completions
//! (with snippets and automatic imports), the VS Code snippets of the user
//! and of extensions, and the information shown on hover.
//!
//! gpui-component asks a provider for completions and inserts the item
//! you pick on its own. Keel recognizes the item from the text it inserted,
//! then does what gpui-component cannot: moves through the snippet's tab
//! stops and adds the item's extra edits, such as an import.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::time::SystemTime;

use gpui::EntityId;
use gpui_component::Rope;
use gpui_component::input::{
    CompletionProvider, HoverProvider, Position as EditorPosition, highlight_characters,
};
use lsp::lsp_types::{
    self, CompletionContext as LspCompletionContext, CompletionItem, CompletionItemKind,
    CompletionParams, CompletionResponse, CompletionTextEdit, CompletionTriggerKind, Hover,
    HoverParams, InsertTextFormat, TextDocumentIdentifier, TextDocumentPositionParams, TextEdit,
    request::{Completion, HoverRequest, ResolveCompletionItem},
};
use lsp::snippet::{self, Snippet};

use super::language_servers::{LanguageState, RunningServer};
use super::problems::LineIndex;
use super::*;

actions!(keel, [NextSnippetStop, PreviousSnippetStop]);

/// The most items the completion menu shows.
const MAX_ITEMS: usize = 200;

/// An item the menu offered, and what inserting it means.
#[derive(Clone)]
struct Candidate {
    /// The text the editor inserts, snippets already expanded.
    inserted: String,
    /// Where it goes, as a byte offset in the text the request saw.
    start: usize,
    snippet: Snippet,
    item: CompletionItem,
    server: Option<Rc<RunningServer>>,
}

/// The completions offered last in one editor.
#[derive(Default)]
pub(super) struct CompletionSession {
    candidates: Vec<Candidate>,
    /// The item the keyboard has selected in the menu, followed from the
    /// arrow keys: gpui-component does not say.
    selected: usize,
    revision: Option<u64>,
    /// Enter or Tab was pressed with the menu open.
    confirming: bool,
}

/// A snippet being filled in: where Tab goes next.
pub(super) struct ActiveSnippet {
    editor: EntityId,
    /// Byte ranges in the editor's text, by tab stop.
    stops: Vec<Vec<Range<usize>>>,
    current: usize,
}

/// A snippet from the user's snippet files or an extension.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SnippetEntry {
    pub(super) name: String,
    pub(super) prefixes: Vec<String>,
    pub(super) body: String,
    pub(super) description: Option<String>,
}

/// A snippet file's snippets, each with the languages it is limited to.
type SnippetList = Rc<Vec<(Vec<String>, SnippetEntry)>>;

/// The snippet files read so far, with when they were last changed.
#[derive(Default)]
pub(super) struct SnippetFiles {
    /// Keel's data folder, where the user's `snippets/` are.
    pub(super) data_dir: Option<PathBuf>,
    /// The snippet files extensions contribute, by language, read once.
    extension_files: Option<Vec<(String, PathBuf)>>,
    files: HashMap<PathBuf, (Option<SystemTime>, SnippetList)>,
}

/// The bridge between one editor and the language servers.
pub(super) struct LanguageFeatures {
    pub(super) workbench: gpui::WeakEntity<Workbench>,
    pub(super) offered: super::code_actions::Offered,
    pub(super) state: Rc<LanguageState>,
    pub(super) snippet_files: Rc<RefCell<SnippetFiles>>,
    pub(super) uri: String,
    pub(super) language_id: String,
    pub(super) extension: Option<ExtensionCompletions>,
    pub(super) session: Rc<RefCell<CompletionSession>>,
}

impl LanguageFeatures {
    /// The running servers for this file, with the text synced first.
    fn servers(&self, text: &str) -> Vec<Rc<RunningServer>> {
        self.state.sync(&self.uri, text);
        self.state.servers_for(&self.uri)
    }

    fn document(&self) -> Option<TextDocumentIdentifier> {
        self.uri.parse().ok().map(TextDocumentIdentifier::new)
    }
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

/// Where the word before `offset` starts.
fn word_start(text: &str, offset: usize) -> usize {
    text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, character)| is_word_character(*character))
        .last()
        .map_or(offset, |(at, _)| at)
}

fn lsp_position(text: &str, offset: usize) -> lsp_types::Position {
    let position = workbench::position::position_of(text, offset);
    lsp_types::Position::new(position.line, position.character)
}

/// How well `label` matches what was typed: `None` when it does not.
/// The typed letters must appear in order; a match at the start counts most.
fn match_score(query: &str, label: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    if label.starts_with(query) {
        return Some(4);
    }
    let lower_label = label.to_lowercase();
    let lower_query = query.to_lowercase();
    if lower_label.starts_with(&lower_query) {
        return Some(3);
    }
    let mut letters = lower_label.chars();
    let in_order = lower_query
        .chars()
        .all(|wanted| letters.any(|letter| letter == wanted));
    if !in_order {
        return None;
    }
    let first = lower_query.chars().next();
    Some(if lower_label.chars().next() == first {
        2
    } else {
        1
    })
}

impl CompletionProvider for LanguageFeatures {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let text = text.to_string();
        let offset = offset.min(text.len());
        let start = word_start(&text, offset);
        let query = text[start..offset].to_owned();
        let position = lsp_position(&text, offset);
        let before = text[..start].chars().next_back();
        let servers = self.servers(&text);
        let requests: Vec<_> = servers
            .iter()
            .filter_map(|running| {
                let provider = running
                    .server
                    .capabilities()?
                    .completion_provider
                    .as_ref()?;
                let triggers = provider.trigger_characters.clone().unwrap_or_default();
                let trigger_character = before
                    .filter(|_| query.is_empty())
                    .map(String::from)
                    .filter(|character| triggers.contains(character));
                let context = LspCompletionContext {
                    trigger_kind: if trigger_character.is_some() {
                        CompletionTriggerKind::TRIGGER_CHARACTER
                    } else {
                        CompletionTriggerKind::INVOKED
                    },
                    trigger_character,
                };
                let request = running.server.request::<Completion>(CompletionParams {
                    text_document_position: TextDocumentPositionParams::new(
                        self.document()?,
                        position,
                    ),
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: Some(context),
                });
                Some((running.clone(), request))
            })
            .collect();
        let extension = self.extension.as_ref().map(|extension| {
            extension.completions(&Rope::from(text.as_str()), offset, trigger, window, cx)
        });
        let snippets = if query.is_empty() {
            Vec::new()
        } else {
            self.snippet_files
                .borrow_mut()
                .snippets_for(&self.language_id)
        };
        let file_name = Uri::parse(&self.uri)
            .and_then(|uri| uri.file_path())
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let session = self.session.clone();
        cx.spawn(async move |_| {
            let mut found: Vec<(CompletionItem, Option<Rc<RunningServer>>)> = Vec::new();
            for (running, request) in requests {
                match request.await {
                    Ok(Some(CompletionResponse::Array(items))) => {
                        found.extend(items.into_iter().map(|item| (item, Some(running.clone()))));
                    }
                    Ok(Some(CompletionResponse::List(list))) => {
                        found.extend(
                            list.items
                                .into_iter()
                                .map(|item| (item, Some(running.clone()))),
                        );
                    }
                    _ => {}
                }
            }
            if let Some(extension) = extension
                && let Ok(response) = extension.await
            {
                let items = match response {
                    CompletionResponse::Array(items) => items,
                    CompletionResponse::List(list) => list.items,
                };
                found.extend(items.into_iter().map(|item| (item, None)));
            }
            for entry in snippets {
                for prefix in &entry.prefixes {
                    found.push((
                        CompletionItem {
                            label: prefix.clone(),
                            kind: Some(CompletionItemKind::SNIPPET),
                            detail: Some(
                                entry
                                    .description
                                    .clone()
                                    .unwrap_or_else(|| entry.name.clone()),
                            ),
                            insert_text: Some(entry.body.clone()),
                            insert_text_format: Some(InsertTextFormat::SNIPPET),
                            sort_text: Some(format!("~{prefix}")),
                            ..Default::default()
                        },
                        None,
                    ));
                }
            }
            let (items, candidates) =
                prepare_items(found, &text, start, offset, &query, &file_name);
            let mut session = session.borrow_mut();
            session.candidates = candidates;
            session.selected = 0;
            session.revision = None;
            Ok(CompletionResponse::Array(items))
        })
    }

    fn is_completion_trigger(&self, offset: usize, new_text: &str, cx: &mut App) -> bool {
        let _ = offset;
        let mut characters = new_text.chars();
        let (Some(character), None) = (characters.next(), characters.next()) else {
            return false;
        };
        if is_word_character(character) {
            return true;
        }
        let typed = character.to_string();
        if self
            .extension
            .as_ref()
            .is_some_and(|extension| extension.triggers.contains(&typed))
        {
            return true;
        }
        let _ = cx;
        self.state
            .servers_for(&self.uri)
            .iter()
            .filter_map(|running| running.server.capabilities()?.completion_provider.clone())
            .any(|provider| {
                provider
                    .trigger_characters
                    .is_some_and(|triggers| triggers.contains(&typed))
            })
    }
}

/// Turns the servers' items into what the menu shows: snippets expanded,
/// ranges in the editor's columns, filtered by what was typed and sorted.
fn prepare_items(
    found: Vec<(CompletionItem, Option<Rc<RunningServer>>)>,
    text: &str,
    word_start: usize,
    offset: usize,
    query: &str,
    file_name: &str,
) -> (Vec<CompletionItem>, Vec<Candidate>) {
    let lines = LineIndex::new(text);
    let line_start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let line_end = text[offset..]
        .find('\n')
        .map_or(text.len(), |at| offset + at);
    let variables = snippet::variables(file_name, "", &text[line_start..line_end], None);
    let mut scored: Vec<(u32, String, CompletionItem, Candidate)> = found
        .into_iter()
        .filter_map(|(item, server)| {
            let filter = item.filter_text.as_deref().unwrap_or(&item.label);
            let score = match_score(query, filter)?;
            let (new_text, range) = match &item.text_edit {
                Some(CompletionTextEdit::Edit(edit)) => (
                    edit.new_text.clone(),
                    lines.offset(edit.range.start)..lines.offset(edit.range.end),
                ),
                Some(CompletionTextEdit::InsertAndReplace(edit)) => (
                    edit.new_text.clone(),
                    lines.offset(edit.insert.start)..lines.offset(edit.insert.end),
                ),
                None => (
                    item.insert_text
                        .clone()
                        .unwrap_or_else(|| item.label.clone()),
                    word_start..offset,
                ),
            };
            let snippet = if item.insert_text_format == Some(InsertTextFormat::SNIPPET) {
                Snippet::parse(&new_text, &variables)
            } else {
                Snippet::plain(&new_text)
            };
            let editor_range = lsp_types::Range::new(
                lines.editor_position_of(range.start),
                lines.editor_position_of(range.end),
            );
            let shown = CompletionItem {
                text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                    editor_range,
                    snippet.text.clone(),
                ))),
                insert_text: None,
                insert_text_format: None,
                additional_text_edits: None,
                command: None,
                data: None,
                ..item.clone()
            };
            let sort = item.sort_text.clone().unwrap_or_else(|| item.label.clone());
            let candidate = Candidate {
                inserted: snippet.text.clone(),
                start: range.start,
                snippet,
                item,
                server,
            };
            Some((score, sort, shown, candidate))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.truncate(MAX_ITEMS);
    scored
        .into_iter()
        .map(|(_, _, shown, candidate)| (shown, candidate))
        .unzip()
}

impl HoverProvider for LanguageFeatures {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Option<Hover>>> {
        let text = text.to_string();
        let position = lsp_position(&text, offset.min(text.len()));
        let Some(document) = self.document() else {
            return Task::ready(Ok(None));
        };
        let requests: Vec<_> = self
            .servers(&text)
            .into_iter()
            .filter(|running| {
                running
                    .server
                    .capabilities()
                    .is_some_and(|capabilities| capabilities.hover_provider.is_some())
            })
            .map(|running| {
                running.server.request::<HoverRequest>(HoverParams {
                    text_document_position_params: TextDocumentPositionParams::new(
                        document.clone(),
                        position,
                    ),
                    work_done_progress_params: Default::default(),
                })
            })
            .collect();
        cx.background_spawn(async move {
            for request in requests {
                if let Ok(Some(mut hover)) = request.await {
                    // The editor counts columns in characters.
                    let lines = LineIndex::new(&text);
                    hover.range = hover.range.map(|range| {
                        lsp_types::Range::new(
                            lines.editor_position(range.start),
                            lines.editor_position(range.end),
                        )
                    });
                    return Ok(Some(hover));
                }
            }
            Ok(None)
        })
    }
}

impl Workbench {
    /// Gives a pane's editor the language features: completions from the
    /// servers, snippets and extensions, and hover.
    pub(super) fn install_language_features(
        &mut self,
        index: usize,
        extension: Option<ExtensionCompletions>,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.panes.get(index) else {
            return;
        };
        let Some(document) = self.documents.get(&pane.uri) else {
            return;
        };
        let file = matches!(document.backing, Backing::File(_));
        if !file && extension.is_none() {
            return;
        }
        let features = Rc::new(LanguageFeatures {
            workbench: cx.weak_entity(),
            offered: Default::default(),
            state: self.languages.state.clone(),
            snippet_files: self.snippet_files.clone(),
            uri: pane.uri.clone(),
            language_id: document.language_id.clone(),
            extension,
            session: pane.completions.clone(),
        });
        pane.editor.update(cx, |editor, cx| {
            let lsp = editor.lsp_mut();
            lsp.completion_provider = Some(features.clone());
            if file {
                lsp.hover_provider = Some(features.clone());
                lsp.code_action_providers = vec![features.clone()];
            }
            lsp.set_completion_highlighter(Rc::new(highlight_characters));
            cx.notify();
        });
        if file {
            let editor = pane.editor.clone();
            self.install_definitions(&editor, features, cx);
        }
    }

    // -----------------------------------------------------------------
    // After a completion

    /// Called as the text of pane `index` changes from `old` to `new`:
    /// recognizes an inserted completion and finishes it.
    pub(super) fn after_text_change(
        &mut self,
        index: usize,
        old: &str,
        new: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.shift_snippet(index, old, new, cx);
        let Some(pane) = self.panes.get(index) else {
            return;
        };
        let candidate = {
            let mut session = pane.completions.borrow_mut();
            let Some(change) = lsp::text::content_change(old, new) else {
                return;
            };
            let typed_one = change.text.chars().count() <= 1
                && change.range.is_some_and(|range| range.start == range.end);
            let confirming = std::mem::take(&mut session.confirming);
            if session.candidates.is_empty() || (typed_one && !confirming) {
                return;
            }
            let end = LineIndex::new(old)
                .offset(change.range.map(|range| range.start).unwrap_or_default())
                + change.text.len();
            let matches: Vec<usize> = session
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    let stop = candidate.start + candidate.inserted.len();
                    stop == end
                        && new.get(candidate.start..stop) == Some(candidate.inserted.as_str())
                })
                .map(|(at, _)| at)
                .collect();
            let chosen = if matches.contains(&session.selected) {
                Some(session.selected)
            } else {
                matches.first().copied()
            };
            let candidate = chosen.map(|at| session.candidates[at].clone());
            if candidate.is_some() {
                session.candidates.clear();
            }
            candidate
        };
        let Some(candidate) = candidate else { return };
        let editor = pane.editor.clone();
        if candidate.snippet.has_stops() {
            self.start_snippet(&editor, &candidate, window, cx);
        }
        self.finish_completion(&editor, candidate, window, cx);
    }

    /// Adds an item's extra edits, such as an automatic import, asking the
    /// server for them first when the item does not carry them yet.
    fn finish_completion(
        &mut self,
        editor: &Entity<EditorState>,
        candidate: Candidate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let follow_up = candidate
            .item
            .command
            .as_ref()
            .map(|command| command.command.clone());
        if let Some(edits) = candidate.item.additional_text_edits.clone() {
            apply_text_edits(editor, &edits, window, cx);
        } else if let Some(running) = candidate.server.clone().filter(|running| {
            running
                .server
                .capabilities()
                .and_then(|capabilities| capabilities.completion_provider.as_ref())
                .and_then(|provider| provider.resolve_provider)
                .unwrap_or(false)
        }) {
            let request = running
                .server
                .request::<ResolveCompletionItem>(candidate.item.clone());
            let editor = editor.downgrade();
            cx.spawn_in(window, async move |_, cx| {
                let Ok(resolved) = request.await else { return };
                let Some(edits) = resolved.additional_text_edits else {
                    return;
                };
                if let Some(editor) = editor.upgrade() {
                    cx.update(|window, cx| apply_text_edits(&editor, &edits, window, cx))
                        .ok();
                }
            })
            .detach();
        }
        // Function completions ask to show the parameters next.
        if follow_up.as_deref() == Some("editor.action.triggerParameterHints") {
            self.request_signature_help(true, window, cx);
        }
    }

    /// Follows the completion menu's selection from the keyboard; called
    /// for every keystroke before it reaches the editor.
    pub(super) fn track_completion_keys(&mut self, keystroke: &gpui::Keystroke, cx: &App) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let menu = pane.editor.read(cx).completion_menu_state();
        if !menu.open {
            return;
        }
        let (revision, count) = (menu.revision(), menu.items.len());
        let mut session = pane.completions.borrow_mut();
        if session.revision != Some(revision) {
            session.revision = Some(revision);
            session.selected = 0;
        }
        if keystroke.modifiers.modified() && !keystroke.modifiers.control {
            return;
        }
        let key = keystroke.key.as_str();
        match (key, keystroke.modifiers.control) {
            ("down", false) | ("n", true) if count > 0 => {
                session.selected = (session.selected + 1) % count;
            }
            ("up", false) | ("p", true) if count > 0 => {
                session.selected = (session.selected + count - 1) % count;
            }
            ("enter" | "tab", false) => session.confirming = true,
            _ => {}
        }
    }

    // -----------------------------------------------------------------
    // Snippets

    fn start_snippet(
        &mut self,
        editor: &Entity<EditorState>,
        candidate: &Candidate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stops: Vec<Vec<Range<usize>>> = candidate
            .snippet
            .stops
            .iter()
            .map(|ranges| {
                ranges
                    .iter()
                    .map(|range| candidate.start + range.start..candidate.start + range.end)
                    .collect()
            })
            .collect();
        self.snippet = Some(ActiveSnippet {
            editor: editor.entity_id(),
            stops,
            current: 0,
        });
        self.select_snippet_stop(editor, window, cx);
    }

    fn select_snippet_stop(
        &mut self,
        editor: &Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snippet) = &self.snippet else { return };
        let Some(range) = snippet
            .stops
            .get(snippet.current)
            .and_then(|ranges| ranges.first())
            .cloned()
        else {
            return;
        };
        let last = snippet.current + 1 == snippet.stops.len();
        editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let end = range.end.min(text.len());
            let start = range.start.min(end);
            let position = workbench::position::position_of(&text, start);
            let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
            let column = text[line_start..start].chars().count() as u32;
            let _ = position;
            editor.set_cursor_position(EditorPosition::new(position.line, column), window, cx);
            editor.set_selected_range(start..end, cx);
        });
        // The final stop ends the snippet.
        if last {
            self.snippet = None;
        }
    }

    /// Tab and Shift+Tab inside a snippet. Returns whether the key was used.
    pub(super) fn move_in_snippet(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pane) = self.panes.get(self.active) else {
            return false;
        };
        let editor = pane.editor.clone();
        let Some(snippet) = &mut self.snippet else {
            return false;
        };
        if snippet.editor != editor.entity_id() {
            return false;
        }
        // Only while the cursor is still inside the snippet.
        let cursor = editor.read(cx).selected_range();
        let span_start = snippet
            .stops
            .iter()
            .flatten()
            .map(|range| range.start)
            .min();
        let span_end = snippet.stops.iter().flatten().map(|range| range.end).max();
        if span_start.is_none_or(|start| cursor.start < start)
            || span_end.is_none_or(|end| cursor.end > end)
        {
            self.snippet = None;
            return false;
        }
        if forward {
            snippet.current += 1;
        } else if snippet.current == 0 {
            return true;
        } else {
            snippet.current -= 1;
        }
        self.select_snippet_stop(&editor, window, cx);
        true
    }

    /// Keeps the tab stops on their text while it is edited.
    fn shift_snippet(&mut self, index: usize, old: &str, new: &str, cx: &App) {
        let Some(snippet) = &mut self.snippet else {
            return;
        };
        let Some(pane) = self.panes.get(index) else {
            return;
        };
        if pane.editor.entity_id() != snippet.editor {
            return;
        }
        let Some(change) = lsp::text::content_change(old, new) else {
            return;
        };
        let Some(range) = change.range else {
            self.snippet = None;
            return;
        };
        let lines = LineIndex::new(old);
        let (start, end) = (lines.offset(range.start), lines.offset(range.end));
        let delta = change.text.len() as isize - (end - start) as isize;
        let current = snippet.current;
        let shift = |at: usize| (at as isize + delta).max(0) as usize;
        let mut broken = false;
        for (stop, ranges) in snippet.stops.iter_mut().enumerate() {
            for range in ranges.iter_mut() {
                if end <= range.start && !(start == range.start && stop == current) {
                    // Entirely after the edit.
                    *range = shift(range.start)..shift(range.end);
                } else if start >= range.start && end <= range.end {
                    // Inside this stop: it grows or shrinks.
                    range.end = shift(range.end);
                } else if start >= range.end {
                    // Entirely before the edit: unchanged.
                } else {
                    broken = true;
                }
            }
        }
        let _ = cx;
        if broken {
            self.snippet = None;
        }
    }

    pub(super) fn snippet_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &NextSnippetStop, window, cx| {
            this.move_in_snippet(true, window, cx);
        }))
        .on_action(cx.listener(|this, _: &PreviousSnippetStop, window, cx| {
            this.move_in_snippet(false, window, cx);
        }))
    }
}

impl SnippetFiles {
    /// The snippets for `language_id`: the user's (`snippets/` in Keel's
    /// data folder, in VS Code's format) and the extensions'.
    pub(super) fn snippets_for(&mut self, language_id: &str) -> Vec<SnippetEntry> {
        let Some(data) = &self.data_dir else {
            return Vec::new();
        };
        let mut files: Vec<(PathBuf, Option<String>)> = Vec::new();
        let user = data.join("snippets");
        files.push((user.join(format!("{language_id}.json")), None));
        if let Ok(entries) = std::fs::read_dir(&user) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "code-snippets")
                {
                    files.push((path, None));
                }
            }
        }
        let data = data.clone();
        let contributed = self.extension_files.get_or_insert_with(|| {
            crate::paths::installed_extensions(&data)
                .into_iter()
                .flat_map(|extension| {
                    let snippets = extension.manifest.raw()["contributes"]["snippets"].clone();
                    snippets
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(move |snippet| {
                            Some((
                                snippet["language"].as_str()?.to_owned(),
                                extension.root.join(snippet["path"].as_str()?),
                            ))
                        })
                })
                .collect()
        });
        files.extend(
            contributed
                .iter()
                .filter(|(language, _)| language == language_id)
                .map(|(language, path)| (path.clone(), Some(language.clone()))),
        );
        let cache = self;
        let mut snippets = Vec::new();
        for (path, _) in files {
            let modified = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok();
            if modified.is_none() {
                continue;
            }
            let fresh = cache
                .files
                .get(&path)
                .is_some_and(|(seen, _)| *seen == modified);
            if !fresh {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                cache
                    .files
                    .insert(path.clone(), (modified, Rc::new(parse_snippet_file(&text))));
            }
            let (_, entries) = &cache.files[&path];
            snippets.extend(
                entries
                    .iter()
                    .filter(|(scopes, _)| {
                        scopes.is_empty() || scopes.iter().any(|scope| scope == language_id)
                    })
                    .map(|(_, entry)| entry.clone()),
            );
        }
        snippets
    }
}

/// Reads a VS Code snippet file: `{ "Name": { "prefix", "body", "description", "scope" } }`,
/// where `body` may be a list of lines. Returns each snippet with the
/// languages its `scope` limits it to.
pub(super) fn parse_snippet_file(text: &str) -> Vec<(Vec<String>, SnippetEntry)> {
    let Ok(Value::Object(snippets)) = settings::jsonc::parse(text) else {
        return Vec::new();
    };
    snippets
        .into_iter()
        .filter_map(|(name, snippet)| {
            let prefixes: Vec<String> = match &snippet["prefix"] {
                Value::String(prefix) => vec![prefix.clone()],
                Value::Array(prefixes) => prefixes
                    .iter()
                    .filter_map(|prefix| prefix.as_str().map(str::to_owned))
                    .collect(),
                _ => return None,
            };
            let body = match &snippet["body"] {
                Value::String(body) => body.clone(),
                Value::Array(lines) => lines
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => return None,
            };
            let scopes = snippet["scope"]
                .as_str()
                .map(|scope| {
                    scope
                        .split(',')
                        .map(|part| part.trim().to_owned())
                        .collect()
                })
                .unwrap_or_default();
            Some((
                scopes,
                SnippetEntry {
                    name,
                    prefixes,
                    body,
                    description: snippet["description"].as_str().map(str::to_owned),
                },
            ))
        })
        .collect()
}

/// Applies a server's edits (LSP ranges, UTF-16 columns) to an editor.
pub(super) fn apply_text_edits(
    editor: &Entity<EditorState>,
    edits: &[TextEdit],
    window: &mut Window,
    cx: &mut App,
) {
    if edits.is_empty() {
        return;
    }
    editor.update(cx, |editor, cx| {
        let text = editor.value().to_string();
        let lines = LineIndex::new(&text);
        // The editor applies edits one after another, so the last first.
        let mut converted: Vec<TextEdit> = edits
            .iter()
            .map(|edit| {
                TextEdit::new(
                    lsp_types::Range::new(
                        lines.editor_position(edit.range.start),
                        lines.editor_position(edit.range.end),
                    ),
                    edit.new_text.clone(),
                )
            })
            .collect();
        converted.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
        // The cursor stays on its text: edits before it push it along.
        let selection = editor.selected_range();
        let reversed = editor.is_selection_reversed();
        let shift = |at: usize| -> usize {
            let mut moved = at as isize;
            for edit in edits {
                let (start, end) = (lines.offset(edit.range.start), lines.offset(edit.range.end));
                if end <= at {
                    moved += edit.new_text.len() as isize - (end - start) as isize;
                }
            }
            moved.max(0) as usize
        };
        let kept = shift(selection.start)..shift(selection.end);
        editor.apply_lsp_edits(&converted, window, cx);
        editor.set_selected_range_with_direction(kept, reversed, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_prefers_the_start_of_the_word() {
        assert_eq!(match_score("use", "useState"), Some(4));
        assert_eq!(match_score("Use", "useState"), Some(3));
        assert_eq!(match_score("ust", "useState"), Some(2));
        assert_eq!(match_score("st", "useState"), Some(1));
        assert_eq!(match_score("xyz", "useState"), None);
        assert_eq!(match_score("", "qualsiasi"), Some(0));
    }

    #[test]
    fn the_word_before_the_cursor() {
        let text = "const total = useSt";
        assert_eq!(&text[word_start(text, text.len())..], "useSt");
        assert_eq!(word_start("a.b", 2), 2);
        assert_eq!(&"caffè"[word_start("caffè", "caffè".len())..], "caffè");
    }

    #[test]
    fn items_are_expanded_converted_filtered_and_sorted() {
        let text = "const x = 1;\n🚀 cons";
        let offset = text.len();
        let start = word_start(text, offset);
        let item = |label: &str, sort: &str| CompletionItem {
            label: label.to_owned(),
            sort_text: Some(sort.to_owned()),
            ..Default::default()
        };
        let snippet = CompletionItem {
            label: "console.log".to_owned(),
            insert_text: Some("console.log($1)$0".to_owned()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("1".to_owned()),
            ..Default::default()
        };
        let edit = CompletionItem {
            label: "constructor".to_owned(),
            // "cons" is at UTF-16 columns 3..7 after the rocket.
            text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                lsp_types::Range::new(
                    lsp_types::Position::new(1, 3),
                    lsp_types::Position::new(1, 7),
                ),
                "constructor".to_owned(),
            ))),
            sort_text: Some("2".to_owned()),
            ..Default::default()
        };
        let found = vec![
            (item("zeta", "0"), None),
            (snippet, None),
            (edit, None),
            (item("const", "3"), None),
        ];
        let (items, candidates) = prepare_items(found, text, start, offset, "cons", "app.ts");
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["console.log", "constructor", "const"]);
        assert_eq!(candidates[0].inserted, "console.log()");
        assert_eq!(
            candidates[0].snippet.stops,
            vec![vec![12..12], vec![13..13]]
        );
        // The editor gets character columns: the rocket is one character.
        let Some(CompletionTextEdit::Edit(edit)) = &items[1].text_edit else {
            panic!("an edit");
        };
        assert_eq!(edit.range.start, lsp_types::Position::new(1, 2));
        assert_eq!(candidates[1].start, start);
    }

    #[test]
    fn snippet_files_in_vs_code_format() {
        let text = r#"{
            // A comment, as VS Code allows.
            "Log": { "prefix": ["log", "clg"], "body": ["console.log($1);", "$0"], "description": "Log" },
            "Only TS": { "prefix": "tsx", "body": "x", "scope": "typescript, typescriptreact" },
            "Broken": { "body": "no prefix" }
        }"#;
        let parsed = parse_snippet_file(text);
        assert_eq!(parsed.len(), 2);
        let log = &parsed
            .iter()
            .find(|(_, entry)| entry.name == "Log")
            .unwrap()
            .1;
        assert_eq!(log.prefixes, ["log", "clg"]);
        assert_eq!(log.body, "console.log($1);\n$0");
        let scoped = parsed
            .iter()
            .find(|(_, entry)| entry.name == "Only TS")
            .unwrap();
        assert_eq!(scoped.0, ["typescript", "typescriptreact"]);
    }
}

#[cfg(test)]
mod window_tests_support {
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    pub(super) fn window(
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
        // TypeScript only: in a TSX file Emmet would start a second fake server.
        std::fs::write(base.join("app.ts"), "const a = 1;\n").unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(opening, settings, None, window, cx);
            workbench.languages.fake = true;
            workbench
        });
        let path = base.join("app.ts");
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        wait_until(cx, "the server", |cx| {
            view.read_with(cx, |view, _| view.languages.is_running("vtsls"))
        });
        (folder, base, view, cx)
    }

    pub(super) fn wait_until(
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
}

#[cfg(test)]
mod window_tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::window_tests_support::*;
    use super::*;

    /// Types `text` as the editor's whole content, cursor at the end.
    fn type_text(view: &Entity<Workbench>, text: &str, cx: &mut VisualTestContext) {
        let text = text.to_owned();
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_value(text.clone(), window, cx);
                editor.set_selected_range(text.len()..text.len(), cx);
            });
            let uri = view.panes[view.active].uri.clone();
            view.sync_document_from(&uri, None, window, cx);
        });
    }

    /// Asks the editor's provider for completions, as the menu does.
    fn completions(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<CompletionItem> {
        let found: Rc<RefCell<Option<Vec<CompletionItem>>>> = Default::default();
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            let (text, offset) = {
                let editor = editor.read(cx);
                (
                    Rope::from(editor.value().as_ref()),
                    editor.selected_range().end,
                )
            };
            let provider = editor.read(cx).lsp().completion_provider.clone().unwrap();
            let context = CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            };
            let task = provider.completions(&text, offset, context, window, cx);
            let found = found.clone();
            cx.spawn(async move |_, _| {
                if let Ok(CompletionResponse::Array(items)) = task.await {
                    *found.borrow_mut() = Some(items);
                }
            })
            .detach();
        });
        wait_until(cx, "completions", |_| found.borrow().is_some());
        found.take().unwrap()
    }

    /// Picks `label` as the menu would.
    fn pick(
        view: &Entity<Workbench>,
        items: &[CompletionItem],
        label: &str,
        cx: &mut VisualTestContext,
    ) {
        let item = items
            .iter()
            .find(|item| item.label == label)
            .unwrap()
            .clone();
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            let offset = editor.read(cx).selected_range().end;
            editor.update(cx, |editor, cx| {
                editor.insert_completion(&item, offset..offset, window, cx)
            });
        });
    }

    fn text_and_selection(
        view: &Entity<Workbench>,
        cx: &mut VisualTestContext,
    ) -> (String, Range<usize>) {
        view.read_with(cx, |view, cx| {
            let editor = view.active_editor().unwrap().read(cx);
            (editor.value().to_string(), editor.selected_range())
        })
    }

    #[gpui::test]
    fn an_automatic_import_comes_with_the_completion(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        type_text(&view, "const a = 1;\nconst [n, setN] = use", cx);
        let items = completions(&view, cx);
        // Only what matches "use".
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["useState"]);
        pick(&view, &items, "useState", cx);
        wait_until(cx, "the import", |cx| {
            text_and_selection(&view, cx)
                .0
                .starts_with("import { useState } from 'react';\n")
        });
        let (text, selection) = text_and_selection(&view, cx);
        assert_eq!(
            text,
            "import { useState } from 'react';\nconst a = 1;\nconst [n, setN] = useState"
        );
        // The cursor stays after the word, moved down with the import.
        assert_eq!(selection, text.len()..text.len());
    }

    #[gpui::test]
    fn snippets_put_the_cursor_inside_and_tab_moves_on(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        type_text(&view, "console.lo", cx);
        let items = completions(&view, cx);
        pick(&view, &items, "log", cx);
        cx.run_until_parked();
        let (text, selection) = text_and_selection(&view, cx);
        assert_eq!(text, "console.log()");
        // Between the parentheses.
        assert_eq!(selection, 12..12);
        // Typing inside the parentheses stretches the place; Tab then goes on.
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| editor.insert("'Keel'", window, cx));
        });
        cx.run_until_parked();
        view.update_in(cx, |view, window, cx| {
            let used = view.move_in_snippet(true, window, cx);
            assert!(used, "Tab goes to the end");
        });
        let (text, selection) = text_and_selection(&view, cx);
        assert_eq!(text, "console.log('Keel')");
        assert_eq!(selection, text.len()..text.len());
        view.read_with(cx, |view, _| assert!(view.snippet.is_none()));
    }

    #[gpui::test]
    fn the_signature_follows_the_parameters(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        type_text(&view, "saluta", cx);
        type_text(&view, "saluta(", cx);
        wait_until(cx, "a signature", |cx| {
            view.read_with(cx, |view, _| view.signature.is_some())
        });
        let active = |cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| {
                view.signature
                    .as_ref()
                    .and_then(|state| state.help.active_parameter)
            })
        };
        assert_eq!(active(cx), Some(0));
        type_text(&view, "saluta('Manuel', ", cx);
        wait_until(cx, "the second parameter", |cx| active(cx) == Some(1));
        // Closing the call closes the signature.
        type_text(&view, "saluta('Manuel', 2)", cx);
        wait_until(cx, "no signature", |cx| {
            view.read_with(cx, |view, _| view.signature.is_none())
        });
    }

    #[gpui::test]
    fn user_snippets_are_offered(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        let data = base.join("data");
        std::fs::create_dir_all(data.join("snippets")).unwrap();
        std::fs::write(
            data.join("snippets/typescript.json"),
            r#"{ "Componente": { "prefix": "rfc", "body": "export function ${1:Nome}() {\n\treturn $0;\n}" } }"#,
        )
        .unwrap();
        view.update(cx, |view, _| {
            view.snippet_files.borrow_mut().data_dir = Some(data.clone())
        });
        type_text(&view, "rf", cx);
        let items = completions(&view, cx);
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["rfc"]
        );
        pick(&view, &items, "rfc", cx);
        cx.run_until_parked();
        let (text, selection) = text_and_selection(&view, cx);
        assert_eq!(text, "export function Nome() {\n\treturn ;\n}");
        // "Nome" is selected, ready to be typed over.
        assert_eq!(&text[selection], "Nome");
    }
}

#[cfg(test)]
mod hover_tests {
    use super::window_tests_support::*;
    use super::*;

    #[gpui::test]
    fn hover_asks_the_server(cx: &mut gpui::TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        let found: Rc<RefCell<Option<Option<Hover>>>> = Default::default();
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            let text = Rope::from(editor.read(cx).value().as_ref());
            let provider = editor.read(cx).lsp().hover_provider.clone().unwrap();
            let task = provider.hover(&text, 2, window, cx);
            let found = found.clone();
            cx.spawn(async move |_, _| *found.borrow_mut() = Some(task.await.ok().flatten()))
                .detach();
        });
        wait_until(cx, "a hover", |_| found.borrow().is_some());
        let hover = found.take().flatten().expect("the server answered");
        let lsp_types::HoverContents::Markup(content) = hover.contents else {
            panic!("markdown");
        };
        // The fake server answers with the length of the text it has.
        assert_eq!(content.value, "13 bytes");
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    /// Prepares 2,000 items for a 3,000-line file, as a large TypeScript
    /// answer would. Run with
    /// `cargo test -p keel --release timing_completion -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing_completion_items() {
        let text = include_str!("../workbench_window.rs");
        let offset = text.len() / 2;
        let offset = (offset..).find(|at| text.is_char_boundary(*at)).unwrap();
        let start = word_start(text, offset);
        let found: Vec<(CompletionItem, Option<Rc<RunningServer>>)> = (0..2000)
            .map(|index| {
                (
                    CompletionItem {
                        label: format!("item{index}"),
                        sort_text: Some(format!("{index:05}")),
                        ..Default::default()
                    },
                    None,
                )
            })
            .collect();
        let started = std::time::Instant::now();
        let (items, _) = prepare_items(found, text, start, offset, "", "workbench_window.rs");
        println!("{} items ready in {:?}", items.len(), started.elapsed());
    }
}
