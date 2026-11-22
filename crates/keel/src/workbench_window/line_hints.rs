//! What the language server says about a line, written at its end as
//! JetBrains' Code Vision puts it on the right: «3 riferimenti» on a
//! declaration, which opens them in a frame, and the types the server
//! infers (`editor.inlayHints.enabled`), dimmed. At the end of the line the
//! text never moves under the cursor.

use std::collections::HashMap;

use gpui::{
    AnyElement, Bounds, ContentMask, CursorStyle, Hitbox, HitboxBehavior, MouseButton,
    MouseDownEvent, Pixels, SharedString, TextRun, canvas, point,
};
use gpui_component::{ActiveTheme as _, RopeExt as _};
use lsp_types::{CodeLens, InlayHint, InlayHintKind, InlayHintLabel, Location};

use super::code_navigation::Place;
use super::language_servers::RunningServer;
use super::*;

/// How long typing pauses before the server is asked again.
const PAUSE: Duration = Duration::from_millis(700);
/// Lenses resolved for one file at most, as each may search the project.
const MOST_LENSES: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LensKind {
    References,
    Implementations,
}

#[derive(Clone, Debug)]
pub(super) struct Lens {
    pub(super) line: usize,
    pub(super) kind: LensKind,
    pub(super) places: Vec<Place>,
}

/// What a line shows at its end: types, then the lenses.
#[derive(Clone, Debug, Default)]
pub(super) struct LineEnd {
    pub(super) hints: Vec<String>,
    pub(super) lenses: Vec<Lens>,
}

#[derive(Default)]
struct DocumentHints {
    version: Option<i32>,
    lines: Rc<HashMap<usize, LineEnd>>,
    task: Option<Task<()>>,
}

/// A question the server did not answer, in the languages' log; a server
/// still reading the project says "content modified" and asks again later.
fn failed(
    this: &gpui::WeakEntity<Workbench>,
    running: &RunningServer,
    method: &str,
    error: &anyhow::Error,
    cx: &mut gpui::AsyncApp,
) {
    let name = running.spec.name;
    let text = format!("[{name}] {method}: {error:#}");
    this.update(cx, |this, _| {
        this.log_to(super::panel_views::OutputChannel::Languages, text)
    })
    .ok();
}

/// What one refresh asks, and of whom.
struct Question {
    version: i32,
    text: String,
    lenses_from: Option<Rc<RunningServer>>,
    hints_from: Option<Rc<RunningServer>>,
    resolves: bool,
}

#[derive(Default)]
pub(super) struct LineHints {
    documents: HashMap<String, DocumentHints>,
    /// Ctrl+Option is held: hints show (or hide) as long as it is.
    pub(super) pressed: bool,
}

/// The places a lens's command shows: VS Code's `showReferences` and
/// rust-analyzer's take the file, the position and the places.
fn places_of_command(command: &lsp_types::Command) -> Option<Vec<Place>> {
    command.arguments.as_ref()?.iter().find_map(|argument| {
        let locations: Vec<Location> = serde_json::from_value(argument.clone()).ok()?;
        Some(
            locations
                .into_iter()
                .map(super::code_navigation::place)
                .collect(),
        )
    })
}

/// A resolved lens Keel can show: references or implementations, with the
/// places they open. Run and Debug lenses are left to the test margin.
fn lens_of(lens: &CodeLens) -> Option<Lens> {
    let command = lens.command.as_ref()?;
    let places = places_of_command(command)?;
    let kind = if command.title.to_lowercase().contains("implementation") {
        LensKind::Implementations
    } else {
        LensKind::References
    };
    Some(Lens {
        line: lens.range.start.line as usize,
        kind,
        places,
    })
}

/// A hint's text at the end of its line. A variable's type is written
/// with the name it belongs to, `rows: Range<usize>`; a chain's type as
/// it is; a parameter's name means nothing away from its argument.
fn hint_text(hint: &InlayHint, line: &str) -> Option<String> {
    if hint.kind == Some(InlayHintKind::PARAMETER) {
        return None;
    }
    let label = match &hint.label {
        InlayHintLabel::String(label) => label.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|part| part.value.as_str()).collect(),
    };
    let label = label.trim().to_owned();
    if label.is_empty() {
        return None;
    }
    let Some(rest) = label.strip_prefix(':') else {
        return Some(label);
    };
    // The identifier right before the hint, counted in UTF-16 as LSP does.
    let mut before = String::new();
    let mut units = 0;
    for char in line.chars() {
        if units >= hint.position.character as usize {
            break;
        }
        units += char.len_utf16();
        before.push(char);
    }
    let name: String = before
        .chars()
        .rev()
        .take_while(|char| char.is_alphanumeric() || *char == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(if name.is_empty() {
        rest.trim().to_owned()
    } else {
        format!("{name}: {}", rest.trim())
    })
}

