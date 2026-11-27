//! Differences in a tab: a file's changes, staged or not, side by side or
//! in one column, drawn by gpui-component's diff view; and files in merge
//! conflict, resolved one conflict at a time.

use gpui::{AnyElement, Div, Entity};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_component::{Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, note};
use super::*;

actions!(keel, [NextDiffChange, PreviousDiffChange, OpenAllChanges]);

/// Which differences of a file a tab shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DiffSource {
    /// Not staged yet: the staged version against the file on disk.
    Changes,
    /// Staged: the last commit against the staged version.
    Staged,
    /// A file in merge conflict, with its conflicts to resolve.
    Conflict,
    /// What a commit changed in the file, from Git Graph: the commit's
    /// hash, and where a renamed file came from.
    Commit(String, Option<String>),
    /// An edit Claude Code proposes, by the name of its tab; the patch is
    /// made from the file and the proposed text.
    Proposed(String),
    /// Every change not staged yet, file after file, in one page.
    AllChanges,
    /// A version kept by the local history, by its file, against the file
    /// now; the tab's path is the file's own.
    LocalHistory(String),
    /// The clipboard against the file in view.
    Clipboard,
    /// Two files of the project, from the Explorer: the tab's path is the
    /// first, this the second.
    Files(String),
    /// A change the agent made to a file, by its path: what Keel kept
    /// before against the file now.
    Agent(String),
    /// What a pull request changes, from where it parted to its last
    /// commit: a file, or all of them with an empty path.
    Pull {
        number: u64,
        from: String,
        to: String,
    },
    /// What the branch changed since it parted from `base` (where it
    /// parted: `from`), as a pull request would show it, before opening one.
    Branch { base: String, from: String },
}

/// A tab with differences.
pub(super) struct DiffTab {
    /// The file, from the repository's top folder.
    pub(super) path: String,
    pub(super) source: DiffSource,
    pub(super) state: Entity<DiffState>,
    pub(super) title: String,
    /// Keel's own view of the two versions, for all but conflicts and
    /// Claude's proposals.
    pub(super) doc: Option<super::diff_pane::DiffDoc>,
    /// A changed picture, shown as pictures.
    pub(super) images: Option<super::image_diff::ImageDiff>,
}

/// The uri of a differences tab, distinct for each file and source.
pub(super) fn diff_uri(path: &str, source: &DiffSource) -> String {
    let kind = match source {
        DiffSource::Changes => "changes",
        DiffSource::Staged => "staged",
        DiffSource::Conflict => "conflict",
        DiffSource::Commit(hash, _) => hash.as_str(),
        DiffSource::Proposed(name) => return format!("keel-claude-diff:{name}"),
        DiffSource::AllChanges => return "keel-diff:all-changes".to_owned(),
        DiffSource::LocalHistory(version) => return format!("keel-history:{version}"),
        DiffSource::Clipboard => return format!("keel-clipboard:{path}"),
        DiffSource::Files(other) => return format!("keel-compare:{path}|{other}"),
        DiffSource::Agent(path) => return format!("keel-agent-diff:{path}"),
        DiffSource::Pull { number, to, .. } => {
            return format!("keel-pull:{number}/{to}/{path}");
        }
        DiffSource::Branch { base, .. } => return format!("keel-branch:{base}/{path}"),
    };
    format!("keel-diff:{kind}/{path}")
}

impl Workbench {
    /// What a differences tab shows: the patch git makes, or the conflicts
    /// read from the file.
    fn diff_files(&self, path: &str, source: &DiffSource) -> Result<Vec<DiffFile>, String> {
        let repo = self.scm.repo.as_ref().ok_or_else(String::new)?;
        let language = Language::for_path(Path::new(path)).highlighter_name();
        let files = match source {
            DiffSource::Conflict => {
                let text = std::fs::read_to_string(repo.absolute(path))
                    .map_err(|error| error.to_string())?;
                vec![
                    DiffFile::parse_conflicts(path.to_owned(), &text)
                        .map_err(|error| error.message().to_string())?,
                ]
            }
            DiffSource::Commit(hash, original) => {
                let patch = repo
                    .commit_file_diff(hash, path, original.as_deref())
                    .map_err(|error| error.to_string())?;
                DiffFile::parse(&patch).map_err(|error| error.message().to_string())?
            }
            DiffSource::Proposed(_)
            | DiffSource::Agent(_)
            | DiffSource::AllChanges
            | DiffSource::LocalHistory(_)
            | DiffSource::Clipboard
            | DiffSource::Pull { .. }
            | DiffSource::Branch { .. }
            | DiffSource::Files(_) => {
                return Err("not a git difference".to_owned());
            }
            DiffSource::Changes | DiffSource::Staged => {
                let patch = repo
                    .diff(path, *source == DiffSource::Staged)
                    .map_err(|error| error.to_string())?;
                DiffFile::parse(&patch).map_err(|error| error.message().to_string())?
            }
        };
        Ok(files
            .into_iter()
            .map(|file| file.with_language(language))
            .collect())
    }

