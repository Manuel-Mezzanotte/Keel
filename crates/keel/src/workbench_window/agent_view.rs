//! How Claude's panel looks. Before the first message: Claude's mark, a
//! few things to ask, the recent chats, or the ways to sign in. In a chat:
//! your messages in bubbles, Claude's answers in full width, its reasoning
//! folded, its tools as quiet rows (an edit with its difference), the plan,
//! the permissions it asks. Under it all the composer: what goes along, the
//! text, `@` for files and folders, `/` for commands and skills, the mode,
//! the model with its reasoning, and the button that sends or stops.

use std::cell::{Cell, RefCell};

use ai::session_meta::matching_commands;
use ai::thread::{Entry, ToolCall, ToolContent, ToolStatus};
use gpui::{Animation, AnimationExt as _, AnyElement, StatefulInteractiveElement as _, svg};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, Textarea};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::text::TextView;
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::agent_panel::{AgentStatus, Attachment, Permission};
use super::shell::{Colors, chevron_turn};
use super::*;

/// How many lines of a file difference a tool shows before "…".
const DIFF_LINES: usize = 14;
/// How many rows a `/` or `@` menu offers.
const MENU_ROWS: usize = 40;

/// Claude's colour, for its mark.
pub(super) fn claude_colour() -> Hsla {
    gpui::rgb(0xd97757).into()
}

/// Claude's mark, `size` points wide.
pub(super) fn claude_mark(size: f32) -> gpui::Svg {
    svg()
        .path(crate::assets::CLAUDE_MARK)
        .size(px(size))
        .flex_none()
        .text_color(claude_colour())
}

/// What a menu of the composer offers.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum ComposerItem {
    Command {
        name: String,
        description: String,
        hint: Option<String>,
    },
    /// A file or folder of the project, by its path in it.
    Path { relative: String, folder: bool },
    /// The open file's selection.
    Selection,
}

/// Which menu a word being typed opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuKind {
    /// `/` at the start of the message.
    Slash,
    /// `@` anywhere.
    Mention,
}

/// The word being typed at `cursor`, when it opens a menu: its kind, where
/// it starts, and what follows the `/` or `@`.
pub(super) fn trigger_at(text: &str, cursor: usize) -> Option<(MenuKind, usize, String)> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];
    let start = before
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_whitespace())
        .map_or(0, |(at, character)| at + character.len_utf8());
    let word = &before[start..];
    if let Some(query) = word.strip_prefix('/')
        && before[..start].trim().is_empty()
    {
        return Some((MenuKind::Slash, start, query.to_owned()));
    }
    let query = word.strip_prefix('@')?;
    Some((MenuKind::Mention, start, query.to_owned()))
}

/// The composer's menu state, kept between frames.
#[derive(Default)]
pub(super) struct ComposerMenus {
    pub(super) selected: Cell<usize>,
    /// The word whose menu Esc closed, by where it starts and its kind.
    pub(super) dismissed: Cell<Option<(usize, MenuKind)>>,
    /// The files and folders found for the last `@` query.
    found: RefCell<Option<(String, Vec<ComposerItem>)>>,
}

impl ComposerMenus {
    /// The text changed: the first row is chosen again, and the files are
    /// found again.
    pub(super) fn text_changed(&self) {
        self.selected.set(0);
        self.found.borrow_mut().take();
    }
}

impl Workbench {
    /// The menu the composer shows now, if any: its kind, where the word
    /// starts, and what it offers.
    pub(super) fn composer_menu(&self, cx: &App) -> Option<(MenuKind, usize, Vec<ComposerItem>)> {
        let input = self.agent_panel.input.read(cx);
        let text = input.value().to_string();
        let Some((kind, start, query)) = trigger_at(&text, input.cursor()) else {
            // Past the word, Esc's choice is forgotten.
            self.agent_panel.menus.dismissed.set(None);
            return None;
        };
        if self.agent_panel.menus.dismissed.get() == Some((start, kind)) {
            return None;
        }
        let items = match kind {
            MenuKind::Slash => matching_commands(&self.agent_panel.commands, &query)
                .into_iter()
                .take(MENU_ROWS)
                .map(|command| ComposerItem::Command {
                    name: command.name.clone(),
                    description: command.description.clone(),
                    hint: command.hint.clone(),
                })
                .collect(),
            MenuKind::Mention => self.mention_items(&query, cx),
        };
        Some((kind, start, items))
    }

    /// What `@query` may point to: the selection, then the open files when
    /// nothing is typed, otherwise the project's files and folders by
    /// fuzzy match.
    fn mention_items(&self, query: &str, cx: &App) -> Vec<ComposerItem> {
        let menus = &self.agent_panel.menus;
        if let Some((known, items)) = menus.found.borrow().as_ref()
            && known == query
        {
            return items.clone();
        }
        let mut items = Vec::new();
        let selected = self
            .panes
            .get(self.active)
            .is_some_and(|pane| !pane.editor.read(cx).selected_range().is_empty());
        let selection_word = self.strings().agent_mention_selection.to_lowercase();
        if selected && (query.is_empty() || selection_word.contains(&query.to_lowercase())) {
            items.push(ComposerItem::Selection);
        }
        let root = self.project.root.clone();
        let relative = |path: &Path| -> Option<String> {
            let root = root.as_ref()?;
            path.strip_prefix(root)
                .ok()
                .map(|relative| relative.to_string_lossy().into_owned())
        };
        if query.is_empty() {
            for pane in &self.panes {
                if let Some(path) = Uri::parse(&pane.uri).and_then(|uri| uri.file_path())
                    && let Some(relative) = relative(&path)
                {
                    let item = ComposerItem::Path {
                        relative,
                        folder: false,
                    };
                    if !items.contains(&item) {
                        items.push(item);
                    }
                }
            }
        } else if let Some(index) = &self.file_index {
            let mut scored: Vec<(i32, &str)> = index
                .iter()
                .filter_map(|path| Some((files::fuzzy::score_path(query, path)?, path.as_str())))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
            // Folders: the parents of the best files that match by name.
            let mut folders: Vec<(i32, String)> = Vec::new();
            for (_, path) in scored.iter().take(300) {
                let mut parent = Path::new(path).parent();
                while let Some(folder) = parent.filter(|folder| !folder.as_os_str().is_empty()) {
                    let folder_text = folder.to_string_lossy().into_owned();
                    if !folders.iter().any(|(_, known)| *known == folder_text)
                        && let Some(score) = files::fuzzy::score_path(query, &folder_text)
                    {
                        folders.push((score, folder_text));
                    }
                    parent = folder.parent();
                }
            }
            folders.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
            let best_file = scored.first().map_or(i32::MIN, |(score, _)| *score);
            for (score, folder) in folders.iter().take(4) {
                if *score >= best_file {
                    items.push(ComposerItem::Path {
                        relative: folder.clone(),
                        folder: true,
                    });
                }
            }
            for (_, path) in scored.iter().take(MENU_ROWS) {
                items.push(ComposerItem::Path {
                    relative: (*path).to_owned(),
                    folder: false,
                });
            }
            for (score, folder) in folders.into_iter().take(6) {
                let item = ComposerItem::Path {
                    relative: folder,
                    folder: true,
                };
                if score < best_file && !items.contains(&item) {
                    items.push(item);
                }
            }
        }
        items.truncate(MENU_ROWS);
        *menus.found.borrow_mut() = Some((query.to_owned(), items.clone()));
        items
    }

