//! The status bar's items about the file in front, each a click from
//! changing it, as in VS Code: line and column, indentation, encoding,
//! line endings and language; the size of the selection; and the chord
//! waiting for its second key after Cmd+K.

use editor::Encoding;
use editor::text_edits::{LineEnding, convert_indentation, detect_indentation};
use gpui_component::input::TabSize;

use super::notices::fill;
use super::shell::language_name;
use super::*;
use crate::quick_pick::{self, PickItem};

/// What a click on an item of the status bar opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StatusClick {
    GoToLine,
    Indentation,
    Encoding,
    LineEnding,
    Language,
    PullRequest,
}

impl StatusClick {
    pub(super) fn tooltip(self, strings: &Strings) -> &'static str {
        match self {
            StatusClick::GoToLine => strings.status_go_to_line,
            StatusClick::Indentation => strings.status_pick_indentation,
            StatusClick::Encoding => strings.status_pick_encoding,
            StatusClick::LineEnding => strings.status_pick_line_ending,
            StatusClick::Language => strings.status_pick_language,
            StatusClick::PullRequest => strings.pulls_title_page,
        }
    }
}

/// The languages Keel can give a file, as «Select Language Mode» lists them.
const LANGUAGES: &[&str] = &[
    "plaintext",
    "c",
    "cpp",
    "csharp",
    "css",
    "dockerfile",
    "dotenv",
    "html",
    "java",
    "javascript",
    "javascriptreact",
    "json",
    "jsonc",
    "less",
    "makefile",
    "markdown",
    "prisma",
    "properties",
    "python",
    "rust",
    "scss",
    "shellscript",
    "sql",
    "toml",
    "typescript",
    "typescriptreact",
    "xml",
    "yaml",
];

/// A keystroke as a Mac shows it: `⌘K`.
fn keystroke_text(keystroke: &gpui::Keystroke) -> String {
    super::keybindings_view::readable(&keystroke.unparse(gpui::OperatingSystem::Mac))
}

impl Workbench {
    /// The indentation of a document: as the user chose or as its lines
    /// say, else as the settings of its language.
    pub(super) fn indent_of(&self, uri: &str) -> TabSize {
        if let Some(indent) = self.indent_overrides.get(uri) {
            return *indent;
        }
        let language = self
            .documents
            .get(uri)
            .map(|document| document.language_id.clone())
            .unwrap_or_default();
        self.tab_size_for(&language)
    }

    /// Reads a file's indentation from its lines, unless
    /// `editor.detectIndentation` is off, as in VS Code.
    pub(super) fn detect_indent(&mut self, uri: &str, text: &str) {
        if self
            .settings
            .get("editor.detectIndentation")
            .and_then(Value::as_bool)
            == Some(false)
        {
            return;
        }
        let Some((tabs, size)) = detect_indentation(text) else {
            return;
        };
        let language = self
            .documents
            .get(uri)
            .map(|document| document.language_id.clone())
            .unwrap_or_default();
        let settings = self.tab_size_for(&language);
        self.indent_overrides.insert(
            uri.to_owned(),
            TabSize {
                tab_size: if tabs { settings.tab_size } else { size },
                hard_tabs: tabs,
            },
        );
    }