/// The hints and lenses of a document, by line.
fn by_line(text: &str, hints: &[InlayHint], lenses: Vec<Lens>) -> HashMap<usize, LineEnd> {
    let lines: Vec<&str> = text.lines().collect();
    let mut ends: HashMap<usize, LineEnd> = HashMap::new();
    let mut hints: Vec<&InlayHint> = hints.iter().collect();
    hints.sort_by_key(|hint| (hint.position.line, hint.position.character));
    for hint in hints {
        let line = hint.position.line as usize;
        if let Some(text) = hint_text(hint, lines.get(line).copied().unwrap_or_default()) {
            ends.entry(line).or_default().hints.push(text);
        }
    }
    for lens in lenses {
        ends.entry(lens.line).or_default().lenses.push(lens);
    }
    ends
}

impl Workbench {
    /// Whether the types show now: `editor.inlayHints.enabled` is "on",
    /// "off", or shown ("offUnlessPressed", Keel's default) or hidden
    /// ("onUnlessPressed") while Ctrl+Option is held, as in VS Code.
    fn hints_shown(&self) -> bool {
        match self.settings.string("editor.inlayHints.enabled") {
            Some("on") => true,
            Some("off") => false,
            Some("onUnlessPressed") => !self.line_hints.pressed,
            _ => self.line_hints.pressed,
        }
    }

