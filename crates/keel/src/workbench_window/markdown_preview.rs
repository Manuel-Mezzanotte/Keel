//! The Markdown preview: a tab that renders a Markdown file as GitHub
//! does, next to its editor (Cmd+K V) or in its place (Cmd+Shift+V), and
//! follows every keystroke. gpui-component's text view does the rendering:
//! headings, tables, task lists, code blocks with colors, images, links
//! and quotes, in the theme's colors.

use gpui::{AnyElement, Entity};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::text::{TextView, TextViewState};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::Colors;
use super::*;

actions!(keel, [ShowMarkdownPreview, ShowMarkdownPreviewToSide]);

/// The widest the preview's text gets, as on GitHub.
const READABLE_WIDTH: f32 = 880.;

/// A preview tab.
pub(super) struct PreviewTab {
    /// The Markdown document it shows.
    pub(super) source: String,
    pub(super) state: Entity<TextViewState>,
    pub(super) title: String,
}

/// The uri of the preview of `source`.
pub(super) fn preview_uri(source: &str) -> String {
    format!("keel-preview:{source}")
}

impl Workbench {
    /// Whether the document of group `index` is Markdown that can be previewed.
    pub(super) fn previewable(&self, group: usize) -> Option<String> {
        let uri = self
            .groups
            .get(group)?
            .tabs
            .get(self.groups[group].active)?
            .uri
            .clone();
        let document = self.documents.get(&uri)?;
        (document.language_id == "markdown").then_some(uri)
    }

    /// Opens the preview of the active Markdown file: in a new group to the
    /// side, or in its own group in place of the editor.
    pub(super) fn open_markdown_preview(
        &mut self,
        to_side: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let group = self.active_group();
        let Some(source) = self.previewable(group) else {
            return;
        };
        let uri = preview_uri(&source);
        if !self.documents.contains_key(&uri) {
            let Some(document) = self.documents.get(&source) else {
                return;
            };
            let text = document.synced_text.clone();
            let title = self.strings().preview_title(&document.name());
            let state = cx.new(|cx| {
                TextViewState::markdown(&text, cx)
                    .scrollable(true)
                    .selectable(true)
            });
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::Preview(PreviewTab {
                        source: source.clone(),
                        state,
                        title,
                    }),
                    language_id: "markdown-preview".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
        }
        let column = if to_side { group + 1 } else { group };
        self.show_tab(&uri, column, OpenAs::Normal, true, window, cx);
    }

    /// The group in front.
    fn active_group(&self) -> usize {
        let Some(uri) = self.panes.get(self.active).map(|pane| pane.uri.as_str()) else {
            return 0;
        };
        self.groups
            .iter()
            .position(|group| {
                group
                    .tabs
                    .get(group.active)
                    .is_some_and(|tab| tab.uri == uri)
            })
            .unwrap_or(0)
    }

