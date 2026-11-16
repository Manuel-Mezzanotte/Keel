//! The signature of the function being called, above the cursor, with the
//! parameter being written in bold, as VS Code's parameter hints.

use gpui::{
    Anchor, AnyElement, EntityId, FontWeight, HighlightStyle, StyledText, anchored, deferred, point,
};
use gpui_component::ActiveTheme as _;
use lsp::lsp_types::{
    self, ParameterLabel, SignatureHelp, SignatureHelpContext, SignatureHelpParams,
    SignatureHelpTriggerKind, TextDocumentIdentifier, TextDocumentPositionParams,
    request::SignatureHelpRequest,
};

use super::shell::Colors;
use super::*;

actions!(keel, [TriggerParameterHints]);

/// The signature shown, and where.
pub(super) struct SignatureState {
    editor: EntityId,
    pub(super) help: SignatureHelp,
    /// The cursor it was asked for, as a byte offset.
    offset: usize,
}

impl Workbench {
    /// Asks the servers for the signature at the cursor of the active
    /// editor. `invoked` is true for the command and after a function
    /// completion; otherwise it follows typing.
    pub(super) fn request_signature_help(
        &mut self,
        invoked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_signature(invoked, None, window, cx);
    }

    fn request_signature(
        &mut self,
        invoked: bool,
        typed: Option<char>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let editor = pane.editor.clone();
        let uri = pane.uri.clone();
        let (text, offset) = {
            let editor = editor.read(cx);
            (editor.value().to_string(), editor.selected_range().end)
        };
        self.languages.sync(&uri, &text);
        let running = self
            .languages
            .servers_for(&uri)
            .into_iter()
            .find(|running| {
                running
                    .server
                    .capabilities()
                    .is_some_and(|capabilities| capabilities.signature_help_provider.is_some())
            });
        let (Some(running), Ok(parsed)) = (running, uri.parse()) else {
            return;
        };
        let active = self.signature.as_ref().map(|state| state.help.clone());
        let trigger_kind = match (invoked, typed) {
            (true, _) => SignatureHelpTriggerKind::INVOKED,
            (false, Some(_)) => SignatureHelpTriggerKind::TRIGGER_CHARACTER,
            (false, None) => SignatureHelpTriggerKind::CONTENT_CHANGE,
        };
        let position = workbench::position::position_of(&text, offset);
        let request = running
            .server
            .request::<SignatureHelpRequest>(SignatureHelpParams {
                context: Some(SignatureHelpContext {
                    trigger_kind,
                    trigger_character: typed.map(String::from),
                    is_retrigger: active.is_some(),
                    active_signature_help: active,
                }),
                text_document_position_params: TextDocumentPositionParams::new(
                    TextDocumentIdentifier::new(parsed),
                    lsp_types::Position::new(position.line, position.character),
                ),
                work_done_progress_params: Default::default(),
            });
        let editor_id = editor.entity_id();
        self.signature_task = cx.spawn_in(window, async move |this, cx| {
            let help = request.await.ok().flatten();
            this.update(cx, |this, cx| {
                this.signature =
                    help.filter(|help| !help.signatures.is_empty())
                        .map(|help| SignatureState {
                            editor: editor_id,
                            help,
                            offset,
                        });
                cx.notify();
            })
            .ok();
        });
    }

    /// Follows typing: a trigger character such as `(` or `,` asks for the
    /// signature; while one is shown, every change asks again.
    pub(super) fn signature_after_change(
        &mut self,
        index: usize,
        old: &str,
        new: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index != self.active {
            return;
        }
        let Some(change) = lsp::text::content_change(old, new) else {
            return;
        };
        let typed = {
            let mut characters = change.text.chars();
            match (characters.next(), characters.next()) {
                (Some(character), None) => Some(character),
                _ => None,
            }
        };
        let uri = self.panes[index].uri.clone();
        let (triggers, retriggers): (Vec<String>, Vec<String>) = self
            .languages
            .servers_for(&uri)
            .iter()
            .filter_map(|running| {
                running
                    .server
                    .capabilities()?
                    .signature_help_provider
                    .clone()
            })
            .map(|provider| {
                (
                    provider.trigger_characters.unwrap_or_default(),
                    provider.retrigger_characters.unwrap_or_default(),
                )
            })
            .fold(
                (Vec::new(), Vec::new()),
                |(mut all_triggers, mut all_retriggers), (triggers, retriggers)| {
                    all_triggers.extend(triggers);
                    all_retriggers.extend(retriggers);
                    (all_triggers, all_retriggers)
                },
            );
        let typed_trigger = typed.filter(|character| {
            let character = character.to_string();
            triggers.contains(&character)
                || (self.signature.is_some() && retriggers.contains(&character))
        });
        if typed_trigger.is_some() {
            self.request_signature(false, typed_trigger, window, cx);
        } else if self.signature.is_some() {
            self.request_signature(false, None, window, cx);
        }
    }