    fn lenses_shown(&self) -> bool {
        self.settings
            .get("editor.codeLens")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    pub(super) fn line_hints_modifiers(
        &mut self,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let pressed = modifiers.control && modifiers.alt;
        if pressed != self.line_hints.pressed {
            self.line_hints.pressed = pressed;
            cx.notify();
        }
    }

    /// Asks again for the documents in view, as a server says its
    /// answers changed (rust-analyzer, once it has read the project).
    pub(super) fn refresh_visible_line_hints(&mut self, cx: &mut Context<Self>) {
        let uris: Vec<String> = self.panes.iter().map(|pane| pane.uri.clone()).collect();
        for uri in uris {
            if let Some(document) = self.line_hints.documents.get_mut(&uri) {
                document.version = None;
            }
            self.refresh_line_hints(&uri, false, cx);
        }
    }
    /// Asks the document's servers for its lenses and hints, after a
    /// pause when it is being edited: what is asked is the text then.
    pub(super) fn refresh_line_hints(&mut self, uri: &str, pause: bool, cx: &mut Context<Self>) {
        if !self.panes.iter().any(|pane| pane.uri == uri) {
            return;
        }
        let key = uri.to_owned();
        let uri = uri.to_owned();
        let task = cx.spawn(async move |this, cx| {
            if pause {
                cx.background_executor().timer(PAUSE).await;
            }
            let Some(question) = this
                .update(cx, |this, cx| this.line_hints_question(&uri, cx))
                .ok()
                .flatten()
            else {
                return;
            };
            let Question {
                version,
                text,
                lenses_from,
                hints_from,
                resolves,
            } = question;
            let document = json!({ "uri": uri });
            let mut lenses = Vec::new();
            if let Some(running) = &lenses_from {
                let answer = running
                    .server
                    .request_raw("textDocument/codeLens", json!({ "textDocument": document }))
                    .await;
                let found: Vec<CodeLens> = match answer {
                    Ok(value) => serde_json::from_value(value).unwrap_or_default(),
                    Err(error) => {
                        failed(&this, running, "textDocument/codeLens", &error, cx);
                        Vec::new()
                    }
                };
                for lens in found.into_iter().take(MOST_LENSES) {
                    let lens = if lens.command.is_none() && resolves {
                        let Ok(asked) = serde_json::to_value(&lens) else {
                            continue;
                        };
                        let answer = running.server.request_raw("codeLens/resolve", asked).await;
                        match answer
                            .ok()
                            .and_then(|value| serde_json::from_value(value).ok())
                        {
                            Some(resolved) => resolved,
                            None => continue,
                        }
                    } else {
                        lens
                    };
                    lenses.extend(lens_of(&lens));
                }
            }
            let mut hints: Vec<InlayHint> = Vec::new();
            if let Some(running) = &hints_from {
                // The whole text, to its last character: rust-analyzer
                // refuses a place past the end.
                let last_line = text.matches('\n').count();
                let last_column: usize = text
                    .rsplit('\n')
                    .next()
                    .unwrap_or_default()
                    .encode_utf16()
                    .count();
                let range = json!({
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": last_line, "character": last_column },
                });
                let answer = running
                    .server
                    .request_raw(
                        "textDocument/inlayHint",
                        json!({ "textDocument": document, "range": range }),
                    )
                    .await;
                match answer {
                    Ok(value) => hints = serde_json::from_value(value).unwrap_or_default(),
                    Err(error) => failed(&this, running, "textDocument/inlayHint", &error, cx),
                }
            }
            let lines = by_line(&text, &hints, lenses);
            this.update(cx, |this, cx| {
                let document = this.line_hints.documents.entry(uri).or_default();
                document.version = Some(version);
                document.lines = Rc::new(lines);
                cx.notify();
            })
            .ok();
        });
        self.line_hints.documents.entry(key).or_default().task = Some(task);
    }

    /// What to ask about the document now, unless its answers are of
    /// this version already or no server of it has lenses or hints.
    fn line_hints_question(&mut self, uri: &str, cx: &mut Context<Self>) -> Option<Question> {
        let pane = self.panes.iter().find(|pane| pane.uri == uri)?;
        let version = self.documents.get(uri)?.version as i32;
        if self
            .line_hints
            .documents
            .get(uri)
            .is_some_and(|document| document.version == Some(version))
        {
            return None;
        }
        let text = pane.editor.read(cx).value().to_string();
        self.languages.sync(uri, &text);
        let servers = self.languages.servers_for(uri);
        let provides = |lenses: bool| {
            servers
                .iter()
                .find(|running| {
                    running.server.capabilities().is_some_and(|capabilities| {
                        if lenses {
                            capabilities.code_lens_provider.is_some()
                        } else {
                            capabilities.inlay_hint_provider.is_some()
                        }
                    })
                })
                .cloned()
        };
        let (lenses_from, hints_from) = (provides(true), provides(false));
        if lenses_from.is_none() && hints_from.is_none() {
            return None;
        }
        let resolves = lenses_from.as_ref().is_some_and(|running| {
            running
                .server
                .capabilities()
                .and_then(|capabilities| capabilities.code_lens_provider.as_ref())
                .and_then(|provider| provider.resolve_provider)
                .unwrap_or(false)
        });
        Some(Question {
            version,
            text,
            lenses_from,
            hints_from,
            resolves,
        })
    }

    /// The types and lenses at the ends of pane `index`'s lines. They are
    /// placed while painting, after the editor has laid out this frame,
    /// so they stay with their lines as it scrolls.
    pub(super) fn render_line_hints(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pane = self.panes.get(index)?;
        let lines = self
            .line_hints
            .documents
            .get(&pane.uri)
            .map(|document| document.lines.clone())
            .unwrap_or_default();
        // The values of the program stopped in this file.
        let values: HashMap<usize, Vec<String>> = self
            .debug
            .inline
            .as_ref()
            .filter(|(uri, _)| *uri == pane.uri)
            .map(|(_, values)| values.clone())
            .unwrap_or_default();
        let (hints, lenses) = (self.hints_shown(), self.lenses_shown());
        if values.is_empty() && (lines.is_empty() || (!hints && !lenses)) {
            return None;
        }
        let editor = pane.editor.clone();
        let strings = self.strings();
        let theme = cx.theme();
        let (faint, muted, text_color) = (
            theme.muted_foreground.opacity(0.7),
            theme.muted_foreground,
            theme.foreground,
        );
        let font_size = px(self.font_size * 0.85);
        let workbench = cx.entity().downgrade();
        // What each line shows: its pieces, a lens with its places.
        type Piece = (String, Option<Lens>);
        let nothing = LineEnd::default();
        let pieces_of = move |row: usize| -> Vec<Piece> {
            let mut pieces: Vec<Piece> = Vec::new();
            // While it is stopped, the values take the types' place.
            let here = values.get(&row);
            pieces.extend(
                here.into_iter()
                    .flatten()
                    .map(|value| (value.clone(), None)),
            );
            let end = lines.get(&row).unwrap_or(&nothing);
            if hints && here.is_none() {
                pieces.extend(end.hints.iter().map(|hint| (hint.clone(), None)));
            }
            if lenses {
                // No references says nothing worth a place: `main`, tests.
                for lens in end.lenses.iter().filter(|lens| !lens.places.is_empty()) {
                    let count = lens.places.len();
                    let word = match (lens.kind, count) {
                        (LensKind::References, 1) => strings.lens_reference,
                        (LensKind::References, _) => strings.lens_references,
                        (LensKind::Implementations, 1) => strings.lens_implementation,
                        (LensKind::Implementations, _) => strings.lens_implementations,
                    };
                    pieces.push((format!("{count} {word}"), Some(lens.clone())));
                }
            }
            pieces
        };
        type Placed = Vec<(
            Bounds<Pixels>,
            gpui::ShapedLine,
            bool,
            Option<(Hitbox, Lens)>,
        )>;
        Some(
            canvas(
                {
                    let editor = editor.clone();
                    move |bounds, window, cx| -> Placed {
                        let state = editor.read(cx);
                        let Some(rows) = state.visible_row_range() else {
                            return Vec::new();
                        };
                        let text = state.text();
                        let font = window.text_style().font();
                        let mut placed = Vec::new();
                        for row in rows {
                            let pieces = pieces_of(row);
                            if pieces.is_empty() {
                                continue;
                            }
                            let offset = text.line_end_offset(row);
                            let Some(at) = state.range_to_bounds(&(offset..offset)) else {
                                continue;
                            };
                            let mut x = at.origin.x + font_size * 1.6;
                            for (label, lens) in pieces {
                                let run = TextRun {
                                    len: label.len(),
                                    font: font.clone(),
                                    color: if lens.is_some() { muted } else { faint },
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                };
                                let shaped = window.text_system().shape_line(
                                    SharedString::from(label),
                                    font_size,
                                    &[run],
                                    None,
                                );
                                let width = shaped.width;
                                let spot = Bounds::new(
                                    point(x, at.origin.y),
                                    gpui::size(width, at.size.height),
                                );
                                // A piece shows whole or not at all.
                                if spot.right() > bounds.right() {
                                    break;
                                }
                                let hitbox = lens.map(|lens| {
                                    (window.insert_hitbox(spot, HitboxBehavior::Normal), lens)
                                });
                                placed.push((spot, shaped, hitbox.is_some(), hitbox));
                                x += width + font_size * 1.2;
                            }
                        }
                        placed
                    }
                },
                move |bounds, placed, window, cx| {
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        for (spot, shaped, is_lens, hitbox) in &placed {
                            let hovered = hitbox
                                .as_ref()
                                .is_some_and(|(hitbox, _)| hitbox.is_hovered(window));
                            if hovered {
                                window.set_cursor_style(
                                    CursorStyle::PointingHand,
                                    &hitbox.as_ref().unwrap().0,
                                );
                            }
                            let origin = point(
                                spot.origin.x,
                                spot.origin.y + (spot.size.height - font_size * 1.3) / 2.,
                            );
                            let _ = shaped.paint(
                                origin,
                                font_size * 1.3,
                                gpui::TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                            if hovered && *is_lens {
                                // Underlined under the mouse, as a link.
                                window.paint_quad(gpui::fill(
                                    Bounds::new(
                                        point(origin.x, origin.y + font_size * 1.2),
                                        gpui::size(shaped.width, px(1.)),
                                    ),
                                    text_color.opacity(0.6),
                                ));
                            }
                        }
                    });
                    let lenses: Vec<(Hitbox, Lens)> = placed
                        .into_iter()
                        .filter_map(|(_, _, _, hitbox)| hitbox)
                        .collect();
                    let workbench = workbench.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if !phase.bubble() || event.button != MouseButton::Left {
                            return;
                        }
                        let Some((_, lens)) =
                            lenses.iter().find(|(hitbox, _)| hitbox.is_hovered(window))
                        else {
                            return;
                        };
                        let lens = lens.clone();
                        cx.stop_propagation();
                        workbench
                            .update(cx, |this, cx| this.open_lens(index, lens, window, cx))
                            .ok();
                    });
                },
            )
            .absolute()
            .inset_0()
            .into_any_element(),
        )
    }

    /// A lens clicked: its places in a frame under its line.
    fn open_lens(&mut self, index: usize, lens: Lens, window: &mut Window, cx: &mut Context<Self>) {
        if lens.places.is_empty() {
            return;
        }
        if let Some(editor) = self.panes.get(index).map(|pane| pane.editor.clone()) {
            self.focus_pane(index, window, cx);
            editor.update(cx, |editor, cx| {
                editor.set_cursor_position(
                    gpui_component::input::Position::new(lens.line as u32, 0),
                    window,
                    cx,
                )
            });
        }
        let strings = self.strings();
        let title = match lens.kind {
            LensKind::References => strings.lsp_references_title(lens.places.len()),
            LensKind::Implementations => {
                format!("{} {}", lens.places.len(), strings.lens_implementations)
            }
        };
        self.open_peek(title, lens.places, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};
    use lsp_types::{Command, Position, Range};

    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.executor().advance_clock(Duration::from_millis(800));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[gpui::test]
    fn references_and_types_come_from_the_server(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        std::fs::write(
            base.join("app.ts"),
            "export function saluta() {}\nconst volte = 3;\nsaluta();\nsaluta();\n",
        )
        .unwrap();
        std::fs::write(
            base.join("settings.json"),
            r#"{ "editor.inlayHints.enabled": "on" }"#,
        )
        .unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(opening, settings, None, window, cx);
            workbench.languages.fake = true;
            workbench
        });
        let path = base.join("app.ts");
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        let uri = Uri::file(&path).to_string();
        let ends = |cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| {
                view.line_hints
                    .documents
                    .get(&uri)
                    .map(|document| document.lines.clone())
            })
        };
        wait_until(cx, "the lenses and hints", |cx| {
            ends(cx).is_some_and(|lines| !lines.is_empty())
        });
        let lines = ends(cx).unwrap();
        let lens = &lines[&0].lenses[0];
        assert_eq!((lens.kind, lens.places.len()), (LensKind::References, 2));
        assert_eq!(lines[&1].hints, ["volte: number"]);

        // Drawn at the ends of their lines.
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        view.update(cx, |view, cx| {
            assert!(view.render_line_hints(0, cx).is_some())
        });

        // A click on the lens: its places in the frame.
        view.update_in(cx, |view, window, cx| {
            view.open_lens(0, lens.clone(), window, cx)
        });
        view.read_with(cx, |view, _| {
            let peek = view.peek.as_ref().unwrap();
            assert_eq!(peek.places.len(), 2);
            assert_eq!(peek.places[0].range.start.line, 2);
        });

        // One more call: asked again after the pause, two lenses become three.
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_cursor_position(gpui_component::input::Position::new(4, 0), window, cx);
                editor.focus(window, cx);
            });
        });
        cx.simulate_input("saluta();\n");
        wait_until(cx, "three references", |cx| {
            ends(cx).is_some_and(|lines| {
                lines
                    .get(&0)
                    .is_some_and(|end| end.lenses[0].places.len() == 3)
            })
        });
    }

    fn hint(line: u32, character: u32, label: &str, kind: Option<InlayHintKind>) -> InlayHint {
        InlayHint {
            position: Position::new(line, character),
            label: InlayHintLabel::String(label.to_owned()),
            kind,
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        }
    }

    #[test]
    fn types_go_to_the_end_of_their_lines_with_their_names() {
        let text = "let rows = range();\nlet é_x = f(path);\n    .map(|x| x)\n";
        let hints = [
            hint(0, 8, ": Range<usize>", Some(InlayHintKind::TYPE)),
            hint(1, 7, ": String", Some(InlayHintKind::TYPE)),
            hint(1, 12, "path:", Some(InlayHintKind::PARAMETER)),
            hint(2, 15, "Vec<u8>", Some(InlayHintKind::TYPE)),
        ];
        let lines = by_line(text, &hints, Vec::new());
        assert_eq!(lines[&0].hints, ["rows: Range<usize>"]);
        // The name is counted in UTF-16, as the server counts.
        assert_eq!(lines[&1].hints, ["é_x: String"]);
        assert_eq!(lines[&2].hints, ["Vec<u8>"]);
    }

    #[test]
    fn lenses_with_places_are_kept() {
        let location = json!({
            "uri": "file:///p/src/a.rs",
            "range": { "start": { "line": 3, "character": 4 }, "end": { "line": 3, "character": 8 } },
        });
        let lens = |title: &str, arguments: Option<Vec<Value>>| CodeLens {
            range: Range::new(Position::new(7, 0), Position::new(7, 5)),
            command: Some(Command {
                title: title.to_owned(),
                command: "rust-analyzer.showReferences".to_owned(),
                arguments,
            }),
            data: None,
        };
        let references = lens(
            "2 references",
            Some(vec![
                json!("file:///p/src/a.rs"),
                json!({ "line": 7, "character": 0 }),
                json!([location.clone(), location]),
            ]),
        );
        let found = lens_of(&references).unwrap();
        assert_eq!(
            (found.line, found.kind, found.places.len()),
            (7, LensKind::References, 2)
        );
        assert_eq!(found.places[0].uri, "file:///p/src/a.rs");
        let implementations = lens("1 implementation", Some(vec![json!([])]));
        assert_eq!(
            lens_of(&implementations).unwrap().kind,
            LensKind::Implementations
        );
        // ▶ Run has no places: the test margin runs tests.
        assert!(lens_of(&lens("▶︎ Run", Some(vec![json!({ "kind": "cargo" })]))).is_none());
    }
}
