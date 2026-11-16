//! A commit of the graph: its details and changed files under the graph,
//! each file's differences in a tab, and the commands of its menu, which
//! ask before changing anything.

use git::{ChangedFile, Commit, CommitDetails, RefLabel};
use gpui::{AnyElement, StatefulInteractiveElement as _};
use gpui_component::input::{Input, InputState};
use gpui_component::menu::PopupMenu;
use gpui_component::{ActiveTheme as _, Sizable as _, WindowExt as _};

use super::diff_view::DiffSource;
use super::git_graph::format_date;
use super::shell::Colors;
use super::*;

/// What a commit's menu can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitCommand {
    Checkout,
    CreateBranch,
    CreateTag,
    CherryPick,
    Revert,
    CopyHash,
    CopySubject,
    CheckoutBranch(String),
    MergeBranch(String),
    DeleteBranch(String),
    DeleteTag(String),
}

/// An entry of a commit's menu.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = git_graph, no_json)]
pub struct CommitMenu {
    pub hash: String,
    pub command: CommitCommand,
}

/// The commit shown under the graph.
pub(super) struct CommitView {
    pub(super) hash: String,
    details: Option<CommitDetails>,
    pub(super) files: Vec<ChangedFile>,
}

/// The height of the commit's panel under the graph.
const PANEL_HEIGHT: f32 = 220.;

/// The commit's menu, as Git Graph's: commands on the commit, then on
/// each branch and tag pointing at it.
pub(super) fn commit_menu(
    strings: &'static Strings,
    commit: &Commit,
    menu: PopupMenu,
) -> PopupMenu {
    let hash = commit.hash.clone();
    let item = |command: CommitCommand| {
        Box::new(CommitMenu {
            hash: hash.clone(),
            command,
        })
    };
    let mut menu = menu
        .menu(strings.graph_checkout, item(CommitCommand::Checkout))
        .menu(
            strings.graph_create_branch,
            item(CommitCommand::CreateBranch),
        )
        .menu(strings.graph_create_tag, item(CommitCommand::CreateTag))
        .separator()
        .menu(strings.graph_cherry_pick, item(CommitCommand::CherryPick))
        .menu(strings.graph_revert, item(CommitCommand::Revert))
        .separator()
        .menu(strings.graph_copy_hash, item(CommitCommand::CopyHash))
        .menu(strings.graph_copy_subject, item(CommitCommand::CopySubject));
    for label in &commit.refs {
        match label {
            RefLabel::Head(name) | RefLabel::Branch(name) => {
                menu = menu
                    .separator()
                    .menu(
                        strings.graph_checkout_branch(name),
                        item(CommitCommand::CheckoutBranch(name.clone())),
                    )
                    .menu(
                        strings.graph_merge_branch(name),
                        item(CommitCommand::MergeBranch(name.clone())),
                    )
                    .menu(
                        strings.graph_delete_branch(name),
                        item(CommitCommand::DeleteBranch(name.clone())),
                    );
            }
            RefLabel::Remote(name) => {
                menu = menu.separator().menu(
                    strings.graph_checkout_branch(name),
                    item(CommitCommand::CheckoutBranch(name.clone())),
                );
            }
            RefLabel::Tag(name) => {
                menu = menu.separator().menu(
                    strings.graph_delete_tag(name),
                    item(CommitCommand::DeleteTag(name.clone())),
                );
            }
            RefLabel::DetachedHead => {}
        }
    }
    menu
}

