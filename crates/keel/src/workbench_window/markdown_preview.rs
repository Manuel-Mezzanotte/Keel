//! The Markdown preview: a tab that renders a Markdown file as GitHub
//! does, next to its editor (Cmd+K V) or in its place (Cmd+Shift+V), and
//! follows every keystroke. gpui-component's text view does the rendering:
//! headings, tables, task lists, code blocks with colors, images, links
//! and quotes, in the theme's colors.
//!
//! Editor and preview scroll together, block by block: the preview's list
//! has one row per top-level block of the Markdown, and Keel reads the
//! blocks' places in the source with the same parser gpui-component uses.

use std::cell::RefCell;
use std::ops::Range;
use std::time::Instant;

use gpui::{AnyElement, Entity, ListOffset};
use gpui_component::RopeExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::text::{TextView, TextViewState};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::Colors;
use super::*;

actions!(keel, [ShowMarkdownPreview, ShowMarkdownPreviewToSide]);

/// The widest the preview's text gets, as on GitHub.
const READABLE_WIDTH: f32 = 880.;

/// How long one side ignores the other after scrolling it, so editor and
/// preview do not push each other back and forth.
const SYNC_QUIET: Duration = Duration::from_millis(250);

/// A preview tab.
pub(super) struct PreviewTab {
    /// The Markdown document it shows.
    pub(super) source: String,
    pub(super) state: Entity<TextViewState>,
    pub(super) title: String,
    /// The folder of the Markdown file, for images and links.
    folder: Option<PathBuf>,
    /// Where each top-level block starts and ends in the source, read when
    /// first needed after a change.
    blocks: RefCell<Option<Vec<Range<usize>>>>,
}

/// Which side scrolled the other last, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SyncSide {
    Editor,
    Preview,
}

/// The markdown options gpui-component parses with.
fn parse_options() -> markdown::ParseOptions {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    options
}

/// The source ranges of a Markdown text's top-level blocks, in order: the
/// rows of gpui-component's preview.
pub(super) fn block_spans(text: &str) -> Vec<Range<usize>> {
    let Ok(markdown::mdast::Node::Root(root)) = markdown::to_mdast(text, &parse_options()) else {
        return Vec::new();
    };
    let mut end = 0;
    root.children
        .iter()
        .map(|node| {
            let span = node.position().map_or(end..end, |position| {
                position.start.offset..position.end.offset
            });
            end = span.end;
            span
        })
        .collect()
}

/// The block that holds `offset`, or the first one after it.
fn block_at(blocks: &[Range<usize>], offset: usize) -> usize {
    blocks
        .iter()
        .position(|block| block.end > offset)
        .unwrap_or(blocks.len().saturating_sub(1))
}

/// GitHub's anchor for a heading: lower case, punctuation dropped, spaces
/// turned into dashes.
pub(super) fn heading_anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(character, ' ' | '-' | '_'))
        .map(|character| if character == ' ' { '-' } else { character })
        .collect()
}