    /// Opens a file's differences in a preview tab, or brings it forward.
    pub(super) fn open_diff(
        &mut self,
        path: &str,
        source: DiffSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uri = diff_uri(path, &source);
        // The clipboard may hold something else since the last time.
        if source == DiffSource::Clipboard
            && let Some(Backing::Diff(tab)) =
                self.documents.get(&uri).map(|document| &document.backing)
            && let (Some(doc), Ok(files)) = (tab.doc.clone(), self.diff_docs(path, &source))
        {
            doc.set_files(files);
        }
        // A changed picture: its two versions as pictures.
        if !self.documents.contains_key(&uri)
            && let Some(images) = self.image_diff(path, &source, cx)
        {
            let name = path.rsplit('/').next().unwrap_or(path);
            let state = cx.new(|cx| DiffState::new(Vec::new(), cx));
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::Diff(DiffTab {
                        path: path.to_owned(),
                        source: source.clone(),
                        state,
                        title: match &source {
                            DiffSource::Staged => format!("{name} ({})", self.strings().diff_index),
                            DiffSource::Commit(hash, _) => {
                                format!("{name} ({})", &hash[..hash.len().min(8)])
                            }
                            _ => format!("{name} ({})", self.strings().diff_working_tree),
                        },
                        doc: None,
                        images: Some(images),
                    }),
                    language_id: "diff".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
        }
        if !self.documents.contains_key(&uri) && source != DiffSource::Conflict {
            let files = match self.diff_docs(path, &source) {
                Ok(files) => files,
                Err(message) => {
                    self.report(format!("{path}: {message}"));
                    cx.notify();
                    return;
                }
            };
            let headers = source == DiffSource::AllChanges
                || (path.is_empty()
                    && matches!(source, DiffSource::Pull { .. } | DiffSource::Branch { .. }));
            let doc = super::diff_pane::DiffDoc::new(files, self.diff_split, headers);
            let strings = self.strings();
            let name = path.rsplit('/').next().unwrap_or(path);
            let title = match &source {
                DiffSource::Changes => format!("{name} ({})", strings.diff_working_tree),
                DiffSource::Staged => format!("{name} ({})", strings.diff_index),
                DiffSource::Commit(hash, _) => format!("{name} ({})", &hash[..hash.len().min(8)]),
                DiffSource::AllChanges => strings.diff_all_changes.to_owned(),
                DiffSource::Pull { number, .. } if path.is_empty() => format!("PR #{number}"),
                DiffSource::Pull { number, .. } => format!("{name} (PR #{number})"),
                DiffSource::Branch { base, .. } if path.is_empty() => strings.diff_against(base),
                DiffSource::Branch { base, .. } => format!("{name} ({})", strings.diff_against(base)),
                DiffSource::LocalHistory(_) => format!("{name} ({})", strings.history_version),
                DiffSource::Clipboard => format!("{name} ↔ {}", strings.diff_clipboard),
                DiffSource::Files(other) => {
                    format!("{name} ↔ {}", other.rsplit('/').next().unwrap_or(other))
                }
                _ => name.to_owned(),
            };
            let state = cx.new(|cx| DiffState::new(Vec::new(), cx));
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::Diff(DiffTab {
                        path: path.to_owned(),
                        source: source.clone(),
                        state,
                        title,
                        doc: Some(doc),
                        images: None,
                    }),
                    language_id: "diff".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
        }
        if !self.documents.contains_key(&uri) {
            let files = match self.diff_files(path, &source) {
                Ok(files) => files,
                Err(message) => {
                    self.report(format!("{path}: {message}"));
                    cx.notify();
                    return;
                }
            };
            let mode = if self.diff_split {
                DiffMode::Split
            } else {
                DiffMode::Unified
            };
            let state = cx.new(|cx| DiffState::new(files, cx).with_mode(mode));
            // A resolved conflict changes what Save can do.
            cx.observe(&state, |_, _, cx| cx.notify()).detach();
            let strings = self.strings();
            let name = path.rsplit('/').next().unwrap_or(path);
            let title = match &source {
                DiffSource::Changes => format!("{name} ({})", strings.diff_working_tree),
                DiffSource::Staged => format!("{name} ({})", strings.diff_index),
                DiffSource::Conflict => format!("{name} ({})", strings.diff_conflict),
                DiffSource::Commit(hash, _) => format!("{name} ({})", &hash[..hash.len().min(8)]),
                DiffSource::Proposed(tab_name) => tab_name.clone(),
                DiffSource::Agent(_) => format!("{name} (Claude)"),
                DiffSource::AllChanges
                | DiffSource::LocalHistory(_)
                | DiffSource::Clipboard
                | DiffSource::Pull { .. }
                | DiffSource::Branch { .. }
                | DiffSource::Files(_) => strings.diff_all_changes.to_owned(),
            };
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::Diff(DiffTab {
                        path: path.to_owned(),
                        source,
                        state,
                        title,
                        doc: None,
                        images: None,
                    }),
                    language_id: "diff".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
        }
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, OpenAs::Preview, true, window, cx);
    }

    /// The two versions of a file, or of every changed file, for Keel's
    /// own view.
    pub(super) fn diff_docs(
        &self,
        path: &str,
        source: &DiffSource,
    ) -> Result<Vec<super::diff_model::FileDiff>, String> {
        let ignore = self.diff_ignore_spaces;
        // Not git's: a kept version, or the clipboard, against the file now.
        let shown = |path: &str| {
            let full = Path::new(path);
            self.project
                .root
                .as_ref()
                .and_then(|root| full.strip_prefix(root).ok())
                .unwrap_or(full)
                .to_string_lossy()
                .into_owned()
        };
        let now = |path: &str| {
            let uri = Uri::file(Path::new(path)).to_string();
            self.documents
                .get(&uri)
                .map(|document| document.synced_text.clone())
                .or_else(|| std::fs::read_to_string(path).ok())
                .or_else(|| {
                    // An untitled file, by its name.
                    self.documents
                        .values()
                        .find(|document| self.display_name(document) == path)
                        .map(|document| document.synced_text.clone())
                })
                .unwrap_or_default()
        };
        match source {
            DiffSource::LocalHistory(version) => {
                let old = std::fs::read_to_string(version).map_err(|error| error.to_string())?;
                return Ok(vec![super::diff_model::FileDiff::new(
                    shown(path),
                    &old,
                    &now(path),
                    ignore,
                )]);
            }
            DiffSource::Clipboard => {
                return Ok(vec![super::diff_model::FileDiff::new(
                    shown(path),
                    &self.clipboard_text,
                    &now(path),
                    ignore,
                )]);
            }
            DiffSource::Files(other) => {
                return Ok(vec![super::diff_model::FileDiff::new(
                    format!("{} ↔ {}", shown(path), shown(other)),
                    &now(path),
                    &now(other),
                    ignore,
                )]);
            }
            _ => {}
        }
        let repo = self.scm.repo.as_ref().ok_or_else(String::new)?;
        let disk = |path: &str| std::fs::read_to_string(repo.absolute(path)).unwrap_or_default();
        let one = |old: String, new: String| {
            vec![super::diff_model::FileDiff::new(path, &old, &new, ignore)]
        };
        Ok(match source {
            DiffSource::Changes => one(repo.index_text(path).unwrap_or_default(), disk(path)),
            DiffSource::Staged => one(
                repo.head_text(path).unwrap_or_default(),
                repo.index_text(path).unwrap_or_default(),
            ),
            DiffSource::Commit(hash, original) => one(
                repo.text_at(&format!("{hash}^"), original.as_deref().unwrap_or(path))
                    .unwrap_or_default(),
                repo.text_at(hash, path)
                    // A stash keeps its new files in its third parent.
                    .or_else(|| {
                        hash.starts_with("stash@")
                            .then(|| repo.text_at(&format!("{hash}^3"), path))
                            .flatten()
                    })
                    .unwrap_or_default(),
            ),
            DiffSource::Pull { number, from, to } => repo
                .files_between(from, to)
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter(|file| path.is_empty() || file.path == path)
                .map(|file| {
                    let before = file.original.as_deref().unwrap_or(&file.path);
                    let mut diff = super::diff_model::FileDiff::new(
                        &file.path,
                        &repo.text_at(from, before).unwrap_or_default(),
                        &repo.text_at(to, &file.path).unwrap_or_default(),
                        ignore,
                    );
                    diff.notes = self.pulls.notes_of(*number, &file.path);
                    diff
                })
                .collect(),
            DiffSource::Branch { from, .. } => repo
                .files_between(from, "HEAD")
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter(|file| path.is_empty() || file.path == path)
                .map(|file| {
                    let before = file.original.as_deref().unwrap_or(&file.path);
                    super::diff_model::FileDiff::new(
                        &file.path,
                        &repo.text_at(from, before).unwrap_or_default(),
                        &repo.text_at("HEAD", &file.path).unwrap_or_default(),
                        ignore,
                    )
                })
                .collect(),
            DiffSource::AllChanges => self
                .scm
                .status
                .changed()
                .map(|file| {
                    super::diff_model::FileDiff::new(
                        &file.path,
                        &repo.index_text(&file.path).unwrap_or_default(),
                        &disk(&file.path),
                        ignore,
                    )
                })
                .collect(),
            _ => return Err("not a git difference".to_owned()),
        })
    }

    /// A differences tab drawn by Keel: the file and its counts, the way
    /// through its changes, how to show them, and the view.
    fn render_doc_tab(
        &self,
        uri: &str,
        tab: &DiffTab,
        doc: &super::diff_pane::DiffDoc,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::diff_pane::BlockActions;
        let strings = self.strings();
        let source = tab.source.clone();
        let (added, removed) = doc.counts();
        let actions = match source {
            DiffSource::Changes | DiffSource::AllChanges => BlockActions::StageOrRevert,
            DiffSource::Staged => BlockActions::Unstage,
            _ => BlockActions::None,
        };
        let all = source == DiffSource::AllChanges
            || (tab.path.is_empty()
                && matches!(source, DiffSource::Pull { .. } | DiffSource::Branch { .. }));
        let keys = |action: &dyn gpui::Action| {
            window
                .highest_precedence_binding_for_action(action)
                .map(|binding| {
                    binding
                        .keystrokes()
                        .iter()
                        .map(|key| key.unparse(gpui::OperatingSystem::Mac))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
        };
        let with_keys = |text: &str, keys: Option<String>| match keys {
            Some(keys) => format!("{text} ({keys})"),
            None => text.to_owned(),
        };
        let previous_tip = with_keys(strings.diff_previous_change, keys(&PreviousDiffChange));
        let next_tip = with_keys(strings.diff_next_change, keys(&NextDiffChange));
        let theme = cx.theme();
        let title: AnyElement = if all {
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .child(Icon::new(IconName::Layers).small().text_color(colors.muted))
                .child(div().text_sm().text_color(colors.text).child(notices::fill(
                    strings.diff_files_count,
                    &[("count", &doc.files_len().to_string())],
                )))
                .into_any_element()
        } else {
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap_1p5()
                .child(self.file_icon_element(Path::new(&tab.path), cx))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(tab.path.clone()),
                )
                .into_any_element()
        };
        let mut toolbar = div()
            .h(px(36.))
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
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(title)
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .gap_1p5()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .child(div().text_color(theme.success).child(format!("+{added}")))
                            .child(div().text_color(theme.danger).child(format!("−{removed}"))),
                    ),
            )
            .child(
                Button::new("diff-previous")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::ArrowUp))
                    .tooltip(previous_tip)
                    .on_click(cx.listener(|this, _, _, cx| this.go_to_diff_change(false, cx))),
            )
            .child(
                Button::new("diff-next")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::ArrowDown))
                    .tooltip(next_tip)
                    .on_click(cx.listener(|this, _, _, cx| this.go_to_diff_change(true, cx))),
            )
            .child(div().w(px(1.)).h(px(16.)).mx_1().bg(colors.border))
            .child(
                Button::new("diff-spaces")
                    .ghost()
                    .small()
                    .label(strings.diff_ignore_spaces)
                    .selected(self.diff_ignore_spaces)
                    .tooltip(strings.diff_ignore_spaces_tip)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_diff_spaces(cx))),
            )
            .child({
                // Too narrow for two columns: one, and the button says why.
                let fits = self.diff_fits_two_columns(window);
                Button::new("diff-split")
                    .ghost()
                    .small()
                    .label(strings.diff_side_by_side)
                    .selected(self.diff_split && fits)
                    .disabled(!fits)
                    .tooltip(if fits {
                        strings.diff_side_by_side
                    } else {
                        strings.diff_too_narrow
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.set_diff_split(true, cx)))
            })
            .child(
                Button::new("diff-unified")
                    .ghost()
                    .small()
                    .label(strings.diff_inline)
                    .selected(!self.diff_split || !self.diff_fits_two_columns(window))
                    .on_click(cx.listener(|this, _, _, cx| this.set_diff_split(false, cx))),
            );
        let path = tab.path.clone();
        toolbar = match source {
            DiffSource::Changes => toolbar.child(
                Button::new("diff-stage")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .tooltip(strings.scm_stage)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.stage_paths(vec![path.clone()], window, cx)
                    })),
            ),
            DiffSource::Staged => toolbar.child(
                Button::new("diff-unstage")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Minus))
                    .tooltip(strings.scm_unstage)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.unstage_paths(vec![path.clone()], window, cx)
                    })),
            ),
            DiffSource::LocalHistory(version) => {
                let (file, version) = (PathBuf::from(&path), PathBuf::from(version));
                toolbar.child(
                    Button::new("diff-restore-version")
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::RotateCcw))
                        .label(strings.history_restore)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.restore_version(&file, &version, window, cx)
                        })),
                )
            }
            DiffSource::AllChanges => toolbar.child(
                Button::new("diff-stage-all")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .tooltip(strings.diff_stage_all)
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.dispatch_action(Box::new(super::StageAll), cx);
                        let _ = this;
                    })),
            ),
            _ => toolbar,
        };
        if !all {
            let open_uri = uri.to_owned();
            toolbar = toolbar.child(
                Button::new("diff-open-file")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::File))
                    .tooltip(strings.diff_open_file_here)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_diff_in_view(&open_uri, window, cx)
                    })),
            );
        }
        let body = if doc.files_len() == 0 {
            note(strings.scm_clean.to_owned(), colors).into_any_element()
        } else {
            self.render_diff_doc(uri, doc, actions, colors, window, cx)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    /// The differences tab in front, if it is Keel's.
    fn active_diff_doc(&self) -> Option<(String, super::diff_pane::DiffDoc)> {
        let uri = self.panes.get(self.active)?.uri.clone();
        match &self.documents.get(&uri)?.backing {
            Backing::Diff(tab) => Some((uri, tab.doc.clone()?)),
            _ => None,
        }
    }

    pub(super) fn go_to_diff_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some((_, doc)) = self.active_diff_doc() {
            let _ = doc.go_to_change(forward);
            cx.notify();
        }
    }

    fn toggle_diff_spaces(&mut self, cx: &mut Context<Self>) {
        self.diff_ignore_spaces = !self.diff_ignore_spaces;
        let docs: Vec<(String, DiffSource, super::diff_pane::DiffDoc)> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::Diff(tab) => {
                    Some((tab.path.clone(), tab.source.clone(), tab.doc.clone()?))
                }
                _ => None,
            })
            .collect();
        for (path, source, doc) in docs {
            if let Ok(files) = self.diff_docs(&path, &source) {
                doc.set_files(files);
            }
        }
        cx.notify();
    }

    /// Opens a file of the repository at a line, counted from zero.
    pub(super) fn open_repo_file(
        &mut self,
        path: &str,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let file = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            let Some(repo) = self.scm.repo.clone() else {
                return;
            };
            self.project_spelling(&repo.absolute(path))
        };
        if !file.is_file() {
            return;
        }
        self.open_file(&file, window, cx);
        self.go_to(line as u32 + 1, 1, window, cx);
    }

    /// A double click on a line of the new side: the file, at that line.
    pub(super) fn open_diff_line(
        &mut self,
        uri: &str,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Backing::Diff(tab)) = self.documents.get(uri).map(|document| &document.backing)
        else {
            return;
        };
        let path = tab.path.clone();
        self.open_repo_file(&path, line, window, cx);
    }

    /// Open the File: at the line at the top of the differences.
    fn open_diff_in_view(&mut self, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Backing::Diff(tab)) = self.documents.get(uri).map(|document| &document.backing)
        else {
            return;
        };
        let (path, line) = tab
            .doc
            .as_ref()
            .and_then(|doc| doc.line_in_view())
            .unwrap_or_else(|| (tab.path.clone(), 0));
        self.open_repo_file(&path, line, window, cx);
    }

    pub(super) fn diff_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &NextDiffChange, _, cx| {
            if this.active_diff_doc().is_some() {
                this.go_to_diff_change(true, cx)
            } else {
                cx.propagate()
            }
        }))
        .on_action(cx.listener(|this, _: &PreviousDiffChange, _, cx| {
            if this.active_diff_doc().is_some() {
                this.go_to_diff_change(false, cx)
            } else {
                cx.propagate()
            }
        }))
        .on_action(cx.listener(|this, _: &OpenAllChanges, window, cx| {
            this.open_diff("", DiffSource::AllChanges, window, cx)
        }))
    }

    /// Reads the open differences again after git's status changed.
    pub(super) fn refresh_open_diffs(&mut self, cx: &mut Context<Self>) {
        let docs: Vec<(String, DiffSource, super::diff_pane::DiffDoc)> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::Diff(tab)
                    if matches!(
                        tab.source,
                        DiffSource::Changes | DiffSource::Staged | DiffSource::AllChanges
                    ) =>
                {
                    Some((tab.path.clone(), tab.source.clone(), tab.doc.clone()?))
                }
                _ => None,
            })
            .collect();
        for (path, source, doc) in docs {
            if let Ok(files) = self.diff_docs(&path, &source) {
                doc.set_files(files);
            }
        }
        cx.notify();
        let tabs: Vec<(String, DiffSource, Entity<DiffState>)> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                // Conflicts keep the choices made so far until saved.
                // Conflicts keep their choices; a commit never changes.
                Backing::Diff(tab)
                    if tab.doc.is_none()
                        && !matches!(
                            tab.source,
                            DiffSource::Conflict
                                | DiffSource::Commit(..)
                                | DiffSource::Proposed(_)
                                | DiffSource::Agent(_)
                                | DiffSource::Pull { .. }
                                | DiffSource::Branch { .. }
                                | DiffSource::AllChanges
                        ) =>
                {
                    Some((tab.path.clone(), tab.source.clone(), tab.state.clone()))
                }
                _ => None,
            })
            .collect();
        for (path, source, state) in tabs {
            if let Ok(files) = self.diff_files(&path, &source) {
                state.update(cx, |state, cx| state.set_files(files, cx));
            }
        }
    }

    pub(super) fn set_diff_split(&mut self, split: bool, cx: &mut Context<Self>) {
        self.diff_split = split;
        let mode = if split {
            DiffMode::Split
        } else {
            DiffMode::Unified
        };
        let states: Vec<Entity<DiffState>> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::Diff(tab) => Some(tab.state.clone()),
                _ => None,
            })
            .collect();
        for state in states {
            state.update(cx, |state, cx| state.set_mode(mode, cx));
        }
        cx.notify();
    }

    /// Writes a resolved conflict to the file and stages it.
    fn save_resolution(
        &mut self,
        path: String,
        state: Entity<DiffState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let Some(text) = state.read(cx).resolved_text(&path) else {
            return;
        };
        let file = repo.absolute(&path);
        if let Err(error) = std::fs::write(&file, text) {
            self.report(format!("{path}: {error}"));
            cx.notify();
            return;
        }
        let uri = diff_uri(&path, &DiffSource::Conflict);
        let label = self.strings().scm_stage;
        self.run_git(
            label,
            move |repo| repo.stage(&[path.as_str()]),
            move |this, window, cx| {
                // The conflict is gone: its tab gives way to the file.
                if let Some((group, tab)) = this.find_tab(&uri, this.active) {
                    this.close_tab_now(group, tab, window, cx);
                }
                this.open_file(&file, window, cx);
            },
            window,
            cx,
        );
    }

    pub(super) fn render_diff(
        &self,
        uri: &str,
        tab: &DiffTab,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let DiffSource::Proposed(name) = &tab.source {
            return self.render_claude_diff(name, tab, colors, window, cx);
        }
        if let DiffSource::Agent(path) = &tab.source {
            return self.render_agent_change_tab(path, tab, colors, window, cx);
        }
        if let Some(images) = &tab.images {
            return self.render_image_diff(uri, tab, images, colors, cx);
        }
        if let Some(doc) = &tab.doc {
            return self.render_doc_tab(uri, tab, doc, colors, window, cx);
        }
        let strings = self.strings();
        let path = tab.path.clone();
        let source = tab.source.clone();
        let empty = tab.state.read(cx).files().is_empty();
        let mut toolbar = div()
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
                    .child(tab.path.clone()),
            );
        if source == DiffSource::Conflict {
            let resolved = tab.state.read(cx).resolved_text(&tab.path).is_some();
            let state = tab.state.clone();
            let save_path = path.clone();
            toolbar = toolbar.child(
                Button::new("diff-save-resolution")
                    .primary()
                    .small()
                    .icon(Icon::new(IconName::Check))
                    .label(strings.diff_save_resolution)
                    .disabled(!resolved)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.save_resolution(save_path.clone(), state.clone(), window, cx)
                    })),
            );
        } else {
            toolbar = toolbar
                .child(
                    Button::new("diff-split")
                        .ghost()
                        .small()
                        .label(strings.diff_side_by_side)
                        .selected(self.diff_split)
                        .on_click(cx.listener(|this, _, _, cx| this.set_diff_split(true, cx))),
                )
                .child(
                    Button::new("diff-unified")
                        .ghost()
                        .small()
                        .label(strings.diff_inline)
                        .selected(!self.diff_split)
                        .on_click(cx.listener(|this, _, _, cx| this.set_diff_split(false, cx))),
                );
            let stage_path = path.clone();
            // A commit's differences have nothing to stage.
            let written = matches!(source, DiffSource::Commit(..) | DiffSource::Pull { .. });
            toolbar = toolbar.when(!written, |toolbar| {
                toolbar.child(match source {
                    DiffSource::Staged => Button::new("diff-unstage")
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::Minus))
                        .tooltip(strings.scm_unstage)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.unstage_paths(vec![stage_path.clone()], window, cx)
                        })),
                    _ => Button::new("diff-stage")
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::Plus))
                        .tooltip(strings.scm_stage)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.stage_paths(vec![stage_path.clone()], window, cx)
                        })),
                })
            });
        }
        let open_path = path.clone();
        toolbar = toolbar.child(
            Button::new("diff-open-file")
                .ghost()
                .small()
                .icon(Icon::new(IconName::File))
                .tooltip(strings.scm_open_file)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(repo) = this.scm.repo.clone() {
                        let file = repo.absolute(&open_path);
                        if file.is_file() {
                            this.open_file(&file, window, cx);
                        }
                    }
                })),
        );
        let body: AnyElement = if empty {
            note(strings.scm_clean.to_owned(), colors).into_any_element()
        } else {
            Diff::new(&tab.state)
                .line_number(true)
                .syntax_highlight(true)
                .header_visible(false)
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use gpui::TestAppContext;

    use super::*;

    fn git(folder: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// "What changes against the base branch": the branch's files since it
    /// parted from main, in one page, and nothing on main itself.
    #[gpui::test]
    fn a_branch_shows_what_it_changes_against_main(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "uno\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        let settings = Settings::load(root.join(".git/keel-test-settings.json"), None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        // On main: nothing to compare.
        view.update_in(cx, |view, window, cx| {
            view.compare_with_base(window, cx);
            assert!(!view.documents.keys().any(|uri| uri.starts_with("keel-branch:")));
        });
        git(&root, &["switch", "-q", "-c", "prezzi"]);
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        fs::write(root.join("b.txt"), "nuovo\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "prezzi"]);
        view.update_in(cx, |view, window, cx| {
            view.compare_with_base(window, cx);
            let document = &view.documents["keel-branch:main/"];
            let Backing::Diff(tab) = &document.backing else {
                panic!("not a differences tab");
            };
            assert_eq!(tab.title, view.strings().diff_against("main"));
            let doc = tab.doc.as_ref().unwrap();
            assert_eq!(doc.counts(), (2, 0));
        });
    }

    #[gpui::test]
    fn changes_open_in_a_tab_and_conflicts_are_resolved(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        // A conflict: two branches change the same line.
        git(&root, &["switch", "-q", "-c", "other"]);
        fs::write(root.join("a.txt"), "uno\nDUE\n").unwrap();
        git(&root, &["commit", "-q", "-am", "other"]);
        git(&root, &["switch", "-q", "main"]);
        fs::write(root.join("a.txt"), "uno\nDue\n").unwrap();
        git(&root, &["commit", "-q", "-am", "main"]);
        git(&root, &["merge", "-q", "other"]);
        fs::write(root.join("b.txt"), "nuovo\n").unwrap();

        let settings = Settings::load(root.join(".git/keel-test-settings.json"), None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update(cx, |view, _| {
            view.scm.status = view.scm.repo.as_ref().unwrap().status().unwrap();
            assert_eq!(view.scm.status.conflicts().count(), 1);
        });

        // A new file's changes open in a tab named after it.
        view.update_in(cx, |view, window, cx| {
            view.open_change(
                "b.txt",
                super::super::source_control::Section::Changes,
                window,
                cx,
            )
        });
        view.read_with(cx, |view, cx| {
            let document = &view.documents[&diff_uri("b.txt", &DiffSource::Changes)];
            let Backing::Diff(tab) = &document.backing else {
                panic!("not a diff");
            };
            assert!(tab.title.starts_with("b.txt ("));
            assert_eq!(tab.doc.as_ref().unwrap().counts(), (1, 0));
            assert_eq!(view.panes.len(), 1);
            let _ = cx;
        });

        // The conflict, resolved by keeping the incoming change.
        view.update_in(cx, |view, window, cx| {
            view.open_change(
                "a.txt",
                super::super::source_control::Section::Conflicts,
                window,
                cx,
            )
        });
        let state = view.read_with(cx, |view, _| {
            let Backing::Diff(tab) =
                &view.documents[&diff_uri("a.txt", &DiffSource::Conflict)].backing
            else {
                panic!("not a diff");
            };
            tab.state.clone()
        });
        state.update(cx, |state, cx| {
            assert_eq!(state.files()[0].conflicts().len(), 1);
            state.resolve_conflict(
                "a.txt",
                0,
                Some(gpui_component::diff::DiffConflictResolution::Incoming),
                cx,
            );
        });
        view.update_in(cx, |view, window, cx| {
            view.save_resolution("a.txt".to_owned(), state.clone(), window, cx)
        });
        for _ in 0..50 {
            cx.run_until_parked();
            if view.read_with(cx, |view, _| view.scm.busy.is_none()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\nDUE\n"
        );
        let status = git(&root, &["status", "--porcelain"]);
        assert!(status.contains("M  a.txt"), "{status}");
    }

    /// A block of a file's changes is staged on its own, another one is
    /// undone in the file; F7 goes from block to block, and the page with
    /// every change lists the changed files.
    #[gpui::test]
    fn the_right_column_is_edited_where_it_is_read(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        let lines: String = (1..=12).map(|line| format!("riga {line}\n")).collect();
        fs::write(root.join("a.txt"), &lines).unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        fs::write(
            root.join("a.txt"),
            lines.replace("riga 3\n", "riga TRE\nriga 3 e mezzo\n"),
        )
        .unwrap();

        let settings = Settings::load(root.join(".git/keel-test-settings.json"), None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update(cx, |view, _| {
            view.scm.status = view.scm.repo.as_ref().unwrap().status().unwrap();
        });
        view.update_in(cx, |view, window, cx| {
            view.open_diff("a.txt", DiffSource::Changes, window, cx)
        });
        let uri = diff_uri("a.txt", &DiffSource::Changes);
        assert!(view.read_with(cx, |view, _| view.diff_editable(&uri)));

        // A double click on the changed line: its block, both new lines.
        view.update_in(cx, |view, window, cx| {
            view.diff_line_double_click(&uri, 0, 3, window, cx)
        });
        view.read_with(cx, |view, cx| {
            let edit = view.diff_edit.as_ref().unwrap();
            assert_eq!(edit.lines, 2..4);
            assert_eq!(
                edit.editor.read(cx).value().to_string(),
                "riga TRE\nriga 3 e mezzo"
            );
        });
        // Esc leaves the file as it was.
        cx.simulate_keystrokes("escape");
        assert!(view.read_with(cx, |view, _| view.diff_edit.is_none()));

        // Edited and applied with Cmd+Enter: written and saved.
        view.update_in(cx, |view, window, cx| {
            view.diff_line_double_click(&uri, 0, 2, window, cx);
            let editor = view.diff_edit.as_ref().unwrap().editor.clone();
            editor.update(cx, |editor, cx| {
                editor.set_value("riga tre, scritta qui", window, cx)
            });
        });
        cx.simulate_keystrokes("cmd-enter");
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.diff_edit.is_none()));
        let file = fs::read_to_string(root.join("a.txt")).unwrap();
        assert_eq!(file, lines.replace("riga 3\n", "riga tre, scritta qui\n"));

        // The file changes while its block is open: it is not written
        // over, and what was typed stays in the card.
        view.update_in(cx, |view, window, cx| {
            view.diff_line_double_click(&uri, 0, 2, window, cx);
            let editor = view.diff_edit.as_ref().unwrap().editor.clone();
            editor.update(cx, |editor, cx| editor.set_value("la mia", window, cx));
        });
        let outside = lines.replace("riga 3\n", "di un altro\n");
        fs::write(root.join("a.txt"), &outside).unwrap();
        cx.simulate_keystrokes("cmd-enter");
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), outside);
        view.read_with(cx, |view, cx| {
            let edit = view.diff_edit.as_ref().unwrap();
            assert_eq!(edit.editor.read(cx).value().to_string(), "la mia");
        });
        cx.simulate_keystrokes("escape");

        // An unchanged line alone; in the staged changes, no editing.
        view.update_in(cx, |view, window, cx| {
            view.diff_line_double_click(&uri, 0, 8, window, cx)
        });
        assert_eq!(
            view.read_with(cx, |view, _| view
                .diff_edit
                .as_ref()
                .map(|edit| edit.lines.clone())),
            Some(8..9)
        );
        let staged = diff_uri("a.txt", &DiffSource::Staged);
        assert!(!view.read_with(cx, |view, _| view.diff_editable(&staged)));
    }

    #[gpui::test]
    fn blocks_are_staged_and_undone_one_at_a_time(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        let lines: String = (1..=30).map(|line| format!("riga {line}\n")).collect();
        fs::write(root.join("a.txt"), &lines).unwrap();
        fs::write(root.join("c.txt"), "c\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        let changed = lines
            .replace("riga 2\n", "riga DUE\n")
            .replace("riga 25\n", "riga VENTICINQUE\n");
        fs::write(root.join("a.txt"), &changed).unwrap();
        fs::write(root.join("c.txt"), "C\n").unwrap();

        let settings = Settings::load(root.join(".git/keel-test-settings.json"), None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update(cx, |view, _| {
            view.scm.status = view.scm.repo.as_ref().unwrap().status().unwrap();
        });
        view.update_in(cx, |view, window, cx| {
            view.open_diff("a.txt", DiffSource::Changes, window, cx)
        });
        let uri = diff_uri("a.txt", &DiffSource::Changes);
        let doc = view.read_with(cx, |view, _| {
            let Backing::Diff(tab) = &view.documents[&uri].backing else {
                panic!("not a diff");
            };
            tab.doc.clone().unwrap()
        });
        assert_eq!(doc.counts(), (2, 2));
        let starts: Vec<usize> = doc
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.starts_hunk())
            .map(|(index, _)| index)
            .collect();
        assert_eq!(starts.len(), 2);

        // F7 goes past the block in view to the next; Shift+F7 back.
        assert!(view.read_with(cx, |view, _| view.active_diff_doc().is_some()));
        assert_eq!(doc.go_to_change(true), Some(starts[1]));
        doc.list.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        assert_eq!(doc.go_to_change(false), Some(starts[0]));

        // Stage the first block: only it goes to the index.
        view.update_in(cx, |view, window, cx| {
            view.diff_block_action(&uri, 0, 0, false, window, cx)
        });
        for _ in 0..50 {
            cx.run_until_parked();
            if view.read_with(cx, |view, _| view.scm.busy.is_none()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let staged = git(&root, &["show", ":a.txt"]);
        assert!(
            staged.contains("riga DUE") && staged.contains("riga 25\n"),
            "{staged}"
        );

        // The view follows: one block left, the other one, now first.
        assert_eq!(doc.counts(), (1, 1));
        // Undo it in the file: it goes back as staged.
        view.update_in(cx, |view, window, cx| {
            view.diff_block_action(&uri, 0, 0, true, window, cx)
        });
        cx.run_until_parked();
        let file = fs::read_to_string(root.join("a.txt")).unwrap();
        assert!(
            file.contains("riga DUE") && file.contains("riga 25\n"),
            "{file}"
        );

        // Every change in one page: both files.
        view.update(cx, |view, _| {
            view.scm.status = view.scm.repo.as_ref().unwrap().status().unwrap();
        });
        view.update_in(cx, |view, window, cx| {
            view.open_diff("", DiffSource::AllChanges, window, cx)
        });
        view.read_with(cx, |view, _| {
            let Backing::Diff(tab) =
                &view.documents[&diff_uri("", &DiffSource::AllChanges)].backing
            else {
                panic!("not a diff");
            };
            let doc = tab.doc.as_ref().unwrap();
            assert_eq!(
                doc.files_len(),
                1,
                "a.txt matches its stage now, c.txt does not"
            );
            assert!(matches!(
                doc.rows()[0],
                super::super::diff_model::Row::Header { .. }
            ));
        });
    }
}