impl Workbench {
    /// A click on a row: the commit's details and files come under the
    /// graph; a second click closes them. The row of uncommitted changes
    /// opens the Source Control view.
    pub(super) fn select_commit(
        &mut self,
        hash: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if hash == "*" {
            window.dispatch_action(Box::new(ShowSourceControl), cx);
            return;
        }
        let Some(tab) = self.graph_tab_mut() else {
            return;
        };
        if tab.selected.as_deref() == Some(hash) {
            tab.selected = None;
            self.commit_view = None;
            cx.notify();
            return;
        }
        tab.selected = Some(hash.to_owned());
        self.commit_view = Some(CommitView {
            hash: hash.to_owned(),
            details: None,
            files: Vec::new(),
        });
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let wanted = hash.to_owned();
        let task = cx.background_spawn(async move {
            let details = repo.commit_details(&wanted).ok();
            let files = repo.commit_files(&wanted).unwrap_or_default();
            (wanted, details, files)
        });
        cx.spawn(async move |this, cx| {
            let (hash, details, files) = task.await;
            this.update(cx, |this, cx| {
                if let Some(view) = this.commit_view.as_mut().filter(|view| view.hash == hash) {
                    view.details = details;
                    view.files = files;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn run_commit_command(
        &mut self,
        action: &CommitMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(commit) = self
            .graph_tab()
            .and_then(|tab| tab.commits.iter().find(|commit| commit.hash == action.hash))
            .cloned()
        else {
            return;
        };
        let hash = commit.hash.clone();
        let short = commit.short_hash().to_owned();
        let is_merge = commit.parents.len() > 1;
        match &action.command {
            CommitCommand::CopyHash => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(hash));
            }
            CommitCommand::CopySubject => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(commit.subject.clone()));
            }
            CommitCommand::Checkout => {
                self.confirm_git(
                    strings.graph_checkout_question(&short),
                    strings.graph_checkout,
                    move |repo| repo.checkout_commit(&hash),
                    window,
                    cx,
                );
            }
            CommitCommand::CheckoutBranch(name) => {
                let remote = commit
                    .refs
                    .iter()
                    .any(|label| matches!(label, RefLabel::Remote(remote) if remote == name));
                let branch = git::Branch {
                    name: name.clone(),
                    remote,
                    current: false,
                };
                self.run_git(
                    strings.graph_checkout,
                    move |repo| repo.switch(&branch),
                    |_, _, _| {},
                    window,
                    cx,
                );
            }
            CommitCommand::CherryPick => self.confirm_git(
                strings.graph_cherry_pick_question(&short),
                strings.graph_cherry_pick,
                move |repo| repo.cherry_pick(&hash, is_merge),
                window,
                cx,
            ),
            CommitCommand::Revert => self.confirm_git(
                strings.graph_revert_question(&short),
                strings.graph_revert,
                move |repo| repo.revert(&hash, is_merge),
                window,
                cx,
            ),
            CommitCommand::MergeBranch(name) => {
                let name = name.clone();
                self.confirm_git(
                    strings.graph_merge_question(&name),
                    strings.graph_merge,
                    move |repo| repo.merge(&name),
                    window,
                    cx,
                );
            }
            CommitCommand::DeleteBranch(name) => {
                let name = name.clone();
                self.confirm_git(
                    strings.graph_delete_branch_question(&name),
                    strings.graph_delete,
                    move |repo| repo.delete_branch(&name, false),
                    window,
                    cx,
                );
            }
            CommitCommand::DeleteTag(name) => {
                let name = name.clone();
                self.confirm_git(
                    strings.graph_delete_tag_question(&name),
                    strings.graph_delete,
                    move |repo| repo.delete_tag(&name),
                    window,
                    cx,
                );
            }
            CommitCommand::CreateBranch => self.ask_ref_name(hash, true, window, cx),
            CommitCommand::CreateTag => self.ask_ref_name(hash, false, window, cx),
        }
    }

    /// Asks before a git command that changes the repository, then runs it.
    fn confirm_git(
        &mut self,
        question: String,
        label: &'static str,
        operation: impl FnOnce(&git::Repo) -> git::Result<()> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let answer = window.prompt(
            PromptLevel::Info,
            &question,
            None,
            &[label, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.run_git(label, operation, |_, _, _| {}, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// Asks the name of a branch or tag to create at `hash`.
    fn ask_ref_name(
        &mut self,
        hash: String,
        branch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let placeholder = if branch {
            strings.scm_branch_name
        } else {
            strings.graph_tag_name
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let this = cx.weak_entity();
        let field = input.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let this = this.clone();
            let shown = field.clone();
            let field = field.clone();
            let hash = hash.clone();
            dialog
                .w(px(380.))
                .title(if branch {
                    strings.graph_create_branch
                } else {
                    strings.graph_create_tag
                })
                .on_ok(move |_, window, cx| {
                    let name = field.read(cx).value().trim().replace(' ', "-");
                    if name.is_empty() {
                        return true;
                    }
                    let hash = hash.clone();
                    this.update(cx, |this, cx| {
                        if branch {
                            this.run_git(
                                strings.graph_create_branch,
                                move |repo| repo.create_branch_at(&name, &hash, true),
                                |_, _, _| {},
                                window,
                                cx,
                            );
                        } else {
                            this.run_git(
                                strings.graph_create_tag,
                                move |repo| repo.create_tag(&name, &hash, None),
                                |_, _, _| {},
                                window,
                                cx,
                            );
                        }
                    })
                    .ok();
                    true
                })
                .child(Input::new(&shown).small())
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(super) fn commit_view_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, action: &CommitMenu, window, cx| {
            this.run_commit_command(action, window, cx)
        }))
    }

    /// The panel under the graph with the chosen commit.
    pub(super) fn render_commit_details(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let view = self.commit_view.as_ref()?;
        let strings = self.strings();
        let italian = self.language == UiLanguage::Italian;
        let theme = cx.theme();
        let info = match &view.details {
            None => div()
                .text_xs()
                .text_color(colors.muted)
                .child(strings.graph_loading),
            Some(details) => div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .child(
                    div()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(colors.muted)
                        .child(format!("{} {}", strings.graph_column_commit, details.hash)),
                )
                .child(div().text_color(colors.muted).child(format!(
                    "{}: {}",
                    strings.graph_parents,
                    details
                        .parents
                        .iter()
                        .map(|parent| &parent[..parent.len().min(8)])
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
                .child(div().text_color(colors.muted).child(format!(
                    "{}: {} <{}> · {}",
                    strings.graph_column_author,
                    details.author,
                    details.email,
                    format_date(details.time, italian)
                )))
                .child(
                    div()
                        .pt_2()
                        .text_sm()
                        .text_color(colors.text)
                        .whitespace_normal()
                        .child(details.message.clone()),
                ),
        };
        let hash = view.hash.clone();
        let files = view.files.iter().enumerate().map(|(index, file)| {
            let (folder, name) = match file.path.rsplit_once('/') {
                Some((folder, name)) => (folder.to_owned(), name.to_owned()),
                None => (String::new(), file.path.clone()),
            };
            let color = match file.status {
                'A' => theme.success,
                'D' => theme.danger,
                'R' | 'C' => theme.info,
                _ => theme.warning,
            };
            let (hash, path, original) = (hash.clone(), file.path.clone(), file.original.clone());
            div()
                .id(("commit-file", index))
                .h(px(22.))
                .px_2()
                .flex()
                .items_center()
                .gap_1p5()
                .rounded(px(6.))
                .text_sm()
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_diff(
                        &path,
                        DiffSource::Commit(hash.clone(), original.clone()),
                        window,
                        cx,
                    )
                }))
                .child(
                    div()
                        .w(px(12.))
                        .text_xs()
                        .text_color(color)
                        .child(file.status.to_string()),
                )
                .child(div().flex_none().text_color(colors.text).child(name))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(colors.faint)
                        .child(folder),
                )
        });
        Some(
            div()
                .h(px(PANEL_HEIGHT))
                .flex_none()
                .flex()
                .border_t_1()
                .border_color(colors.border)
                .child(
                    div()
                        .id("commit-info")
                        .flex_1()
                        .min_w_0()
                        .p_3()
                        .overflow_y_scroll()
                        .child(info),
                )
                .child(
                    div()
                        .id("commit-files")
                        .w(px(360.))
                        .flex_none()
                        .p_2()
                        .border_l_1()
                        .border_color(colors.border)
                        .overflow_y_scroll()
                        .children(files),
                )
                .into_any_element(),
        )
    }
}