/// The block of the heading whose anchor is `anchor`.
fn heading_block(text: &str, anchor: &str) -> Option<usize> {
    let markdown::mdast::Node::Root(root) = markdown::to_mdast(text, &parse_options()).ok()? else {
        return None;
    };
    let wanted = anchor.trim_start_matches('#').to_lowercase();
    root.children.iter().position(|node| {
        matches!(node, markdown::mdast::Node::Heading(_))
            && heading_anchor(&node.to_string()) == wanted
    })
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
            let folder = document
                .disk_path()
                .and_then(Path::parent)
                .map(Path::to_path_buf);
            let state = cx.new(|cx| {
                TextViewState::markdown(&text, cx)
                    .scrollable(true)
                    .selectable(true)
            });
            // Scrolling the preview scrolls its editor.
            let list = state.read(cx).list_state().clone();
            let workbench = cx.weak_entity();
            let preview = uri.clone();
            list.set_scroll_handler(move |_, _, cx| {
                workbench
                    .update(cx, |this, cx| this.sync_editor_from_preview(&preview, cx))
                    .ok();
            });
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::Preview(PreviewTab {
                        source: source.clone(),
                        state,
                        title,
                        folder,
                        blocks: RefCell::new(None),
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
                Backing::Preview(tab) if tab.source == source => {
                    tab.blocks.borrow_mut().take();
                    Some(tab.state.clone())
                }
                _ => None,
            })
            .collect();
        for state in states {
            state.update(cx, |state, cx| state.set_text(text, cx));
        }
    }

    /// The blocks of a preview's source, read again after a change.
    fn preview_blocks(&self, tab: &PreviewTab) -> Vec<Range<usize>> {
        let mut blocks = tab.blocks.borrow_mut();
        if blocks.is_none() {
            let text = self
                .documents
                .get(&tab.source)
                .map(|document| document.synced_text.as_str())
                .unwrap_or_default();
            *blocks = Some(block_spans(text));
        }
        blocks.clone().unwrap_or_default()
    }

    /// Whether `side` should leave the other alone for now.
    fn sync_paused(&self, side: SyncSide) -> bool {
        self.preview_sync
            .is_some_and(|(last, at)| last != side && at.elapsed() < SYNC_QUIET)
    }

    /// The editor `editor` scrolled or moved: the preview of its file shows
    /// the block at its top line.
    pub(super) fn sync_preview_from_editor(
        &mut self,
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        if self.sync_paused(SyncSide::Editor) {
            return;
        }
        let Some(uri) = self
            .panes
            .iter()
            .find(|pane| pane.editor.entity_id() == editor.entity_id())
            .map(|pane| pane.uri.clone())
        else {
            return;
        };
        let preview = preview_uri(&uri);
        let Some(Backing::Preview(tab)) = self
            .documents
            .get(&preview)
            .map(|document| &document.backing)
        else {
            return;
        };
        // From the scroll offset, which a scroll changes at once, rather
        // than the rows laid out, which lag a frame behind.
        let top = {
            let editor = editor.read(cx);
            let Some(height) = editor.line_height().filter(|height| *height > px(0.)) else {
                return;
            };
            (f32::from(-editor.scroll_offset().y) / f32::from(height))
                .floor()
                .max(0.) as usize
        };
        if self.preview_tops.get(&preview) == Some(&top) {
            return;
        }
        let offset = editor.read(cx).text().line_start_offset(top);
        let blocks = self.preview_blocks(tab);
        if blocks.is_empty() {
            return;
        }
        let list = tab.state.read(cx).list_state().clone();
        list.scroll_to(ListOffset {
            item_ix: block_at(&blocks, offset),
            offset_in_item: px(0.),
        });
        self.preview_tops.insert(preview, top);
        self.preview_sync = Some((SyncSide::Editor, Instant::now()));
        cx.notify();
    }

    /// The preview `preview` scrolled: its file's editors show the line its
    /// top block starts on.
    pub(super) fn sync_editor_from_preview(&mut self, preview: &str, cx: &mut Context<Self>) {
        if self.sync_paused(SyncSide::Preview) {
            return;
        }
        let Some(Backing::Preview(tab)) = self
            .documents
            .get(preview)
            .map(|document| &document.backing)
        else {
            return;
        };
        let item = tab.state.read(cx).list_state().logical_scroll_top().item_ix;
        let blocks = self.preview_blocks(tab);
        let Some(block) = blocks.get(item) else {
            return;
        };
        let source = tab.source.clone();
        let text = self
            .documents
            .get(&source)
            .map(|document| document.synced_text.clone())
            .unwrap_or_default();
        let line = text[..block.start.min(text.len())].matches('\n').count();
        self.preview_sync = Some((SyncSide::Preview, Instant::now()));
        let editors: Vec<Entity<EditorState>> = self
            .panes
            .iter()
            .filter(|pane| pane.uri == source)
            .map(|pane| pane.editor.clone())
            .collect();
        for editor in editors {
            editor.update(cx, |editor, cx| {
                let Some(height) = editor.line_height() else {
                    return;
                };
                let x = editor.scroll_offset().x;
                editor.set_scroll_offset(gpui::point(x, -(height * line as f32)), cx);
            });
        }
        self.preview_tops.insert(preview.to_owned(), line);
    }

    /// A link clicked in a preview: a web page opens in the browser, `#title`
    /// scrolls to its heading, and a file of the project opens in Keel.
    fn follow_preview_link(
        &mut self,
        preview: &str,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Backing::Preview(tab)) = self
            .documents
            .get(preview)
            .map(|document| &document.backing)
        else {
            return;
        };
        if url.contains("://") || url.starts_with("mailto:") {
            cx.open_url(url);
            return;
        }
        if let Some(anchor) = url.strip_prefix('#') {
            let text = self
                .documents
                .get(&tab.source)
                .map(|document| document.synced_text.clone())
                .unwrap_or_default();
            if let Some(block) = heading_block(&text, anchor) {
                tab.state.read(cx).list_state().scroll_to(ListOffset {
                    item_ix: block,
                    offset_in_item: px(0.),
                });
                cx.notify();
            }
            return;
        }
        let Some(folder) = tab.folder.clone() else {
            return;
        };
        let path = percent_decode(url.split('#').next().unwrap_or(url));
        let target = if Path::new(&path).is_absolute() {
            PathBuf::from(&path)
        } else {
            folder.join(&path)
        };
        if !target.exists() {
            self.status_message = Some(
                self.strings()
                    .could_not_open(&target.display().to_string(), "not found"),
            );
            cx.notify();
            return;
        }
        // In the group of the Markdown's editor, not over the preview.
        let source = tab.source.clone();
        if let Some(group) = self
            .groups
            .iter()
            .position(|group| group.tabs.iter().any(|tab| tab.uri == source))
        {
            self.focus_group(group, window, cx);
        }
        self.open_file(&target, window, cx);
    }

    /// A task box ticked in a preview: its `[ ]` becomes `[x]` in the editor.
    fn toggle_task(
        &mut self,
        preview: &str,
        marker: Range<usize>,
        checked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Backing::Preview(tab)) = self
            .documents
            .get(preview)
            .map(|document| &document.backing)
        else {
            return;
        };
        let source = tab.source.clone();
        let Some(editor) = self
            .panes
            .iter()
            .find(|pane| pane.uri == source)
            .map(|pane| pane.editor.clone())
        else {
            return;
        };
        editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let is_marker = text
                .get(marker.clone())
                .is_some_and(|found| matches!(found, "[ ]" | "[x]" | "[X]"));
            if !is_marker {
                return;
            }
            let selection = editor.selected_range();
            editor.set_selected_range(marker, cx);
            editor.replace(if checked { "[x]" } else { "[ ]" }, window, cx);
            editor.set_selected_range(selection, cx);
        });
    }

    pub(super) fn render_markdown_preview(
        &self,
        tab: &PreviewTab,
        _colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preview = preview_uri(&tab.source);
        let folder = tab.folder.clone();
        let links = {
            let workbench = cx.weak_entity();
            let preview = preview.clone();
            move |url: &gpui::SharedString,
                  _: &gpui::ClickEvent,
                  window: &mut Window,
                  cx: &mut App| {
                workbench
                    .update(cx, |this, cx| {
                        this.follow_preview_link(&preview, url, window, cx)
                    })
                    .ok();
            }
        };
        let tasks = {
            let workbench = cx.weak_entity();
            move |event: &gpui_component::text::TaskToggleEvent,
                  window: &mut Window,
                  cx: &mut App| {
                let (marker, checked) = (event.marker_range(), event.checked());
                workbench
                    .update(cx, |this, cx| {
                        this.toggle_task(&preview, marker, checked, window, cx)
                    })
                    .ok();
            }
        };
        // The element's settings win over the state's: without these the
        // preview neither scrolls nor lets text be selected.
        let text_view = TextView::new(&tab.state)
            .scrollable(true)
            .selectable(true)
            .size_full()
            .image_source(move |uri| {
                let address: &str = uri.as_ref();
                match &folder {
                    Some(folder) if !address.contains("://") && !address.starts_with("data:") => {
                        let path = percent_decode(address);
                        let path = if Path::new(&path).is_absolute() {
                            PathBuf::from(path)
                        } else {
                            folder.join(path)
                        };
                        gpui::ImageSource::from(path)
                    }
                    _ => gpui::ImageSource::from(uri.clone()),
                }
            })
            .on_link_click(links)
            .on_task_toggle(tasks);
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
                    .child(text_view),
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
                .small()
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

/// Turns `%20` and friends back into characters, as in a link to a file.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(value) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(value);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
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

    #[test]
    fn blocks_anchors_and_paths() {
        let text = "# Titolo\n\nPrimo paragrafo.\n\n- uno\n- due\n\n## Le tue prove, a mano!\n";
        let blocks = block_spans(text);
        assert_eq!(blocks.len(), 4);
        assert_eq!(&text[blocks[2].clone()], "- uno\n- due\n");
        // A line inside the list, and the blank line after it, both belong
        // to the list or the block after.
        assert_eq!(block_at(&blocks, text.find("due").unwrap()), 2);
        assert_eq!(block_at(&blocks, text.find("## Le").unwrap() - 1), 3);
        assert_eq!(
            heading_anchor("Le tue prove, a mano!"),
            "le-tue-prove-a-mano"
        );
        assert_eq!(heading_block(text, "#le-tue-prove-a-mano"), Some(3));
        assert_eq!(percent_decode("docs/Il%20piano.md"), "docs/Il piano.md");
        assert_eq!(percent_decode("100%"), "100%");
    }

    fn preview_list(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> gpui::ListState {
        view.read_with(cx, |view, cx| {
            view.documents
                .values()
                .find_map(|document| match &document.backing {
                    Backing::Preview(tab) => Some(tab.state.read(cx).list_state().clone()),
                    _ => None,
                })
                .unwrap()
        })
    }

    #[gpui::test]
    fn links_tasks_and_scrolling(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        let mut text =
            String::from("# Inizio\n\n- [ ] da fare\n\n[il piano](docs/Il%20piano.md)\n\n");
        for index in 0..60 {
            if index == 21 {
                text.push_str("## Metà strada\n\n");
            }
            text.push_str(&format!("Paragrafo {index}.\n\n"));
        }
        text.push_str("## Fine\n");
        std::fs::create_dir_all(base.join("docs")).unwrap();
        std::fs::write(base.join("docs/Il piano.md"), "# Piano\n").unwrap();
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| editor.set_value(text.clone(), window, cx));
            let uri = view.panes[view.active].uri.clone();
            view.sync_document_from(&uri, None, window, cx);
            view.open_markdown_preview(true, window, cx);
        });
        // The preview's rows exist once it is drawn.
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
        let preview = view.read_with(cx, |view, _| view.panes[view.active].uri.clone());

        // A link to a heading scrolls the preview there.
        view.update_in(cx, |view, window, cx| {
            view.follow_preview_link(&preview, "#metà-strada", window, cx)
        });
        assert_eq!(preview_list(&view, cx).logical_scroll_top().item_ix, 24);

        // Ticking the box changes the Markdown.
        let marker = text.find("[ ]").unwrap();
        view.update_in(cx, |view, window, cx| {
            view.toggle_task(&preview, marker..marker + 3, true, window, cx)
        });
        cx.run_until_parked();
        let source = view.read_with(cx, |view, cx| {
            let pane = view
                .panes
                .iter()
                .find(|pane| pane.uri.ends_with("README.md"))
                .unwrap();
            pane.editor.read(cx).value().to_string()
        });
        assert!(source.contains("- [x] da fare"));

        // Scrolling the preview scrolls the editor to the block's line.
        preview_list(&view, cx).scroll_to(ListOffset {
            item_ix: 5,
            offset_in_item: px(0.),
        });
        view.update(cx, |view, cx| {
            view.preview_sync = None;
            view.sync_editor_from_preview(&preview, cx)
        });
        cx.run_until_parked();
        let top = view.read_with(cx, |view, cx| {
            let pane = view
                .panes
                .iter()
                .find(|pane| pane.uri.ends_with("README.md"))
                .unwrap();
            let editor = pane.editor.read(cx);
            let height = editor.line_height().map(f32::from).unwrap_or(1.);
            (-f32::from(editor.scroll_offset().y) / height).round() as usize
        });
        // Block 5 is "Paragrafo 2.", on line 10.
        assert_eq!(top, 10);

        // Scrolling the editor scrolls the preview: line 30 is "Paragrafo 12.",
        // block 15.
        view.update(cx, |view, cx| {
            let pane = view
                .panes
                .iter()
                .find(|pane| pane.uri.ends_with("README.md"))
                .unwrap();
            pane.editor.update(cx, |editor, cx| {
                let height = editor.line_height().unwrap();
                editor.set_scroll_offset(gpui::point(px(0.), -(height * 30.)), cx);
            });
            view.preview_sync = None;
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        // A real scroll notifies the editor right away; here the offset is
        // applied by the frame just drawn.
        view.update(cx, |view, cx| {
            let pane = view
                .panes
                .iter()
                .find(|pane| pane.uri.ends_with("README.md"))
                .unwrap();
            let editor = pane.editor.clone();
            view.sync_preview_from_editor(&editor, cx);
        });
        cx.run_until_parked();
        assert_eq!(preview_list(&view, cx).logical_scroll_top().item_ix, 15);

        // A link to a file opens it, in the editor's group.
        view.update_in(cx, |view, window, cx| {
            view.follow_preview_link(&preview, "docs/Il%20piano.md#piano", window, cx)
        });
        view.read_with(cx, |view, _| {
            assert!(view.panes[view.active].uri.ends_with("Il%20piano.md"));
            assert_eq!(
                view.groups[0].tabs[view.groups[0].active].uri,
                view.panes[view.active].uri
            );
        });
    }

    /// How long finding the blocks of Keel's plan takes, as the first scroll
    /// after a change does. Run with `cargo test -p keel --release
    /// timing_preview_blocks -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing_preview_blocks() {
        let plan = include_str!("../../../../docs/PIANO.md");
        let started = std::time::Instant::now();
        let blocks = block_spans(plan);
        println!(
            "{} lines, {} blocks found in {:?}",
            plan.lines().count(),
            blocks.len(),
            started.elapsed()
        );
    }
}