    /// The text of `source` changed: its previews follow.
    pub(super) fn update_previews(&mut self, source: &str, text: &str, cx: &mut Context<Self>) {
        let states: Vec<Entity<TextViewState>> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::Preview(tab) if tab.source == source => Some(tab.state.clone()),
                _ => None,
            })
            .collect();
        for state in states {
            state.update(cx, |state, cx| state.set_text(text, cx));
        }
    }

    pub(super) fn render_markdown_preview(
        &self,
        tab: &PreviewTab,
        _colors: Colors,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .size_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .h_full()
                    .w_full()
                    .max_w(px(READABLE_WIDTH))
                    .px_8()
                    .child(TextView::new(&tab.state).size_full()),
            )
            .into_any_element()
    }

    /// The preview button at the end of a group's tabs, on Markdown files.
    pub(super) fn render_preview_button(
        &self,
        group: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.previewable(group)?;
        Some(
            Button::new(("markdown-preview", group))
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::Columns2))
                .tooltip(self.strings().command_markdown_preview_side)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus_group(group, window, cx);
                    this.open_markdown_preview(true, window, cx);
                }))
                .into_any_element(),
        )
    }

    pub(super) fn markdown_preview_actions(
        &self,
        root: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &ShowMarkdownPreview, window, cx| {
            this.open_markdown_preview(false, window, cx)
        }))
        .on_action(
            cx.listener(|this, _: &ShowMarkdownPreviewToSide, window, cx| {
                this.open_markdown_preview(true, window, cx)
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        std::fs::write(base.join("README.md"), "# Keel\n\nUn IDE.\n").unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        let path = base.join("README.md");
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        (folder, base, view, cx)
    }

    fn preview_text(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        cx.run_until_parked();
        view.read_with(cx, |view, cx| {
            view.documents
                .values()
                .find_map(|document| match &document.backing {
                    Backing::Preview(tab) => {
                        Some(tab.state.read(cx).rendered_text().as_str().to_owned())
                    }
                    _ => None,
                })
        })
    }

    #[gpui::test]
    fn the_preview_opens_beside_the_editor_and_follows_typing(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        view.update_in(cx, |view, window, cx| {
            view.open_markdown_preview(true, window, cx)
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.groups.len(), 2, "a group to the side");
            let tab = &view.groups[1].tabs[view.groups[1].active];
            assert!(tab.uri.starts_with("keel-preview:file:"));
            assert_eq!(view.documents[&tab.uri].name(), "Anteprima README.md");
        });
        assert_eq!(preview_text(&view, cx).as_deref(), Some("Keel\nUn IDE.\n"));

        // Typing in the editor shows at once in the preview.
        view.update_in(cx, |view, window, cx| {
            let index = view
                .panes
                .iter()
                .position(|pane| pane.uri.ends_with("README.md"))
                .unwrap();
            let editor = view.panes[index].editor.clone();
            editor.update(cx, |editor, cx| {
                editor.set_value("# Keel\n\n- [ ] provare\n", window, cx)
            });
            let uri = view.panes[index].uri.clone();
            view.sync_document_from(&uri, None, window, cx);
        });
        assert_eq!(preview_text(&view, cx).as_deref(), Some("Keel\nprovare\n"));
    }

    #[gpui::test]
    fn the_preview_can_take_the_editors_place(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        view.update_in(cx, |view, window, cx| {
            view.open_markdown_preview(false, window, cx)
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.groups.len(), 1);
            let group = &view.groups[0];
            assert!(group.tabs[group.active].uri.starts_with("keel-preview:"));
            // Both tabs stay in the group.
            assert_eq!(group.tabs.len(), 2);
        });
    }

    /// GitHub's Markdown: tables, task lists, strikethrough, fenced code
    /// and quotes all reach the rendered text, and Keel's own plan renders.
    #[gpui::test]
    fn github_markdown_and_the_plan_render(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
        });
        let sample = "# Titolo\n\n| Fase | Stato |\n|---|---|\n| 7 | in corso |\n\n- [x] fatto\n- [ ] da fare\n\n~~vecchio~~ e https://keel.dev\n\n> citazione\n\n```rust\nfn main() {}\n```\n";
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown(sample, cx)));
        cx.run_until_parked();
        let rendered = state.read_with(cx, |state, _| state.rendered_text().as_str().to_owned());
        for expected in [
            "Titolo",
            "in corso",
            "da fare",
            "vecchio",
            "citazione",
            "fn main() {}",
        ] {
            assert!(rendered.contains(expected), "{expected:?} in {rendered:?}");
        }
        let plan = include_str!("../../../../docs/PIANO.md");
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown(plan, cx)));
        cx.run_until_parked();
        let rendered = state.read_with(cx, |state, _| state.rendered_text().as_str().to_owned());
        assert!(rendered.contains("Le tue prove a mano della fase 6"));
        assert!(
            !rendered.contains("| Sottofase |"),
            "tables are drawn, not shown as text"
        );
    }
}
