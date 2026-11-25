//! Claude's chats in the panel, as the Claude app's sidebar keeps them: a
//! card for each project, the open one first, with its name, a fold and how
//! many chats it has; in it the chats, the latest first. The home shows the
//! latest few of each project, the list of chats all of them, this
//! project's by day, with a field to find one.

use std::path::{Path, PathBuf};

use ai::history::Chat;
use gpui::{AnyElement, SharedString, StatefulInteractiveElement as _};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::agent_view::{claude_colour, claude_mark};
use super::shell::{Colors, heading};
use super::*;

/// How many chats of this project the home shows, how many other projects,
/// and how many chats of each.
const HOME_CHATS: usize = 5;
const HOME_PROJECTS: usize = 4;
const HOME_PROJECT_CHATS: usize = 3;
/// Where a chat's title starts in a card: under the project's name.
const ROW_INDENT: f32 = 34.;

/// One project's card: its folder, whether it is the open project, the
/// chats it shows and how many it has in all.
struct ProjectCard<'a> {
    folder: PathBuf,
    here: bool,
    chats: Vec<&'a Chat>,
    total: usize,
    /// This project's chats by day, in the list of chats.
    by_day: bool,
}

impl Workbench {
    /// The chats on the home: a card for this project and for the others.
    pub(super) fn render_home_chats(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let strings = self.strings();
        let panel = &self.agent_panel;
        if panel.chats.is_empty() && panel.other_chats.is_empty() {
            return None;
        }
        let mut cards = Vec::new();
        if !panel.chats.is_empty() {
            cards.push(ProjectCard {
                folder: self.agent_folder(),
                here: true,
                chats: panel.chats.iter().take(HOME_CHATS).collect(),
                total: panel.chats.len(),
                by_day: false,
            });
        }
        for (folder, chats) in panel.other_chats.iter().take(HOME_PROJECTS) {
            cards.push(ProjectCard {
                folder: folder.clone(),
                here: false,
                chats: chats.iter().take(HOME_PROJECT_CHATS).collect(),
                total: chats.len(),
                by_day: false,
            });
        }
        let mut section = div().flex().flex_col().gap(px(8.)).child(
            div()
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .child(heading(strings.agent_recent_chats, colors)),
                )
                .child(
                    Button::new("agent-all-chats")
                        .ghost()
                        .small()
                        .label(strings.agent_all_chats)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.show_all_chats(window, cx)),
                        ),
                ),
        );
        for (index, card) in cards.into_iter().enumerate() {
            section = section.child(self.render_project_card(("home", index), card, colors, cx));
        }
        Some(section.into_any_element())
    }

    /// The list of chats: every project's card, with a field to find one.
    pub(super) fn render_agent_chats(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let filter = panel.chat_filter.read(cx).value().to_lowercase();
        let matches =
            |chat: &&Chat| filter.is_empty() || chat.title.to_lowercase().contains(&filter);
        let mut cards = Vec::new();
        let here: Vec<&Chat> = panel.chats.iter().filter(matches).collect();
        if !here.is_empty() {
            cards.push(ProjectCard {
                folder: self.agent_folder(),
                here: true,
                total: here.len(),
                chats: here,
                by_day: true,
            });
        }
        for (folder, chats) in &panel.other_chats {
            let chats: Vec<&Chat> = chats.iter().filter(matches).collect();
            if !chats.is_empty() {
                cards.push(ProjectCard {
                    folder: folder.clone(),
                    here: false,
                    total: chats.len(),
                    chats,
                    by_day: false,
                });
            }
        }
        let mut list = div()
            .id("agent-chats")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.))
            .pt_1()
            .pb_3()
            .flex()
            .flex_col()
            .gap(px(8.));
        if cards.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_6()
                    .text_sm()
                    .text_center()
                    .text_color(colors.muted)
                    .child(if panel.chats.is_empty() && panel.other_chats.is_empty() {
                        strings.agent_no_chats
                    } else {
                        strings.agent_no_match
                    }),
            );
        }
        // While searching, folded projects open: what was found shows.
        let searching = !filter.is_empty();
        for (index, card) in cards.into_iter().enumerate() {
            list = list.child(self.render_project_card_with(
                ("list", index),
                card,
                searching,
                colors,
                cx,
            ));
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div().px(px(12.)).pt(px(10.)).pb(px(6.)).child(
                    Input::new(&panel.chat_filter)
                        .small()
                        .prefix(Icon::new(IconName::Search).small().text_color(colors.faint))
                        .cleanable(true),
                ),
            )
            .child(list)
            .into_any_element()
    }

    /// Shows every chat, in place of the chat.
    pub(super) fn show_all_chats(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agent_panel.show_chats = true;
        self.refresh_agent_history(window, cx);
        self.agent_panel
            .chat_filter
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn render_project_card(
        &self,
        key: (&'static str, usize),
        card: ProjectCard,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_project_card_with(key, card, false, colors, cx)
    }

    fn render_project_card_with(
        &self,
        key: (&'static str, usize),
        card: ProjectCard,
        unfold: bool,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let id = |part: &str| SharedString::from(format!("agent-{}-{}-{part}", key.0, key.1));
        let folded = !unfold && self.agent_panel.folded_projects.contains(&card.folder);
        let name = card
            .folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| card.folder.display().to_string());
        let folder = card.folder.clone();
        let header = div()
            .id(id("header"))
            .h(px(34.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(9.))
            .cursor_pointer()
            .hover(|header| header.bg(colors.hover.opacity(0.6)))
            .on_click(cx.listener(move |this, _, _, cx| {
                let folded = &mut this.agent_panel.folded_projects;
                if !folded.remove(&folder) {
                    folded.insert(folder.clone());
                }
                cx.notify();
            }))
            .child(
                Icon::new(IconName::ChevronRight)
                    .xsmall()
                    .text_color(colors.faint)
                    .rotate(gpui::radians(if folded {
                        0.
                    } else {
                        std::f32::consts::FRAC_PI_2
                    })),
            )
            .child(
                div()
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .bg(if card.here {
                        claude_colour().opacity(0.16)
                    } else {
                        colors.hover
                    })
                    .child(
                        Icon::new(if card.here {
                            IconName::FolderOpen
                        } else {
                            IconName::Folder
                        })
                        .xsmall()
                        .text_color(if card.here {
                            claude_colour()
                        } else {
                            colors.muted
                        }),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text)
                    .child(name),
            )
            .when(card.here, |header| {
                header.child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(colors.faint)
                        .child(strings.agent_this_project),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .min_w(px(20.))
                    .h(px(18.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(colors.hover)
                    .text_size(px(10.5))
                    .text_color(colors.muted)
                    .child(card.total.to_string()),
            );
        let mut body = div().flex().flex_col().pb(px(4.));
        if !folded {
            let elsewhere = (!card.here).then(|| card.folder.clone());
            let mut row = 0usize;
            let mut push = |body: gpui::Div, chat: &Chat, row: &mut usize| {
                let element = self.render_chat_row(
                    id(&format!("row-{row}")),
                    chat,
                    elsewhere.as_deref(),
                    card.here,
                    colors,
                    cx,
                );
                *row += 1;
                body.child(element)
            };
            if card.by_day {
                for (label, chats) in by_day(&card.chats, strings) {
                    body = body.child(
                        div()
                            .pl(px(ROW_INDENT))
                            .pt(px(6.))
                            .pb(px(2.))
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.faint)
                            .child(label.to_uppercase()),
                    );
                    for chat in chats {
                        body = push(body, chat, &mut row);
                    }
                }
            } else {
                for chat in &card.chats {
                    body = push(body, chat, &mut row);
                }
            }
            let hidden = card.total.saturating_sub(card.chats.len());
            if hidden > 0 {
                body = body.child(
                    div()
                        .id(id("more"))
                        .h(px(28.))
                        .pl(px(ROW_INDENT))
                        .flex()
                        .items_center()
                        .gap_1()
                        .rounded(px(8.))
                        .text_size(px(12.))
                        .text_color(colors.muted)
                        .cursor_pointer()
                        .hover(|more| more.text_color(colors.text))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.show_all_chats(window, cx)),
                        )
                        .child(strings.agent_show_all(card.total))
                        .child(Icon::new(IconName::ChevronRight).xsmall()),
                );
            }
        }
        div()
            .p(px(4.))
            .flex()
            .flex_col()
            .rounded(px(12.))
            .border_1()
            .border_color(colors.border.opacity(0.7))
            .bg(colors.hover.opacity(0.18))
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// A chat in a card: its title, the Claude app's mark when it is one of
    /// the app's in this project, and how long ago it changed; the bin on
    /// hover, for the chats Keel can delete.
    fn render_chat_row(
        &self,
        id: SharedString,
        chat: &Chat,
        folder: Option<&Path>,
        here: bool,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let elsewhere = folder.map(Path::to_path_buf);
        let italian = self.language == UiLanguage::Italian;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        let strings = self.strings();
        let current = here && self.agent_panel.session.as_deref() == Some(chat.id.as_str());
        let chat_id = chat.id.clone();
        let when =
            (chat.updated > 0).then(|| super::scm_graph::relative_time(chat.updated, now, italian));
        let group: SharedString = format!("{id}-group").into();
        // The second click on the bin: the row asks before deleting.
        if self.agent_panel.deleting.as_deref() == Some(chat.id.as_str()) {
            let (yes, title) = (chat.id.clone(), chat.title.clone());
            return div()
                .id(id.clone())
                .min_h(px(34.))
                .pl(px(ROW_INDENT))
                .pr(px(6.))
                .py(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(8.))
                .bg(cx.theme().danger.opacity(0.08))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.5))
                                .text_color(colors.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(cx.theme().danger)
                                .child(strings.agent_delete_question),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("{id}-no")))
                        .ghost()
                        .xsmall()
                        .label(strings.cancel)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.agent_panel.deleting = None;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("{id}-yes")))
                        .danger()
                        .xsmall()
                        .label(strings.agent_delete)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.ask_delete_agent_chat(yes.clone(), cx)
                        })),
                )
                .into_any_element();
        }
        let deletable = !chat.app;
        let delete_id = chat.id.clone();
        div()
            .id(id.clone())
            .group(group.clone())
            .relative()
            .h(px(32.))
            .pl(px(ROW_INDENT))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .text_size(px(13.))
            .cursor_pointer()
            .when(current, |row| row.bg(claude_colour().opacity(0.12)))
            .when(!current, |row| {
                row.hover(|row| row.bg(colors.hover.opacity(0.7)))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_agent_chat(chat_id.clone(), elsewhere.clone(), window, cx)
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text)
                    .when(current, |title| title.font_weight(FontWeight::MEDIUM))
                    .child(chat.title.clone()),
            )
            // In this project, the Claude app's chats among Claude Code's.
            .when(chat.app && here, |row| {
                row.child(claude_mark(11.).opacity(0.8))
            })
            .child(
                div()
                    .flex_none()
                    .min_w(px(36.))
                    .flex()
                    .justify_end()
                    .text_size(px(11.5))
                    .text_color(colors.faint)
                    .when(deletable, |when| {
                        when.group_hover(group.clone(), |when| when.invisible())
                    })
                    .children(when),
            )
            .when(deletable, |row| {
                row.child(
                    div()
                        .absolute()
                        .right(px(4.))
                        .invisible()
                        .group_hover(group, |bin| bin.visible())
                        .child(
                            Button::new(SharedString::from(format!("{id}-delete")))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::Trash))
                                .tooltip(strings.agent_delete_chat)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.ask_delete_agent_chat(delete_id.clone(), cx)
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}

/// Chats, the latest first, by day: today, yesterday, this week, before.
fn by_day<'a>(chats: &[&'a Chat], strings: &'static Strings) -> Vec<(&'static str, Vec<&'a Chat>)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let midnight = super::agent_view::local_midnight(now);
    let days = [
        (strings.agent_today, midnight),
        (strings.agent_yesterday, midnight - 86_400),
        (strings.agent_this_week, midnight - 6 * 86_400),
        (strings.agent_older, i64::MIN),
    ];
    let mut newer = i64::MAX;
    let mut found = Vec::new();
    for (label, since) in days {
        let day: Vec<&Chat> = chats
            .iter()
            .copied()
            .filter(|chat| chat.updated >= since && chat.updated < newer)
            .collect();
        newer = since;
        if !day.is_empty() {
            found.push((label, day));
        }
    }
    found
}
