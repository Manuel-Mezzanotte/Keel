//! The Outline section of the Explorer, as VS Code's: the symbols of the
//! file in front, nested, with the one the cursor is in highlighted; a
//! click goes there. It follows the file as you type, after a pause.

use gpui::{AnyElement, ClickEvent, StatefulInteractiveElement as _};
use lsp_types::request::DocumentSymbolRequest;
use lsp_types::{DocumentSymbolParams, SymbolKind};

use super::code_navigation::{FlatSymbol, flatten_symbols, kind_name};
use super::shell::{Colors, chevron_turn, section_header};
use super::*;

/// How long the outline waits after typing before asking again.
const PAUSE: Duration = Duration::from_millis(700);

#[derive(Default)]
pub(super) struct Outline {
    /// The document the symbols are of.
    uri: String,
    symbols: Vec<FlatSymbol>,
    pub(super) collapsed: bool,
    task: Option<Task<()>>,
}

/// A symbol's letter and colour, as the editors' outlines draw them.
fn badge(kind: SymbolKind, cx: &App) -> (&'static str, Hsla) {
    let theme = cx.theme();
    let violet = gpui::hsla(0.74, 0.55, 0.65, 1.);
    match kind {
        SymbolKind::FUNCTION | SymbolKind::METHOD | SymbolKind::CONSTRUCTOR => ("ƒ", violet),
        SymbolKind::CLASS | SymbolKind::STRUCT => ("C", theme.warning),
        SymbolKind::INTERFACE | SymbolKind::TYPE_PARAMETER => ("I", theme.info),
        SymbolKind::ENUM | SymbolKind::ENUM_MEMBER => ("E", theme.warning),
        SymbolKind::CONSTANT => ("c", theme.info),
        SymbolKind::VARIABLE => ("v", theme.info),
        SymbolKind::FIELD | SymbolKind::PROPERTY => ("p", theme.success),
        SymbolKind::MODULE | SymbolKind::NAMESPACE | SymbolKind::PACKAGE => {
            ("{}", theme.muted_foreground)
        }
        _ => ("•", theme.muted_foreground),
    }
}

impl Workbench {
    pub(super) fn outline_uri(&self) -> String {
        self.outline.uri.clone()
    }

    /// Asks the active file's server for its symbols, now or after a pause.
    pub(super) fn refresh_outline(&mut self, pause: bool, cx: &mut Context<Self>) {
        if self.outline.collapsed {
            return;
        }
        let Some(pane) = self.panes.get(self.active) else {
            self.outline.symbols.clear();
            return;
        };
        let uri = pane.uri.clone();
        let text = pane.editor.read(cx).value().to_string();
        self.languages.sync(&uri, &text);
        let Some(running) = self
            .languages
            .servers_for(&uri)
            .into_iter()
            .find(|running| {
                running
                    .server
                    .capabilities()
                    .is_some_and(|capabilities| capabilities.document_symbol_provider.is_some())
            })
        else {
            if self.outline.uri != uri {
                self.outline.uri = uri;
                self.outline.symbols.clear();
            }
            return;
        };
        let Ok(document) = uri.parse() else {
            return;
        };
        let running = running.clone();
        self.outline.task = Some(cx.spawn(async move |this, cx| {
            if pause {
                cx.background_executor().timer(PAUSE).await;
            }
            let request = running
                .server
                .request::<DocumentSymbolRequest>(DocumentSymbolParams {
                    text_document: lsp_types::TextDocumentIdentifier::new(document),
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                });
            let symbols = flatten_symbols(request.await.ok().flatten());
            this.update(cx, |this, cx| {
                this.outline.uri = uri;
                this.outline.symbols = symbols;
                cx.notify();
            })
            .ok();
        }));
    }

    /// The symbol the cursor is in: the last one starting above it.
    fn current_symbol(&self, cx: &App) -> Option<usize> {
        let pane = self.panes.get(self.active)?;
        if pane.uri != self.outline.uri {
            return None;
        }
        let line = pane.editor.read(cx).cursor_position().line;
        self.outline
            .symbols
            .iter()
            .rposition(|(_, _, _, _, range)| range.start.line <= line)
    }

    pub(super) fn render_outline(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // Only a file in front has symbols: not Settings, a diff or an image.
        let pane = self.panes.get(self.active)?;
        let document = self.documents.get(&pane.uri)?;
        if !matches!(document.backing, Backing::File(_) | Backing::Untitled) {
            return None;
        }
        let strings = self.strings();
        let open = !self.outline.collapsed;
        let turn = chevron_turn("outline-turn", open, self.look, window, cx);
        let current = self.current_symbol(cx);
        let uri = self.outline.uri.clone();
        let rows: Vec<AnyElement> = self
            .outline
            .symbols
            .iter()
            .enumerate()
            .map(|(index, (depth, name, kind, detail, range))| {
                let (letter, color) = badge(*kind, cx);
                let range = *range;
                let uri = uri.clone();
                div()
                    .id(("outline-symbol", index))
                    .h(px(24.))
                    .mx_1()
                    .pl(px(8. + *depth as f32 * 12.))
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .rounded(px(6.))
                    .text_sm()
                    .cursor_pointer()
                    .when(current == Some(index), |row| row.bg(colors.hover))
                    .hover(|row| row.bg(colors.hover))
                    .tooltip({
                        let kind = kind_name(*kind);
                        move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(kind).build(window, cx)
                        }
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.open_lsp_range(&uri, range, OpenAs::Normal, window, cx)
                    }))
                    .child(
                        div()
                            .flex_none()
                            .w(px(14.))
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(color)
                            .child(letter),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(220.))
                            .truncate()
                            .child(name.clone()),
                    )
                    .children(detail.clone().map(|detail| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(detail)
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let header = section_header("outline-header", strings.outline_title, turn, colors)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.outline.collapsed = !this.outline.collapsed;
                if !this.outline.collapsed {
                    this.refresh_outline(false, cx);
                }
                this.session_changed(cx);
                cx.notify();
            }));
        Some(
            div()
                .flex_none()
                .max_h(px(260.))
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(colors.border)
                .pt_1()
                .pb_1()
                .child(header)
                .when(open && empty, |section| {
                    section.child(
                        div()
                            .px_3()
                            .py_1()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(strings.outline_empty),
                    )
                })
                .when(open && !empty, |section| {
                    section.child(
                        div()
                            .id("outline-symbols")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(rows),
                    )
                })
                .into_any_element(),
        )
    }
}
