//! Edits Claude Code proposes from the terminal (`openDiff`): a tab with
//! the differences between the file and the proposal, and Accept or
//! Reject. Claude Code waits for the answer; accepting gives it the text,
//! and Claude Code writes the file itself, as with VS Code.

use futures::channel::oneshot;
use gpui::AnyElement;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_component::{Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::diff_view::{DiffSource, DiffTab, diff_uri};
use super::shell::Colors;
use super::*;

actions!(keel, [AcceptProposedDiff, RejectProposedDiff]);

/// The uris of proposal tabs start with this.
pub(super) const PROPOSED_SCHEME: &str = "keel-claude-diff:";

/// A proposal waiting for the user.
pub(super) struct ProposedEdit {
    pub(super) tab_name: String,
    pub(super) uri: String,
    pub(super) proposed: String,
    reply: Option<oneshot::Sender<Value>>,
}

fn text(texts: &[&str]) -> Value {
    json!({ "content": texts.iter().map(|text| json!({ "type": "text", "text": text })).collect::<Vec<_>>() })
}

impl Workbench {
    pub(super) fn answer_claude_diff_tool(
        &mut self,
        name: &str,
        arguments: &Value,
        reply: oneshot::Sender<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match name {
            "openDiff" => self.open_claude_diff(arguments, reply, window, cx),
            "close_tab" => {
                let tab_name = arguments["tab_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let uri = diff_uri("", &DiffSource::Proposed(tab_name));
                self.close_claude_diff_tabs(|edit| edit.uri == uri, window, cx);
                let _ = reply.send(text(&["TAB_CLOSED"]));
            }
            _ => {
                let closed = self.close_claude_diff_tabs(|_| true, window, cx);
                let _ = reply.send(text(&[&format!("CLOSED_{closed}_DIFF_TABS")]));
            }
        }
    }

    /// `openDiff`: the file as it is on disk against Claude Code's text.
    fn open_claude_diff(
        &mut self,
        arguments: &Value,
        reply: oneshot::Sender<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = arguments["old_file_path"]
            .as_str()
            .or(arguments["new_file_path"].as_str())
            .map(PathBuf::from)
            .unwrap_or_default();
        let proposed = arguments["new_file_contents"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let tab_name = arguments["tab_name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| path.display().to_string());
        // A file that does not exist yet is new: nothing on the left.
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        let shown = self
            .project
            .root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        let patch = git::unified_patch(&shown, &current, &proposed, 3);
        let language = Language::for_path(&path).highlighter_name();
        let files: Vec<DiffFile> = DiffFile::parse(&patch)
            .map(|files| {
                files
                    .into_iter()
                    .map(|file| file.with_language(language))
                    .collect()
            })
            .unwrap_or_default();
        let source = DiffSource::Proposed(tab_name.clone());
        let uri = diff_uri("", &source);
        // A proposal with the same name replaces the one before.
        self.close_claude_diff_tabs(|edit| edit.uri == uri, window, cx);
        let mode = if self.diff_split {
            DiffMode::Split
        } else {
            DiffMode::Unified
        };
        let state = cx.new(|cx| DiffState::new(files, cx).with_mode(mode));
        self.documents.insert(
            uri.clone(),
            OpenDocument {
                uri: uri.clone(),
                backing: Backing::Diff(DiffTab {
                    path: path.display().to_string(),
                    source,
                    state,
                    title: tab_name.clone(),
                }),
                language_id: "diff".to_owned(),
                version: 1,
                synced_text: String::new(),
                modified: false,
            },
        );
        self.claude_diffs.push(ProposedEdit {
            tab_name,
            uri: uri.clone(),
            proposed,
            reply: Some(reply),
        });
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, OpenAs::Normal, true, window, cx);
        cx.notify();
    }

    /// Closes the proposal tabs `which` picks; those still waiting are
    /// rejected. Returns how many there were.
    fn close_claude_diff_tabs(
        &mut self,
        which: impl Fn(&ProposedEdit) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let uris: Vec<String> = self
            .claude_diffs
            .iter()
            .filter(|edit| which(edit))
            .map(|edit| edit.uri.clone())
            .collect();
        for uri in &uris {
            while let Some((group, tab)) =
                self.groups.iter().enumerate().find_map(|(group, tabs)| {
                    tabs.tabs
                        .iter()
                        .position(|tab| tab.uri == *uri)
                        .map(|tab| (group, tab))
                })
            {
                self.close_tab_now(group, tab, window, cx);
            }
            // Not in a tab any more, or never shown.
            self.documents.remove(uri);
            self.claude_diff_closed(uri);
        }
        uris.len()
    }

    /// The tab of a proposal went away: if Claude Code still waits, the
    /// answer is no.
    pub(super) fn claude_diff_closed(&mut self, uri: &str) {
        let Some(index) = self.claude_diffs.iter().position(|edit| edit.uri == uri) else {
            return;
        };
        let mut edit = self.claude_diffs.remove(index);
        if let Some(reply) = edit.reply.take() {
            let _ = reply.send(text(&["DIFF_REJECTED", &edit.tab_name]));
        }
    }

    /// Accept (`accept`) or Reject the proposal in tab `uri`.
    fn answer_claude_diff(
        &mut self,
        uri: &str,
        accept: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.claude_diffs.iter_mut().find(|edit| edit.uri == uri) else {
            return;
        };
        if let Some(reply) = edit.reply.take() {
            let answer = if accept {
                text(&["FILE_SAVED", &edit.proposed])
            } else {
                text(&["DIFF_REJECTED", &edit.tab_name])
            };
            let _ = reply.send(answer);
        }
        let uri = uri.to_owned();
        self.close_claude_diff_tabs(|edit| edit.uri == uri, window, cx);
        cx.notify();
    }

    /// The proposal in the active tab, for the commands.
    fn active_claude_diff(&self) -> Option<String> {
        let uri = &self.panes.get(self.active)?.uri;
        uri.starts_with(PROPOSED_SCHEME).then(|| uri.clone())
    }

    pub(super) fn claude_diff_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &AcceptProposedDiff, window, cx| {
            if let Some(uri) = this.active_claude_diff() {
                this.answer_claude_diff(&uri, true, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &RejectProposedDiff, window, cx| {
            if let Some(uri) = this.active_claude_diff() {
                this.answer_claude_diff(&uri, false, window, cx);
            }
        }))
    }

    pub(super) fn render_claude_diff(
        &self,
        name: &str,
        tab: &DiffTab,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let uri = diff_uri("", &DiffSource::Proposed(name.to_owned()));
        let waiting = self
            .claude_diffs
            .iter()
            .any(|edit| edit.uri == uri && edit.reply.is_some());
        let (accept, reject) = (uri.clone(), uri);
        let toolbar = div()
            .h(px(34.))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(format!("{} · {}", strings.claude_proposed_edit, tab.path)),
            )
            .child(
                Button::new("claude-diff-split")
                    .ghost()
                    .small()
                    .label(strings.diff_side_by_side)
                    .selected(self.diff_split)
                    .on_click(cx.listener(|this, _, _, cx| this.set_diff_split(true, cx))),
            )
            .child(
                Button::new("claude-diff-unified")
                    .ghost()
                    .small()
                    .label(strings.diff_inline)
                    .selected(!self.diff_split)
                    .on_click(cx.listener(|this, _, _, cx| this.set_diff_split(false, cx))),
            )
            .when(waiting, |toolbar| {
                toolbar
                    .child(
                        Button::new("claude-diff-reject")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::X))
                            .label(strings.claude_reject)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.answer_claude_diff(&reject, false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("claude-diff-accept")
                            .primary()
                            .small()
                            .icon(Icon::new(IconName::Check))
                            .label(strings.claude_accept)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.answer_claude_diff(&accept, true, window, cx)
                            })),
                    )
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div().flex_1().min_h_0().child(
                    Diff::new(&tab.state)
                        .line_number(true)
                        .syntax_highlight(true)
                        .header_visible(false),
                ),
            )
            .into_any_element()
    }

    /// For tests: Accept or Reject without the toolbar.
    #[cfg(test)]
    pub(super) fn answer_claude_diff_for_tests(
        &mut self,
        uri: &str,
        accept: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.answer_claude_diff(uri, accept, window, cx);
    }
}