    /// The cursor moved: a signature on screen follows it, or goes.
    pub(super) fn signature_after_cursor_move(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = &self.signature else {
            return;
        };
        let Some(pane) = self.panes.get(self.active) else {
            self.signature = None;
            return;
        };
        if pane.editor.entity_id() != state.editor {
            self.signature = None;
            cx.notify();
            return;
        }
        let editor = pane.editor.read(cx);
        let range = editor.selected_range();
        if range.start != range.end {
            self.signature = None;
            cx.notify();
        } else if range.end != state.offset {
            self.request_signature(false, None, window, cx);
        }
    }

    pub(super) fn close_signature_help(&mut self, cx: &mut Context<Self>) {
        if self.signature.take().is_some() {
            self.signature_task = Task::ready(());
            cx.notify();
        }
    }

    pub(super) fn signature_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &TriggerParameterHints, window, cx| {
            this.request_signature_help(true, window, cx)
        }))
    }

    /// The signature above the cursor of pane `index`, if one is shown.
    pub(super) fn render_signature_help(
        &self,
        index: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.signature.as_ref()?;
        let pane = self.panes.get(index)?;
        if index != self.active || pane.editor.entity_id() != state.editor {
            return None;
        }
        let editor = pane.editor.read(cx);
        let offset = editor.selected_range().end;
        let caret = editor.range_to_bounds(&(offset..offset))?;
        let help = &state.help;
        let active = help.active_signature.unwrap_or(0) as usize;
        let signature = help.signatures.get(active).or(help.signatures.first())?;
        let parameter = signature
            .active_parameter
            .or(help.active_parameter)
            .unwrap_or(0) as usize;
        let label = signature.label.clone();
        let highlighted = signature
            .parameters
            .as_ref()
            .and_then(|parameters| parameters.get(parameter))
            .and_then(|parameter| parameter_range(&label, &parameter.label));
        let documentation = signature
            .parameters
            .as_ref()
            .and_then(|parameters| parameters.get(parameter))
            .and_then(|parameter| parameter.documentation.as_ref())
            .or(signature.documentation.as_ref())
            .map(documentation_text)
            .filter(|text| !text.is_empty());
        let style = HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            color: Some(colors.accent),
            ..Default::default()
        };
        let text = StyledText::new(label).with_highlights(highlighted.map(|range| (range, style)));
        let count = (help.signatures.len() > 1)
            .then(|| format!("{}/{}", active + 1, help.signatures.len()));
        let theme = cx.theme();
        Some(
            deferred(
                anchored()
                    .position(point(caret.origin.x, caret.origin.y))
                    .anchor(Anchor::BottomLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .mb_1()
                            .max_w(px(560.))
                            .px_2()
                            .py_1()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(colors.border)
                            .bg(theme.popover)
                            .shadow_md()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .children(
                                        count.map(|count| {
                                            div().text_color(colors.faint).child(count)
                                        }),
                                    )
                                    .child(div().text_color(colors.text).child(text)),
                            )
                            .children(documentation.map(|text| {
                                div()
                                    .font_family(theme.font_family.clone())
                                    .text_color(colors.muted)
                                    .child(text)
                            })),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// The byte range of a parameter in the signature's label: given either as
/// text to find or as UTF-16 offsets.
fn parameter_range(label: &str, parameter: &ParameterLabel) -> Option<std::ops::Range<usize>> {
    match parameter {
        ParameterLabel::Simple(text) => {
            let start = label.find(text.as_str())?;
            Some(start..start + text.len())
        }
        ParameterLabel::LabelOffsets([start, end]) => {
            let byte = |units: u32| {
                let mut counted = 0;
                for (at, character) in label.char_indices() {
                    if counted >= units as usize {
                        return at;
                    }
                    counted += character.len_utf16();
                }
                label.len()
            };
            Some(byte(*start)..byte(*end))
        }
    }
}

/// The first paragraph of a documentation, without Markdown's code fences.
fn documentation_text(documentation: &lsp_types::Documentation) -> String {
    let text = match documentation {
        lsp_types::Documentation::String(text) => text.as_str(),
        lsp_types::Documentation::MarkupContent(content) => content.value.as_str(),
    };
    text.split("\n\n")
        .next()
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.starts_with("```"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_are_found_by_text_or_offsets() {
        let label = "useState<S>(initialState: S | (() => S)): [S, Dispatch<S>]";
        assert_eq!(
            parameter_range(
                label,
                &ParameterLabel::Simple("initialState: S | (() => S)".into())
            ),
            Some(12..39)
        );
        // Offsets count UTF-16 units: the accent is one unit, two bytes.
        let label = "saluta(perché: string, n: number)";
        assert_eq!(
            parameter_range(label, &ParameterLabel::LabelOffsets([23, 32]))
                .map(|range| &label[range]),
            Some("n: number")
        );
    }

    #[test]
    fn documentation_keeps_the_first_paragraph() {
        let documentation = lsp_types::Documentation::MarkupContent(lsp_types::MarkupContent {
            kind: lsp_types::MarkupKind::Markdown,
            value: "Returns a stateful value.\nAnd a setter.\n\n```ts\nexample\n```".into(),
        });
        assert_eq!(
            documentation_text(&documentation),
            "Returns a stateful value. And a setter."
        );
    }
}