    /// Uses what the menu offers at `index`: a command goes into the text,
    /// a file, folder or the selection along with the message.
    pub(super) fn choose_composer_item(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, start, items)) = self.composer_menu(cx) else {
            return;
        };
        let Some(item) = items.get(index).cloned() else {
            return;
        };
        let input = self.agent_panel.input.clone();
        let (text, cursor) = {
            let input = input.read(cx);
            (input.value().to_string(), input.cursor())
        };
        let cursor = cursor.min(text.len()).max(start);
        let (inserted, attach) = match &item {
            ComposerItem::Command { name, .. } => (format!("/{name} "), None),
            ComposerItem::Path { relative, .. } => (
                String::new(),
                self.project.root.as_ref().map(|root| root.join(relative)),
            ),
            ComposerItem::Selection => (String::new(), None),
        };
        let new_text = format!("{}{inserted}{}", &text[..start], &text[cursor..]);
        let at = start + inserted.len();
        input.update(cx, |input, cx| {
            input.set_value(new_text, window, cx);
            input.set_selected_range(at..at, cx);
        });
        self.agent_panel.menus.selected.set(0);
        match item {
            ComposerItem::Selection => self.add_to_agent(window, cx),
            ComposerItem::Path { .. } => {
                if let Some(path) = attach {
                    let attachment = Attachment {
                        path,
                        selection: None,
                    };
                    if !self.agent_panel.attachments.contains(&attachment) {
                        self.agent_panel.attachments.push(attachment);
                    }
                }
            }
            ComposerItem::Command { .. } => {}
        }
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// The keys of an open menu, before the message box has them: arrows
    /// to choose, Return or Tab to use, Esc to close it. Whether the key
    /// was used.
    pub(super) fn composer_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if key == "escape" && self.agent_panel.popover.is_some() && self.layout.agent {
            self.agent_panel.popover = None;
            cx.notify();
            return true;
        }
        if !matches!(key, "up" | "down" | "enter" | "tab" | "escape")
            || !self.layout.agent
            || !self
                .agent_panel
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            return false;
        }
        let Some((kind, start, items)) = self.composer_menu(cx) else {
            // Esc stops Claude, as in Claude Code, once no menu is open.
            if key == "escape" && self.agent_panel.running {
                self.cancel_agent(cx);
                return true;
            }
            return false;
        };
        let menus = &self.agent_panel.menus;
        if items.is_empty() {
            if key != "escape" {
                return false;
            }
            menus.dismissed.set(Some((start, kind)));
            cx.notify();
            return true;
        }
        let selected = menus.selected.get().min(items.len() - 1);
        match key {
            "up" => menus
                .selected
                .set(selected.checked_sub(1).unwrap_or(items.len() - 1)),
            "down" => menus.selected.set((selected + 1) % items.len()),
            "enter" | "tab" => self.choose_composer_item(selected, window, cx),
            _ => menus.dismissed.set(Some((start, kind))),
        }
        cx.notify();
        true
    }

    /// Dictation into Claude's message box: the box takes the keyboard,
    /// then macOS's dictation writes what is said in it.
    pub(super) fn dictate_to_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agent_panel
            .input
            .update(cx, |input, cx| input.focus(window, cx));
        // Once the box has the keyboard, which macOS reads from the window.
        cx.defer_in(window, |_, _, _| {
            crate::mac::start_dictation();
        });
    }

    /// ⌘D in Claude's message box starts dictation, as in the Claude Code
    /// extension; elsewhere it stays "add the next occurrence". Whether the
    /// key was used.
    pub(super) fn dictation_key(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = keystroke.modifiers;
        if keystroke.key != "d"
            || !modifiers.platform
            || modifiers.shift
            || modifiers.alt
            || modifiers.control
            || !self.layout.agent
            || !self
                .agent_panel
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            return false;
        }
        self.dictate_to_agent(window, cx);
        true
    }

    /// Puts an answer of Claude on the clipboard, in the Markdown it was
    /// written in.
    pub(super) fn copy_agent_answer(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.trim().to_owned()));
        self.status_message = Some(self.strings().agent_copied.to_owned());
        cx.notify();
    }

    // -----------------------------------------------------------------
    // The header

    /// What the chat is called: Claude Code's title, the one in the list of
    /// chats, or its first message.
    pub(super) fn agent_title(&self) -> String {
        let panel = &self.agent_panel;
        if let Some(title) = panel
            .thread
            .title
            .as_ref()
            .filter(|title| !title.is_empty())
        {
            return title.clone();
        }
        // In the list of the folder the chat works in.
        let listed = match &panel.chat_folder {
            Some(folder) => panel
                .other_chats
                .iter()
                .find(|(known, _)| known == folder)
                .map_or(&[][..], |(_, chats)| chats.as_slice()),
            None => panel.chats.as_slice(),
        };
        if let Some(chat) = panel
            .session
            .as_ref()
            .and_then(|id| listed.iter().find(|chat| &chat.id == id))
        {
            return chat.title.clone();
        }
        panel
            .thread
            .entries
            .iter()
            .find_map(|entry| match entry {
                Entry::User(text) => text.lines().next().map(str::to_owned),
                _ => None,
            })
            .unwrap_or_else(|| "Claude Code".to_owned())
    }

    /// The header's buttons while the chat shows: the chats, a new one, and
    /// the account with what it can do.
    pub(super) fn render_agent_header_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let account = panel.account.clone();
        let signed_in = account.as_ref().is_some_and(|account| account.signed_in);
        let uses_key = panel.uses_api_key;
        let session = panel.session.clone();
        let initial = account
            .as_ref()
            .and_then(|account| account.email.clone().or_else(|| account.label.clone()))
            .and_then(|name| name.chars().next())
            .map(|first| first.to_uppercase().to_string());
        let account_button = Button::new("agent-account")
            .ghost()
            .small()
            .tooltip(
                account
                    .as_ref()
                    .and_then(|account| account.email.clone().or_else(|| account.label.clone()))
                    .unwrap_or_else(|| strings.agent_signed_out.to_owned()),
            )
            .when_some(initial.clone(), |button, initial| {
                button.child(
                    div()
                        .size(px(18.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(claude_colour().opacity(0.85))
                        .text_size(px(10.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(gpui::white())
                        .child(initial),
                )
            })
            .when(initial.is_none(), |button| {
                button.icon(Icon::new(IconName::CircleUser))
            })
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(220.));
                if uses_key {
                    menu = menu.label(strings.agent_account_api_key).separator();
                } else if let Some(account) = &account {
                    if let Some(label) = account.label.clone() {
                        menu = menu.label(label);
                    }
                    if let Some(email) = account.email.clone() {
                        menu = menu.label(email);
                    }
                    menu = menu.separator();
                }
                if let Some(session) = session.clone() {
                    menu = menu.item(
                        PopupMenuItem::new(strings.agent_continue_terminal).on_click(
                            move |_, window, cx| {
                                window.dispatch_action(
                                    Box::new(ContinueInTerminal {
                                        session: session.clone(),
                                    }),
                                    cx,
                                )
                            },
                        ),
                    );
                    menu = menu.separator();
                }
                if uses_key {
                    menu = menu.item(PopupMenuItem::new(strings.agent_forget_key).on_click(
                        |_, window, cx| {
                            window.dispatch_action(Box::new(AgentAccount::ForgetKey), cx)
                        },
                    ));
                } else {
                    menu = menu.item(PopupMenuItem::new(strings.agent_use_key).on_click(
                        |_, window, cx| window.dispatch_action(Box::new(AgentAccount::UseKey), cx),
                    ));
                }
                if signed_in && !uses_key {
                    menu = menu.item(PopupMenuItem::new(strings.agent_logout).on_click(
                        |_, window, cx| window.dispatch_action(Box::new(AgentAccount::Logout), cx),
                    ));
                }
                menu
            });
        div()
            .flex()
            .items_center()
            .gap_0p5()
            .child(
                Button::new("agent-history")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Clock))
                    .selected(self.agent_panel.show_chats)
                    .tooltip(strings.agent_all_chats)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.agent_panel.show_chats = !this.agent_panel.show_chats;
                        if this.agent_panel.show_chats {
                            this.refresh_agent_history(window, cx);
                            this.agent_panel
                                .chat_filter
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new("agent-new")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .tooltip(strings.agent_new_chat)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.new_agent_conversation(window, cx)),
                    ),
            )
            .child(account_button)
            .into_any_element()
    }

    // -----------------------------------------------------------------
    // The body

    pub(super) fn render_agent_conversation(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        if panel.show_chats {
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(self.render_agent_chats(colors, cx))
                .child(self.render_agent_input(colors, window, cx))
                .into_any_element();
        }
        // The ways in take the place of the chat until Claude has one.
        let home =
            panel.thread.entries.is_empty() || panel.needs_login || panel.key_input.is_some();
        let body: AnyElement = if home {
            self.render_agent_home(colors, cx)
        } else {
            let mut body = div()
                .id("agent-conversation")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&panel.scroll)
                .px(px(14.))
                .pt(px(14.))
                .pb(px(8.))
                .flex()
                .flex_col()
                .gap(px(12.));
            let mut after_user = false;
            for (index, entry) in panel.thread.entries.iter().enumerate() {
                // Text with nothing to read, as a replayed chat may have,
                // leaves no gap.
                if let Entry::Agent(text) | Entry::Thought(text) = entry
                    && text.trim().is_empty()
                {
                    continue;
                }
                // A little more room before each new message of yours.
                let user = matches!(entry, Entry::User(_));
                if user && index > 0 && !after_user {
                    body = body.child(div().h(px(4.)));
                }
                after_user = user;
                body = body.child(self.render_agent_entry(index, entry, colors, window, cx));
            }
            if panel.running && panel.permissions.is_empty() {
                body = body.child(working(strings.agent_working, colors, self.look.motion));
            }
            if let Some(notice) = &panel.notice {
                body = body.child(notice_row(notice.clone(), cx));
            }
            body.into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .children(self.render_chat_folder(colors, cx))
            .child(body)
            .children(self.render_agent_plan(colors))
            .children(self.render_agent_changes(colors, cx))
            .children(
                panel
                    .permissions
                    .iter()
                    .enumerate()
                    .map(|(index, permission)| {
                        self.render_permission(index, permission, colors, cx)
                    }),
            )
            .child(self.render_agent_input(colors, window, cx))
            .into_any_element()
    }

    /// Over a chat of another project: the folder Claude works in, and a
    /// way to open that project in its own window.
    fn render_chat_folder(&self, colors: Colors, cx: &mut Context<Self>) -> Option<AnyElement> {
        let folder = self.agent_panel.chat_folder.as_ref()?;
        let strings = self.strings();
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| folder.display().to_string());
        Some(
            div()
                .id("agent-chat-folder")
                .debug_selector(|| "agent-chat-folder".to_owned())
                .flex_none()
                .h(px(30.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(6.))
                .border_b_1()
                .border_color(colors.border.opacity(0.6))
                .bg(claude_colour().opacity(0.06))
                .child(
                    Icon::new(IconName::Folder)
                        .xsmall()
                        .text_color(claude_colour()),
                )
                .child(
                    div()
                        .id("agent-chat-folder-name")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(colors.muted)
                        .tooltip({
                            let path = folder.display().to_string();
                            move |window, cx| gpui_component::tooltip::Tooltip::new(path.clone()).build(window, cx)
                        })
                        .child(strings.agent_works_in(&name)),
                )
                .child(
                    Button::new("agent-open-chat-project")
                        .ghost()
                        .xsmall()
                        .label(strings.agent_open_project)
                        .on_click(cx.listener(|this, _, _, cx| this.open_chat_project(cx))),
                )
                .into_any_element(),
        )
    }

    /// Before the first message: Claude's mark, how it is doing, a way in
    /// when it needs one, things to ask, and the latest chats.
    fn render_agent_home(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let status: Option<(String, bool)> = match &panel.status {
            AgentStatus::Installing => Some((strings.agent_installing.to_owned(), true)),
            AgentStatus::Starting if panel.session.is_some() => {
                Some((strings.agent_opening_chat.to_owned(), true))
            }
            AgentStatus::Starting => Some((strings.agent_starting.to_owned(), true)),
            AgentStatus::Failed(error) => Some((error.clone(), false)),
            AgentStatus::Off | AgentStatus::Ready => None,
        };
        let failed = matches!(panel.status, AgentStatus::Failed(_));
        let suggestion = |index: usize, text: &'static str| {
            div()
                .id(("agent-suggestion", index))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(10.))
                .border_1()
                .border_color(colors.border.opacity(0.8))
                .text_size(px(12.5))
                .text_color(colors.text)
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.agent_panel.input.update(cx, |input, cx| {
                        input.set_value(text, window, cx);
                        input.focus(window, cx);
                    });
                    cx.notify();
                }))
                .child(text)
        };
        let mut home = div()
            .id("agent-home")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(16.))
            .pt(px(28.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(claude_mark(34.))
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.text)
                            .child(strings.agent_greeting),
                    )
                    .child(
                        div()
                            .max_w(px(300.))
                            .text_center()
                            .text_size(px(12.5))
                            .line_height(px(18.))
                            .text_color(colors.muted)
                            .child(strings.agent_home_hint),
                    ),
            );
        if let Some((text, busy)) = status {
            home = home.child(
                div()
                    .px(px(12.))
                    .py(px(9.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(10.))
                    .bg(if busy {
                        colors.hover
                    } else {
                        cx.theme().warning.opacity(0.1)
                    })
                    .text_size(px(12.))
                    .text_color(if busy {
                        colors.muted
                    } else {
                        cx.theme().warning
                    })
                    .when(busy, |row| {
                        row.child(spinner(colors.accent, self.look.motion))
                    })
                    .child(div().flex_1().min_w_0().child(text))
                    .when(failed, |row| {
                        row.child(
                            Button::new("agent-start")
                                .primary()
                                .small()
                                .label(strings.agent_start)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.open_agent(window, cx)),
                                ),
                        )
                    }),
            );
        }
        if panel.needs_login || panel.key_input.is_some() {
            home = home.child(self.render_agent_login(colors, cx));
        } else {
            home = home.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(suggestion(0, strings.agent_suggest_explain))
                    .child(suggestion(1, strings.agent_suggest_bugs))
                    .child(suggestion(2, strings.agent_suggest_tests)),
            );
        }
        home = home.children(self.render_home_chats(colors, cx));
        home.into_any_element()
    }

    /// The ways in: a Claude subscription or the Anthropic Console, through
    /// Claude Code's own login, or an API key of one's own.
    fn render_agent_login(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let mut card = div()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(px(14.))
            .border_1()
            .border_color(colors.border)
            .bg(colors.editor)
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text)
                    .child(strings.agent_login_title),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .line_height(px(18.))
                    .text_color(colors.muted)
                    .child(strings.agent_login_hint),
            );
        if let Some(input) = &panel.key_input {
            return card
                .child(Input::new(input).small())
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(colors.faint)
                        .child(strings.agent_key_hint),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_1p5()
                        .child(
                            Button::new("agent-key-cancel")
                                .ghost()
                                .small()
                                .label(strings.cancel)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.agent_panel.key_input = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("agent-key-save")
                                .primary()
                                .small()
                                .label(strings.agent_key_save)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_agent_api_key(window, cx)
                                })),
                        ),
                )
                .into_any_element();
        }
        if panel.login.is_some() {
            return card
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.5))
                        .text_color(colors.text)
                        .child(spinner(colors.accent, self.look.motion))
                        .child(div().flex_1().child(strings.agent_login_waiting)),
                )
                .into_any_element();
        }
        let way = |id: &'static str, title: &'static str, words: &'static str, primary: bool| {
            let method = id.to_owned();
            div()
                .id(id)
                .px(px(12.))
                .py(px(9.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(10.))
                .border_1()
                .border_color(if primary {
                    claude_colour().opacity(0.7)
                } else {
                    colors.border
                })
                .when(primary, |row| row.bg(claude_colour().opacity(0.08)))
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.login_agent(method.clone(), window, cx)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(colors.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(colors.muted)
                                .child(words),
                        ),
                )
                .child(
                    Icon::new(IconName::ChevronRight)
                        .small()
                        .text_color(colors.faint),
                )
        };
        let offered = |id: &str| panel.auth_methods.iter().any(|method| method.id == id);
        if offered("claude-ai-login") || offered("claude-login") {
            let id = if offered("claude-ai-login") {
                "claude-ai-login"
            } else {
                "claude-login"
            };
            card = card.child(way(
                id,
                strings.agent_login_subscription,
                strings.agent_login_subscription_hint,
                true,
            ));
        }
        if offered("console-login") {
            card = card.child(way(
                "console-login",
                strings.agent_login_console,
                strings.agent_login_console_hint,
                false,
            ));
        }
        card.child(
            div().flex().justify_center().child(
                Button::new("agent-use-key")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Key))
                    .label(strings.agent_login_key)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.ask_agent_api_key(window, cx)),
                    ),
            ),
        )
        .into_any_element()
    }

    fn render_agent_entry(
        &self,
        index: usize,
        entry: &Entry,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match entry {
            // Your messages on the right, in a quiet bubble.
            Entry::User(text) => div()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(self.agent_width * 0.84))
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(14.))
                        .rounded_br(px(4.))
                        .bg(colors.text.opacity(if colors.dark { 0.08 } else { 0.06 }))
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(colors.text)
                        .child(text.clone()),
                )
                .into_any_element(),
            Entry::Agent(text) => match self.agent_panel.markdown.get(&index) {
                Some((state, _)) => {
                    // Under an answer, on hover: copy it as Claude wrote it,
                    // in Markdown. Not while it is still being written.
                    let group: gpui::SharedString = format!("agent-answer-{index}").into();
                    let live = self.agent_panel.running
                        && index + 1 == self.agent_panel.thread.entries.len();
                    let copied = text.clone();
                    div()
                        .group(group.clone())
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(13.)).child(TextView::new(state)))
                        .when(!live, |answer| {
                            answer.child(
                                div()
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .invisible()
                                    .group_hover(group, |row| row.visible())
                                    .child(
                                        Button::new(("agent-copy-answer", index))
                                            .ghost()
                                            .xsmall()
                                            .icon(Icon::new(IconName::Copy))
                                            .tooltip(self.strings().agent_copy_answer)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.copy_agent_answer(copied.clone(), cx)
                                            })),
                                    ),
                            )
                        })
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
            Entry::Thought(text) => {
                let panel = &self.agent_panel;
                let open = panel.open_thoughts.contains(&index);
                let turn = chevron_turn(("agent-thought-turn", index), open, self.look, window, cx);
                // Still thinking: the last entry of a turn that runs.
                let live = panel.running && index + 1 == panel.thread.entries.len();
                let header = div()
                    .id(("agent-thought", index))
                    .h(px(26.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover.opacity(0.6)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.agent_panel.open_thoughts.remove(&index) {
                            this.agent_panel.open_thoughts.insert(index);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.muted)
                            .child(self.strings().agent_thinking),
                    )
                    .when(live, |row| {
                        row.child(spinner(colors.accent, self.look.motion))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(colors.faint)
                            .when(!open, |preview| {
                                preview.child(
                                    text.trim().lines().next().unwrap_or_default().to_owned(),
                                )
                            }),
                    )
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .xsmall()
                            .text_color(colors.faint)
                            .rotate(gpui::radians(turn * std::f32::consts::FRAC_PI_2)),
                    );
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(header)
                    .when(open, |thought| {
                        thought.child(
                            div()
                                .id(("agent-thought-text", index))
                                .mx(px(8.))
                                .max_h(px(280.))
                                .overflow_y_scroll()
                                .px(px(12.))
                                .py(px(8.))
                                .rounded(px(8.))
                                .border_l_2()
                                .border_color(claude_colour().opacity(0.6))
                                .bg(colors.hover.opacity(0.35))
                                .text_size(px(12.5))
                                .line_height(px(19.))
                                .text_color(colors.muted)
                                .child(text.trim().to_owned()),
                        )
                    })
                    .into_any_element()
            }
            Entry::Tool(tool) => self.render_tool(index, tool, colors, window, cx),
        }
    }

    /// A tool Claude used: a quiet row with its symbol, what it did and
    /// how it went; an edit as a card with its difference, open at first.
    fn render_tool(
        &self,
        index: usize,
        tool: &ToolCall,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (success, danger) = (theme.success, theme.danger);
        let icon = match tool.kind.as_str() {
            "edit" | "delete" | "move" => IconName::Pencil,
            "read" => IconName::FileText,
            "search" => IconName::Search,
            "execute" => IconName::SquareTerminal,
            "fetch" => IconName::Globe,
            "think" => IconName::Lightbulb,
            _ => IconName::Wrench,
        };
        let has_diff = tool
            .content
            .iter()
            .any(|content| matches!(content, ToolContent::Diff { .. }));
        let toggled = self.agent_panel.open_tools.contains(&index);
        // A command may be several lines: the row shows the first, the
        // card all of it.
        let title = tool.title.trim();
        let first_line = title.lines().next().unwrap_or_default();
        let more_title = title.contains('\n') || title.chars().count() > 90;
        let command = tool.kind == "execute";
        let expandable = !tool.content.is_empty() || more_title;
        let open = expandable && (has_diff != toggled);
        let turn = chevron_turn(("agent-tool-turn", index), open, self.look, window, cx);
        let state: AnyElement = match tool.status {
            ToolStatus::Pending | ToolStatus::InProgress => {
                spinner(colors.accent, self.look.motion).into_any_element()
            }
            ToolStatus::Completed => Icon::new(IconName::Check)
                .xsmall()
                .text_color(success)
                .into_any_element(),
            ToolStatus::Failed => Icon::new(IconName::X)
                .xsmall()
                .text_color(danger)
                .into_any_element(),
        };
        // What an edit changed, in lines.
        let counts =
            tool.content.iter().fold(
                (0usize, 0usize),
                |(added, removed), content| match content {
                    ToolContent::Diff { old, new, .. } => (
                        added + new.lines().count(),
                        removed + old.as_deref().unwrap_or_default().lines().count(),
                    ),
                    _ => (added, removed),
                },
            );
        let location = tool.locations.first().cloned();
        let open_file = location.map(|(path, line)| {
            Button::new(("agent-tool-open", index))
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::ExternalLink))
                .tooltip(self.strings().agent_open_file)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    if path.is_file() {
                        this.open_file(&path, window, cx);
                        if let Some(line) = line
                            && let Some(editor) = this.active_editor()
                        {
                            editor.update(cx, |editor, cx| {
                                editor.set_cursor_position(
                                    gpui_component::input::Position::new(line.saturating_sub(1), 0),
                                    window,
                                    cx,
                                );
                            });
                        }
                    }
                }))
        });
        let row = div()
            .id(("agent-tool", index))
            .h(px(28.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .rounded(px(8.))
            .when(expandable, |row| {
                row.cursor_pointer()
                    .hover(|row| row.bg(colors.hover.opacity(0.6)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.agent_panel.open_tools.remove(&index) {
                            this.agent_panel.open_tools.insert(index);
                        }
                        cx.notify();
                    }))
            })
            .child(
                Icon::new(icon)
                    .xsmall()
                    .flex_none()
                    .text_color(colors.muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.muted)
                    .when(command, |title| {
                        title.font_family("Menlo").text_size(px(11.5))
                    })
                    .child(if more_title && title.contains('\n') {
                        format!("{first_line} …")
                    } else {
                        first_line.to_owned()
                    }),
            )
            .when(has_diff, |row| {
                row.child(
                    div()
                        .flex_none()
                        .flex()
                        .gap_1()
                        .text_size(px(11.))
                        .font_family("Menlo")
                        .child(div().text_color(success).child(format!("+{}", counts.0)))
                        .child(div().text_color(danger).child(format!("−{}", counts.1))),
                )
            })
            .children(open_file)
            .child(
                div()
                    .flex_none()
                    .w(px(14.))
                    .flex()
                    .justify_center()
                    .child(state),
            )
            .when(expandable, |row| {
                row.child(
                    Icon::new(IconName::ChevronRight)
                        .xsmall()
                        .rotate(gpui::radians(turn * std::f32::consts::FRAC_PI_2))
                        .text_color(colors.faint),
                )
            });
        if !open {
            return row.into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(colors.border.opacity(0.8))
            .bg(colors.editor.opacity(0.7))
            .child(row)
            .child(
                div()
                    .px(px(8.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(more_title, |card| {
                        card.child(
                            div()
                                .px(px(8.))
                                .py(px(6.))
                                .rounded(px(7.))
                                .bg(colors.editor)
                                .border_1()
                                .border_color(colors.border.opacity(0.6))
                                .text_size(px(11.5))
                                .line_height(px(17.))
                                .font_family("Menlo")
                                .text_color(colors.text)
                                .child(title.to_owned()),
                        )
                    })
                    .children(
                        tool.content
                            .iter()
                            .map(|content| render_tool_content(content, colors, success, danger)),
                    ),
            )
            .into_any_element()
    }

    fn render_agent_plan(&self, colors: Colors) -> Option<AnyElement> {
        let plan = &self.agent_panel.thread.plan;
        if plan.is_empty() {
            return None;
        }
        let done = plan
            .iter()
            .filter(|item| item.status == "completed")
            .count();
        let progress = done as f32 / plan.len() as f32;
        Some(
            div()
                .flex_none()
                .mx(px(10.))
                .mb(px(6.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(12.))
                .border_1()
                .border_color(colors.border)
                .bg(colors.editor.opacity(0.7))
                .flex()
                .flex_col()
                .gap(px(6.))
                .text_size(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.text)
                                .child(self.strings().agent_plan(done, plan.len())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .h(px(4.))
                                .rounded_full()
                                .bg(colors.hover)
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui::relative(progress))
                                        .rounded_full()
                                        .bg(claude_colour()),
                                ),
                        ),
                )
                .children(plan.iter().map(|item| {
                    let (mark, color) = match item.status.as_str() {
                        "completed" => (IconName::CircleCheck, claude_colour()),
                        "in_progress" => (IconName::CircleDot, colors.text),
                        _ => (IconName::Circle, colors.faint),
                    };
                    div()
                        .flex()
                        .items_start()
                        .gap_1p5()
                        .text_color(if item.status == "completed" {
                            colors.faint
                        } else {
                            colors.text
                        })
                        .child(
                            div()
                                .pt(px(1.))
                                .child(Icon::new(mark).xsmall().text_color(color)),
                        )
                        .child(div().flex_1().min_w_0().child(item.content.clone()))
                }))
                .into_any_element(),
        )
    }

    fn render_permission(
        &self,
        index: usize,
        permission: &Permission,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let mut card = div()
            .flex_none()
            .mx(px(10.))
            .mb(px(6.))
            .p(px(12.))
            .rounded(px(12.))
            .border_1()
            .border_color(claude_colour().opacity(0.55))
            .bg(claude_colour().opacity(0.06))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text)
                    .child(
                        div().pt(px(1.)).child(
                            Icon::new(IconName::ShieldCheck)
                                .small()
                                .text_color(claude_colour()),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.strings().agent_permission(&permission.title)),
                    ),
            );
        for content in &permission.content {
            card = card.child(render_tool_content(
                content,
                colors,
                theme.success,
                theme.danger,
            ));
        }
        let mut buttons = div().flex().flex_wrap().gap_1p5();
        for (number, (id, name, kind)) in permission.options.iter().enumerate() {
            let option = id.clone();
            let button = Button::new(("agent-permission", index * 16 + number))
                .small()
                .label(name.clone());
            let button = if kind.starts_with("allow") && number == 0 {
                button.primary()
            } else if kind.starts_with("reject") {
                button.ghost()
            } else {
                button.outline()
            };
            buttons = buttons.child(button.on_click(
                cx.listener(move |this, _, _, cx| this.answer_permission(index, &option, cx)),
            ));
        }
        card.child(buttons).into_any_element()
    }

    // -----------------------------------------------------------------
    // The composer

    /// The `/` or `@` menu over the composer, while a word opens one.
    fn render_composer_menu(&self, colors: Colors, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (kind, _, items) = self.composer_menu(cx)?;
        let strings = self.strings();
        let selected = self
            .agent_panel
            .menus
            .selected
            .get()
            .min(items.len().saturating_sub(1));
        let title = match kind {
            MenuKind::Slash => strings.agent_commands,
            MenuKind::Mention => strings.agent_mention,
        };
        let mut list = div()
            .id("agent-menu-list")
            .max_h(px(260.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(1.));
        if items.is_empty() {
            list = list.child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .text_color(colors.faint)
                    .child(match kind {
                        MenuKind::Slash => strings.agent_no_commands,
                        MenuKind::Mention => strings.agent_no_files,
                    }),
            );
        }
        for (index, item) in items.iter().enumerate() {
            let on = index == selected;
            let (icon, name, detail) = match item {
                ComposerItem::Command {
                    name,
                    description,
                    hint,
                } => (
                    IconName::SquareSlash,
                    format!(
                        "/{name}{}",
                        hint.as_ref()
                            .map_or(String::new(), |hint| format!(" {hint}"))
                    ),
                    description.clone(),
                ),
                ComposerItem::Path { relative, folder } => {
                    let path = Path::new(relative);
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| relative.clone());
                    let parent = path
                        .parent()
                        .map(|parent| parent.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (
                        if *folder {
                            IconName::Folder
                        } else {
                            IconName::FileText
                        },
                        name,
                        parent,
                    )
                }
                ComposerItem::Selection => (
                    IconName::TextCursorInput,
                    strings.agent_mention_selection.to_owned(),
                    String::new(),
                ),
            };
            list = list.child(
                div()
                    .id(("agent-menu-item", index))
                    .px(px(8.))
                    .py(px(5.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .when(on, |row| row.bg(colors.selected))
                    .hover(|row| row.bg(colors.hover))
                    .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                        window.prevent_default()
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.choose_composer_item(index, window, cx)
                    }))
                    .child(Icon::new(icon).xsmall().flex_none().text_color(if on {
                        colors.accent
                    } else {
                        colors.muted
                    }))
                    .child(
                        div()
                            .flex_none()
                            .max_w(gpui::relative(0.6))
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(colors.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(colors.faint)
                            .child(detail),
                    ),
            );
        }
        Some(
            div()
                .mx(px(10.))
                .mb(px(-4.))
                .p(px(4.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .rounded(px(12.))
                .border_1()
                .border_color(colors.border)
                .bg(colors.panel)
                .shadow_lg()
                .child(
                    div()
                        .px(px(8.))
                        .pt(px(4.))
                        .pb(px(2.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.faint)
                                .child(title.to_uppercase()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(colors.faint)
                                .child(strings.agent_menu_hint),
                        ),
                )
                .child(list)
                .into_any_element(),
        )
    }

    /// The message box, as a composer: what goes along, the text, and under
    /// it `@`, `/`, the mode, the model, and the button that sends or stops.
    fn render_agent_input(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let chips = panel
            .attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| {
                let name = attachment
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let label = match &attachment.selection {
                    Some(((start, end), _)) => format!("{name}:{start}-{end}"),
                    None => name,
                };
                let folder = attachment.path.is_dir();
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(7.))
                    .bg(colors.hover)
                    .text_size(px(11.5))
                    .text_color(colors.text)
                    .child(
                        Icon::new(if folder {
                            IconName::Folder
                        } else {
                            IconName::FileText
                        })
                        .xsmall()
                        .text_color(colors.muted),
                    )
                    .child(label)
                    .child(
                        Button::new(("agent-attachment", index))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::X))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if index < this.agent_panel.attachments.len() {
                                    this.agent_panel.attachments.remove(index);
                                }
                                cx.notify();
                            })),
                    )
            })
            .collect::<Vec<_>>();
        let send: AnyElement = if panel.running {
            Button::new("agent-stop")
                .icon(Icon::new(IconName::Square))
                .tooltip(strings.agent_stop)
                .small()
                .rounded(px(14.))
                .on_click(cx.listener(|this, _, _, cx| this.cancel_agent(cx)))
                .into_any_element()
        } else {
            let empty =
                panel.input.read(cx).value().trim().is_empty() && panel.attachments.is_empty();
            Button::new("agent-send")
                .primary()
                .small()
                .icon(Icon::new(IconName::ArrowUp))
                .tooltip(strings.agent_send)
                .rounded(px(14.))
                .disabled(empty)
                .on_click(cx.listener(|this, _, window, cx| this.send_to_agent(window, cx)))
                .into_any_element()
        };
        let typing = panel.input.read(cx).focus_handle(cx).is_focused(window);
        // What was written while Claude works, waiting for the turn to end.
        let queued = panel.queued.as_ref().map(|(_, said)| {
            div()
                .debug_selector(|| "agent-queued".to_owned())
                .min_h(px(26.))
                .pl(px(8.))
                .py(px(3.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(7.))
                .bg(colors.hover.opacity(0.7))
                .child(
                    Icon::new(IconName::Clock)
                        .xsmall()
                        .text_color(colors.faint),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(colors.faint)
                                .child(strings.agent_queued),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(colors.text)
                                .child(said.replace('\n', " ")),
                        ),
                )
                .child(
                    Button::new("agent-unqueue")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::X))
                        .tooltip(strings.agent_unqueue)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.agent_panel.queued = None;
                            cx.notify();
                        })),
                )
        });

        let mode_button = self.render_mode_button(colors, cx);
        let model_button = self.render_model_button(colors, cx);
        let context_button = self.render_context_button(colors, cx);
        let popover = self.render_composer_popover(colors, cx);
        let insert =
            |id: &'static str, icon: IconName, tooltip: &'static str, character: &'static str| {
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(icon))
                    .tooltip(tooltip)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.agent_panel.menus.dismissed.set(None);
                        this.agent_panel.input.update(cx, |input, cx| {
                            let text = input.value().to_string();
                            let at = input.cursor().min(text.len());
                            let spacer = if at > 0 && !text[..at].ends_with(char::is_whitespace) {
                                " "
                            } else {
                                ""
                            };
                            let new_text =
                                format!("{}{spacer}{character}{}", &text[..at], &text[at..]);
                            let cursor = at + spacer.len() + character.len();
                            input.set_value(new_text, window, cx);
                            input.set_selected_range(cursor..cursor, cx);
                            input.focus(window, cx);
                        });
                        cx.notify();
                    }))
            };
        let mention = insert(
            "agent-insert-mention",
            IconName::AtSign,
            strings.agent_add_file,
            "@",
        );
        let command = insert(
            "agent-insert-command",
            IconName::SquareSlash,
            strings.agent_commands,
            "/",
        );
        let dictate = Button::new("agent-dictate")
            .ghost()
            .xsmall()
            .icon(Icon::new(IconName::Mic))
            .tooltip(strings.agent_dictate)
            .on_click(cx.listener(|this, _, window, cx| this.dictate_to_agent(window, cx)));
        let menu = self.render_composer_menu(colors, cx);
        div()
            .flex_none()
            .flex()
            .flex_col()
            .children(menu)
            .child(
                div().relative().children(popover).child(
                    div()
                        .id("agent-input-box")
                        .drag_over::<super::explorer::DraggedPath>(move |style, _, _, _| {
                            style
                                .border_color(colors.accent)
                                .bg(colors.accent.opacity(0.06))
                        })
                        .drag_over::<gpui::ExternalPaths>(move |style, _, _, _| {
                            style
                                .border_color(colors.accent)
                                .bg(colors.accent.opacity(0.06))
                        })
                        .on_drop(cx.listener(
                            |this, dragged: &super::explorer::DraggedPath, window, cx| {
                                this.attach_to_agent(dragged.all(), window, cx)
                            },
                        ))
                        .on_drop(
                            cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                                this.attach_to_agent(paths.paths().to_vec(), window, cx)
                            }),
                        )
                        // A pasted picture, such as a screenshot, is attached too.
                        .capture_action(cx.listener(
                            |this, _: &gpui_component::input::Paste, window, cx| {
                                if this.paste_image_into_chat(window, cx) {
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .m(px(10.))
                        .pt(px(6.))
                        .pb(px(6.))
                        .px(px(8.))
                        .rounded(px(14.))
                        .border_1()
                        .border_color(if typing {
                            colors.accent.opacity(0.6)
                        } else {
                            colors.border
                        })
                        .bg(colors.editor)
                        .shadow_sm()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(queued)
                        .when(!panel.attachments.is_empty(), |input| {
                            input.child(div().pt_0p5().flex().flex_wrap().gap_1().children(chips))
                        })
                        .child(
                            div()
                                .text_size(px(13.))
                                .child(Textarea::new(&panel.input).appearance(false)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .child(mention)
                                .child(command)
                                .child(dictate)
                                .children(mode_button)
                                .child(div().flex_1())
                                .children(context_button)
                                .children(model_button)
                                .child(div().w(px(4.)))
                                .child(send),
                        ),
                ),
            )
            .into_any_element()
    }
}

/// A notice under the conversation, such as an error.
fn notice_row(text: String, cx: &App) -> gpui::Div {
    div()
        .px(px(10.))
        .py(px(7.))
        .rounded(px(8.))
        .bg(cx.theme().warning.opacity(0.1))
        .text_size(px(12.))
        .text_color(cx.theme().warning)
        .child(text)
}

/// A small circle turning while something runs; still without movements.
pub(super) fn spinner(color: Hsla, motion: bool) -> AnyElement {
    let icon = Icon::new(IconName::LoaderCircle).xsmall().text_color(color);
    if !motion {
        return icon.into_any_element();
    }
    icon.with_animation(
        "spinner",
        Animation::new(std::time::Duration::from_millis(900)).repeat(),
        |icon, delta| icon.rotate(gpui::radians(delta * std::f32::consts::TAU)),
    )
    .into_any_element()
}

/// "Claude is working…", next to Claude's mark breathing.
fn working(text: &'static str, colors: Colors, motion: bool) -> gpui::Div {
    let mark = claude_mark(14.);
    let mark: AnyElement = if motion {
        mark.with_animation(
            "working-mark",
            Animation::new(std::time::Duration::from_millis(1600)).repeat(),
            |mark, delta| {
                let breath = 0.45 + 0.55 * ((delta * std::f32::consts::TAU).sin() * 0.5 + 0.5);
                mark.opacity(breath)
            },
        )
        .into_any_element()
    } else {
        mark.into_any_element()
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(12.))
        .text_color(colors.faint)
        .child(mark)
        .child(text)
}

/// Seconds since 1970 of the last local midnight before `now`.
pub(super) fn local_midnight(now: i64) -> i64 {
    let time = now as libc::time_t;
    // SAFETY: localtime_r writes only into the tm it is given.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let found = unsafe { !libc::localtime_r(&time, &mut tm).is_null() };
    if !found {
        return now - now.rem_euclid(86_400);
    }
    now - (tm.tm_hour as i64 * 3600 + tm.tm_min as i64 * 60 + tm.tm_sec as i64)
}

/// Changes an option of the session, from the model's menu.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = agent, no_json)]
pub struct SetAgentOption {
    pub id: String,
    pub value: String,
}

/// Continues a chat in Claude Code in the terminal.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = agent, no_json)]
pub struct ContinueInTerminal {
    pub session: String,
}

/// What the account's menu does.
#[derive(Clone, Copy, Debug, PartialEq, gpui::Action)]
#[action(namespace = agent, no_json)]
pub enum AgentAccount {
    UseKey,
    ForgetKey,
    Logout,
}

/// A tool's text, or a file difference with its removed lines in red and
/// added lines in green.
fn render_tool_content(
    content: &ToolContent,
    colors: Colors,
    added: gpui::Hsla,
    removed: gpui::Hsla,
) -> AnyElement {
    match content {
        ToolContent::Text(text) => {
            let shown: String = text.lines().take(6).collect::<Vec<_>>().join("\n");
            div()
                .text_size(px(11.5))
                .font_family("Menlo")
                .text_color(colors.muted)
                .child(shown)
                .into_any_element()
        }
        ToolContent::Diff { path, old, new } => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut lines: Vec<(char, String)> = Vec::new();
            for line in old.as_deref().unwrap_or_default().lines() {
                lines.push(('-', line.to_owned()));
            }
            for line in new.lines() {
                lines.push(('+', line.to_owned()));
            }
            let more = lines.len().saturating_sub(DIFF_LINES);
            div()
                .flex()
                .flex_col()
                .rounded(px(7.))
                .bg(colors.editor)
                .border_1()
                .border_color(colors.border.opacity(0.6))
                .px(px(8.))
                .py(px(6.))
                .text_size(px(11.))
                .font_family("Menlo")
                .child(div().pb_1().text_color(colors.muted).child(name))
                .children(lines.into_iter().take(DIFF_LINES).map(|(sign, line)| {
                    let color = if sign == '+' { added } else { removed };
                    div()
                        .px_1()
                        .rounded(px(3.))
                        .bg(color.opacity(0.1))
                        .text_color(color)
                        .truncate()
                        .child(format!("{sign} {line}"))
                }))
                .when(more > 0, |diff| {
                    diff.child(
                        div()
                            .pt_0p5()
                            .text_color(colors.faint)
                            .child(format!("… +{more}")),
                    )
                })
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::tests::test_settings;
    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Writes `text` in the message box, with the cursor at its end.
    fn type_text(view: &Entity<Workbench>, cx: &mut VisualTestContext, text: &str) {
        view.update_in(cx, |view, window, cx| {
            view.agent_panel.input.update(cx, |input, cx| {
                input.set_value(text, window, cx);
                input.set_selected_range(text.len()..text.len(), cx);
            });
            view.agent_panel.menus.text_changed();
        });
    }

    #[gpui::test]
    fn the_composer_offers_commands_files_models_and_the_account(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let project = tempfile::tempdir().unwrap();
        let root = std::path::absolute(project.path()).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("docs/guide.md"), "# Guide\n").unwrap();
        let document = Document::open(root.join("src/main.rs")).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(Some(document), test_settings(), None, window, cx);
            workbench.agent_panel.fake = true;
            workbench
        });
        view.update_in(cx, |view, window, cx| {
            view.open_agent(window, cx);
            view.index_files(cx);
        });
        wait_until(cx, "the session and its commands", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel.session.is_some()
                    && !view.agent_panel.commands.is_empty()
                    && view.file_index.is_some()
            })
        });

        // `/` at the start offers the commands and skills; one goes in.
        type_text(&view, cx, "/re");
        view.update_in(cx, |view, window, cx| {
            let (kind, start, items) = view.composer_menu(cx).unwrap();
            assert_eq!((kind, start), (MenuKind::Slash, 0));
            assert!(matches!(&items[0], ComposerItem::Command { name, .. } if name == "review"));
            view.choose_composer_item(0, window, cx);
            assert_eq!(view.agent_panel.input.read(cx).value().as_ref(), "/review ");
            assert!(view.composer_menu(cx).is_none());
        });

        // `@` finds files and folders of the project; one goes along.
        type_text(&view, cx, "guarda @guide");
        view.update_in(cx, |view, window, cx| {
            let (kind, _, items) = view.composer_menu(cx).unwrap();
            assert_eq!(kind, MenuKind::Mention);
            let index = items
                .iter()
                .position(|item| {
                    matches!(item, ComposerItem::Path { relative, folder: false } if relative == "docs/guide.md")
                })
                .unwrap();
            view.choose_composer_item(index, window, cx);
            assert_eq!(view.agent_panel.input.read(cx).value().as_ref(), "guarda ");
            assert_eq!(view.agent_panel.attachments[0].path, root.join("docs/guide.md"));
        });
        type_text(&view, cx, "@doc");
        view.read_with(cx, |view, cx| {
            let (_, _, items) = view.composer_menu(cx).unwrap();
            assert!(items.contains(&ComposerItem::Path {
                relative: "docs".to_owned(),
                folder: true
            }));
        });
        // The arrows choose, Esc closes the menu until the word ends.
        view.update_in(cx, |view, window, cx| {
            view.agent_panel
                .input
                .update(cx, |input, cx| input.focus(window, cx));
        });
        cx.simulate_keystrokes("down");
        view.read_with(cx, |view, _| {
            assert_eq!(view.agent_panel.menus.selected.get(), 1)
        });
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |view, cx| assert!(view.composer_menu(cx).is_none()));
        // Return in a `/` menu uses the command instead of sending.
        type_text(&view, cx, "/comp");
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |view, cx| {
            assert_eq!(
                view.agent_panel.input.read(cx).value().as_ref(),
                "/compact "
            );
            assert!(view.agent_panel.thread.entries.is_empty());
        });

        // The model changes, and the panel draws with its menus.
        view.update(cx, |view, cx| {
            view.set_agent_option("model".to_owned(), "sonnet".to_owned(), cx)
        });
        wait_until(cx, "the model", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_option("model")
                    .and_then(|model| model.current.as_deref())
                    == Some("sonnet")
            })
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.agent_option("model").unwrap().values.len(), 2);
        });
        type_text(&view, cx, "/");
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        // A message: the context's ring fills, and the cards draw.
        type_text(&view, cx, "ciao");
        view.update_in(cx, |view, window, cx| view.send_to_agent(window, cx));
        wait_until(cx, "the context", |cx| {
            view.read_with(cx, |view, _| view.agent_panel.context.is_some())
        });
        view.read_with(cx, |view, _| {
            let context = view.agent_panel.context.clone().unwrap();
            assert_eq!(
                (context.used, context.size, context.cost),
                (313_000, 1_000_000, Some(0.42))
            );
        });
        for card in [
            super::super::agent_cards::ComposerPopover::Mode,
            super::super::agent_cards::ComposerPopover::Model,
        ] {
            view.update(cx, |view, cx| view.toggle_composer_popover(card, cx));
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
        }
        // The usage card asks `/usage` in a session of its own, then drops it.
        view.update(cx, |view, cx| {
            view.toggle_composer_popover(super::super::agent_cards::ComposerPopover::Usage, cx)
        });
        wait_until(cx, "the plan's limits", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel
                    .plan_usage
                    .as_ref()
                    .is_some_and(|usage| usage.limits.len() == 2)
            })
        });
        view.read_with(cx, |view, _| {
            assert!(view.agent_panel.usage_session.is_none());
            assert_eq!(
                view.agent_panel
                    .plan_usage
                    .as_ref()
                    .unwrap()
                    .plan
                    .as_deref(),
                Some("pro")
            );
            // The chat itself did not get the `/usage` answer.
            assert!(
                view.agent_panel
                    .thread
                    .entries
                    .iter()
                    .all(|entry| !matches!(entry,
                Entry::Agent(text) if text.contains("5-hour")))
            );
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        // A chat is deleted on the second click of its bin.
        view.update_in(cx, |view, window, cx| {
            view.refresh_agent_history(window, cx)
        });
        wait_until(cx, "the chat in the list", |cx| {
            view.read_with(cx, |view, _| view.agent_panel.chats.len() == 1)
        });
        let id = view.read_with(cx, |view, _| view.agent_panel.chats[0].id.clone());
        view.update(cx, |view, cx| view.ask_delete_agent_chat(id.clone(), cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.agent_panel.deleting.as_deref(), Some(id.as_str()));
            assert_eq!(view.agent_panel.chats.len(), 1);
        });
        view.update(cx, |view, cx| view.ask_delete_agent_chat(id.clone(), cx));
        view.read_with(cx, |view, _| {
            assert!(view.agent_panel.chats.is_empty() && view.agent_panel.session.is_none())
        });
        // Claude Code has it no more.
        wait_until(cx, "the chat gone from Claude Code", |cx| {
            view.update_in(cx, |view, window, cx| {
                view.refresh_agent_history(window, cx)
            });
            cx.run_until_parked();
            view.read_with(cx, |view, _| view.agent_panel.chats.is_empty())
        });

        // Logging out offers the ways in.
        view.update_in(cx, |view, window, cx| view.logout_agent(window, cx));
        wait_until(cx, "the logout", |cx| {
            view.read_with(cx, |view, _| view.agent_panel.needs_login)
        });
        view.read_with(cx, |view, _| {
            assert!(!view.agent_panel.account.as_ref().unwrap().signed_in);
            assert_eq!(view.agent_panel.auth_methods.len(), 2);
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    #[test]
    fn the_word_at_the_cursor_opens_its_menu() {
        assert_eq!(
            trigger_at("/rev", 4),
            Some((MenuKind::Slash, 0, "rev".to_owned()))
        );
        assert_eq!(
            trigger_at("  /", 3),
            Some((MenuKind::Slash, 2, String::new()))
        );
        // A `/` later in the message is just a slash.
        assert_eq!(trigger_at("a/b", 3), None);
        assert_eq!(trigger_at("look at /tmp", 12), None);
        assert_eq!(
            trigger_at("look at @src/ma", 15),
            Some((MenuKind::Mention, 8, "src/ma".to_owned()))
        );
        assert_eq!(
            trigger_at("@", 1),
            Some((MenuKind::Mention, 0, String::new()))
        );
        // Past the word, the menu closes.
        assert_eq!(trigger_at("@main.rs ", 9), None);
        assert_eq!(trigger_at("email me@x", 10), None);
        assert_eq!(
            trigger_at("caffè @é", 11),
            Some((MenuKind::Mention, 7, "é".to_owned()))
        );
    }
}