    /// Follows the pending keystrokes, for the chord in the status bar.
    pub(super) fn pending_input_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pending = window
            .pending_input_keystrokes()
            .filter(|keys| !keys.is_empty())
            .map(|keys| {
                keys.iter()
                    .map(keystroke_text)
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        if pending != self.pending_chord {
            self.pending_chord = pending;
            cx.notify();
        }
    }

    /// The chord waiting for its second key, as the status bar says it.
    pub(super) fn pending_chord_text(&self) -> Option<String> {
        let keys = self.pending_chord.as_ref()?;
        Some(fill(self.strings().pending_chord, &[("keys", keys)]))
    }

    /// The right side of the status bar for an editor: each item and what
    /// a click on it opens.
    pub(super) fn editor_status(&self, uri: &str, cx: &App) -> Vec<(String, Option<StatusClick>)> {
        let strings = self.strings();
        let Some(pane) = self.panes.get(self.active) else {
            return Vec::new();
        };
        let editor = pane.editor.read(cx);
        let position = editor.cursor_position();
        let mut line =
            strings.line_column(position.line as usize + 1, position.character as usize + 1);
        let selected = editor.selected_range();
        if !selected.is_empty() {
            let text = editor.value();
            let count = text
                .get(selected.clone())
                .map(|part| part.chars().count())
                .unwrap_or_default();
            line.push(' ');
            line.push_str(&fill(
                strings.status_selected,
                &[("count", &count.to_string())],
            ));
        }
        let mut items = vec![(line, Some(StatusClick::GoToLine))];
        let indent = self.indent_of(uri);
        items.push((
            strings.indentation(indent.tab_size, indent.hard_tabs),
            Some(StatusClick::Indentation),
        ));
        let document = self.documents.get(uri);
        let encoding = match document.map(|document| &document.backing) {
            Some(Backing::File(file)) => Some(file.encoding()),
            Some(Backing::Untitled) => Some(Encoding::Utf8),
            _ => None,
        };
        if let Some(encoding) = encoding {
            items.push((encoding.name().to_owned(), Some(StatusClick::Encoding)));
        }
        if let Some(document) = document {
            let ending = LineEnding::of(&document.synced_text);
            items.push((ending.name().to_owned(), Some(StatusClick::LineEnding)));
            items.push((
                language_name(&document.language_id, strings),
                Some(StatusClick::Language),
            ));
        }
        items
    }

    pub(super) fn status_clicked(
        &mut self,
        click: StatusClick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match click {
            StatusClick::GoToLine => window.dispatch_action(Box::new(GoToLine), cx),
            StatusClick::Indentation => self.pick_indentation(window, cx),
            StatusClick::Encoding => self.pick_encoding(window, cx),
            StatusClick::LineEnding => self.pick_line_ending(window, cx),
            StatusClick::Language => self.pick_language(window, cx),
            StatusClick::PullRequest => self.open_branch_pull(window, cx),
        }
    }

    fn active_uri(&self) -> Option<String> {
        self.panes.get(self.active).map(|pane| pane.uri.clone())
    }

    /// Gives every editor of a document its indentation.
    fn apply_indent(&mut self, uri: &str, indent: TabSize, cx: &mut Context<Self>) {
        self.indent_overrides.insert(uri.to_owned(), indent);
        for pane in self.panes.iter().filter(|pane| pane.uri == uri) {
            pane.editor
                .update(cx, |editor, cx| editor.set_tab_size(indent, cx));
        }
        cx.notify();
    }

    fn pick_indentation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(uri) = self.active_uri() else {
            return;
        };
        let strings = self.strings();
        let choices = [
            strings.indent_with_spaces,
            strings.indent_with_tabs,
            strings.indent_detect,
            strings.indent_to_spaces,
            strings.indent_to_tabs,
        ];
        let items = choices.iter().map(|label| PickItem::new(*label)).collect();
        let chosen = quick_pick::show(None, None, items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                let current = this.indent_of(&uri);
                match picked {
                    0 | 1 => this.pick_indent_size(uri.clone(), picked == 1, window, cx),
                    2 => {
                        this.indent_overrides.remove(&uri);
                        let text = this
                            .documents
                            .get(&uri)
                            .map(|document| document.synced_text.clone())
                            .unwrap_or_default();
                        this.detect_indent(&uri, &text);
                        let indent = this.indent_of(&uri);
                        this.apply_indent(&uri, indent, cx);
                    }
                    _ => {
                        let to_tabs = picked == 4;
                        if let Some(pane) = this.panes.get(this.active) {
                            let text = pane.editor.read(cx).value().to_string();
                            let converted = convert_indentation(&text, to_tabs, current.tab_size);
                            super::tabs::replace_text(&pane.editor, &converted, window, cx);
                        }
                        let indent = TabSize {
                            tab_size: current.tab_size,
                            hard_tabs: to_tabs,
                        };
                        this.apply_indent(&uri, indent, cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn pick_indent_size(
        &mut self,
        uri: String,
        tabs: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.indent_of(&uri).tab_size;
        let sizes: Vec<usize> = (1..=8).collect();
        let items = sizes
            .iter()
            .map(|size| PickItem::new(size.to_string()))
            .collect();
        let options = quick_pick::PickOptions {
            selected: Some(current.clamp(1, 8) - 1),
            ..quick_pick::PickOptions::default()
        };
        let chosen = quick_pick::show_with_options(
            Some(self.strings().indent_size_title.to_owned()),
            None,
            items,
            quick_pick::Matching::Text,
            None,
            options,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(choice)) = chosen.await else {
                return;
            };
            this.update(cx, |this, cx| {
                let indent = TabSize {
                    tab_size: sizes[choice.index],
                    hard_tabs: tabs,
                };
                this.apply_indent(&uri, indent, cx);
            })
            .ok();
        })
        .detach();
    }

    fn pick_encoding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(uri) = self.active_uri() else {
            return;
        };
        let Some(current) = self
            .documents
            .get(&uri)
            .and_then(|document| match &document.backing {
                Backing::File(file) => Some(file.encoding()),
                _ => None,
            })
        else {
            return;
        };
        let strings = self.strings();
        let items = vec![
            PickItem::new(strings.encoding_reopen),
            PickItem::new(strings.encoding_save),
        ];
        let chosen = quick_pick::show(None, None, items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(action)) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                let strings = this.strings();
                let items = Encoding::ALL
                    .iter()
                    .map(|encoding| {
                        let mut item = PickItem::new(encoding.name());
                        if *encoding == current {
                            item.detail = Some(strings.encoding_current.to_owned());
                        }
                        item
                    })
                    .collect();
                let options = quick_pick::PickOptions {
                    selected: Encoding::ALL
                        .iter()
                        .position(|encoding| *encoding == current),
                    ..quick_pick::PickOptions::default()
                };
                let title = if action == 0 {
                    strings.encoding_reopen
                } else {
                    strings.encoding_save
                };
                let chosen = quick_pick::show_with_options(
                    Some(title.to_owned()),
                    None,
                    items,
                    quick_pick::Matching::Text,
                    None,
                    options,
                    window,
                    cx,
                );
                let uri = uri.clone();
                cx.spawn_in(window, async move |this, cx| {
                    let Ok(Some(choice)) = chosen.await else {
                        return;
                    };
                    let encoding = Encoding::ALL[choice.index];
                    this.update_in(cx, |this, window, cx| {
                        if action == 0 {
                            this.reopen_with(&uri, encoding, window, cx);
                        } else {
                            this.save_with(&uri, encoding, window, cx);
                        }
                    })
                    .ok();
                })
                .detach();
            })
            .ok();
        })
        .detach();
    }

    /// «Reopen with Encoding»: the file read again, its text as that
    /// encoding gives it.
    pub(super) fn reopen_with(
        &mut self,
        uri: &str,
        encoding: Encoding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        if document.modified {
            self.status_message = Some(strings.encoding_save_first.to_owned());
            cx.notify();
            return;
        }
        let Backing::File(file) = &mut document.backing else {
            return;
        };
        match file.reopen_with(encoding) {
            Ok(Some(text)) => {
                document.synced_text = text.clone();
                for pane in self.panes.iter().filter(|pane| pane.uri == uri) {
                    super::tabs::replace_text(&pane.editor, &text, window, cx);
                }
            }
            Ok(None) => {
                self.status_message = Some(fill(
                    strings.encoding_not_valid,
                    &[("encoding", encoding.name())],
                ));
            }
            Err(error) => self.status_message = Some(error.to_string()),
        }
        cx.notify();
    }

    /// «Save with Encoding»: written now in that encoding, and from now on.
    pub(super) fn save_with(
        &mut self,
        uri: &str,
        encoding: Encoding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(Backing::File(file)) = self
            .documents
            .get_mut(uri)
            .map(|document| &mut document.backing)
        {
            file.set_encoding(encoding);
            self.save_file_now(uri, None, window, cx);
        }
    }

    fn pick_line_ending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let editor = pane.editor.clone();
        let current = LineEnding::of(&editor.read(cx).value());
        let endings = [LineEnding::Lf, LineEnding::CrLf];
        let items = endings
            .iter()
            .map(|ending| PickItem::new(ending.name()))
            .collect();
        let options = quick_pick::PickOptions {
            selected: endings.iter().position(|ending| *ending == current),
            ..quick_pick::PickOptions::default()
        };
        let chosen = quick_pick::show_with_options(
            Some(self.strings().line_ending_title.to_owned()),
            None,
            items,
            quick_pick::Matching::Text,
            None,
            options,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(choice)) = chosen.await else {
                return;
            };
            this.update_in(cx, |_, window, cx| {
                let text = editor.read(cx).value().to_string();
                let converted = endings[choice.index].apply(&text);
                super::tabs::replace_text(&editor, &converted, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn pick_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(uri) = self.active_uri() else {
            return;
        };
        let strings = self.strings();
        let current = self
            .documents
            .get(&uri)
            .map(|document| document.language_id.clone())
            .unwrap_or_default();
        let mut items = vec![PickItem::new(strings.language_auto)];
        items.extend(LANGUAGES.iter().map(|id| {
            let mut item = PickItem::new(language_name(id, strings));
            item.detail = Some((*id).to_owned());
            item
        }));
        let options = quick_pick::PickOptions {
            selected: LANGUAGES
                .iter()
                .position(|id| *id == current)
                .map(|index| index + 1),
            ..quick_pick::PickOptions::default()
        };
        let chosen = quick_pick::show_with_options(
            Some(strings.language_title.to_owned()),
            None,
            items,
            quick_pick::Matching::Text,
            None,
            options,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(choice)) = chosen.await else {
                return;
            };
            this.update(cx, |this, cx| {
                let language = match choice.index {
                    0 => Uri::parse(&uri)
                        .and_then(|parsed| parsed.file_path())
                        .map(|path| this.language_id_for(&path))
                        .unwrap_or_else(|| "plaintext".to_owned()),
                    index => LANGUAGES[index - 1].to_owned(),
                };
                this.set_document_language(&uri, language, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Gives a document another language: its colors, and the language
    /// servers and extensions see it closed and opened again, as VS Code's
    /// `setTextDocumentLanguage` does.
    pub(super) fn set_document_language(
        &mut self,
        uri: &str,
        language: String,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        if document.language_id == language {
            return;
        }
        document.language_id = language.clone();
        let text = document.synced_text.clone();
        let version = document.version;
        let dirty = document.modified;
        let highlighter = Language::for_vscode_id(&language).highlighter_name();
        for pane in self.panes.iter().filter(|pane| pane.uri == uri) {
            pane.editor
                .update(cx, |editor, cx| editor.set_highlighter(highlighter, cx));
        }
        self.notify_host("documents/closed", json!({ "uri": uri }));
        self.notify_host(
            "documents/opened",
            json!({ "uri": uri, "languageId": language, "version": version, "text": text, "isDirty": dirty }),
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_read_as_on_a_mac() {
        let keystroke = gpui::Keystroke::parse("cmd-k").unwrap();
        assert_eq!(keystroke_text(&keystroke), "⌘K");
    }
}
