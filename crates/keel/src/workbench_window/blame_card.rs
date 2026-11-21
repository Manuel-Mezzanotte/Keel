//! The card a line's blame shows when the mouse rests on it, as GitLens
//! and JetBrains do: who wrote it and when, the whole message, and the
//! commit in Git Graph or on the remote's site.

use git::CommitDetails;
use gpui::{
    Anchor, AnyElement, Pixels, Point, SharedString, StatefulInteractiveElement as _, anchored,
    deferred, point,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Sizable as _};

use super::git_graph::format_date;
use super::shell::Colors;
use super::*;

/// How long the mouse rests on a blame before its card opens.
const OPEN_AFTER: Duration = Duration::from_millis(450);
/// How long an open card waits, once the mouse left, for it to come back.
const CLOSE_AFTER: Duration = Duration::from_millis(250);
const CARD_WIDTH: f32 = 440.;

pub(super) struct BlameCard {
    pub(super) hash: String,
    anchor: Point<Pixels>,
    corner: Anchor,
    pub(super) details: Option<CommitDetails>,
    /// The repository's page on its site, for the commit's link.
    pub(super) web: Option<String>,
    over_trigger: bool,
    over_card: bool,
}

#[derive(Default)]
pub(super) struct BlameHover {
    pub(super) card: Option<BlameCard>,
    /// The commit whose card is about to open.
    pending: Option<String>,
    /// The card about to open or to close.
    timer: Option<Task<()>>,
    load: Option<Task<()>>,
}

/// The site a remote's web address is on, as its link names it.
fn site_name(web: &str) -> String {
    let host = web
        .split("://")
        .nth(1)
        .unwrap_or(web)
        .split('/')
        .next()
        .unwrap_or_default();
    match host {
        "github.com" => "GitHub".to_owned(),
        "bitbucket.org" => "Bitbucket".to_owned(),
        host if host.contains("gitlab") => "GitLab".to_owned(),
        host => host.to_owned(),
    }
}

/// A commit message's first line, and the rest without the blank line.
fn subject_and_body(message: &str) -> (&str, &str) {
    match message.split_once('\n') {
        Some((subject, body)) => (subject.trim(), body.trim()),
        None => (message.trim(), ""),
    }
}

/// A message's body joined into paragraphs, as the card is narrower than
/// the 72 columns commits are wrapped at: the lines of a paragraph run on,
/// a list's items and a blank line keep their own lines.
fn reflow(body: &str) -> String {
    let mut text = String::new();
    for line in body.lines() {
        let line = line.trim_end();
        let item = {
            let trimmed = line.trim_start();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("• ")
                || trimmed
                    .split_once(". ")
                    .is_some_and(|(number, _)| number.parse::<u32>().is_ok())
        };
        let runs_on = !text.is_empty() && !text.ends_with('\n') && !line.is_empty() && !item;
        if runs_on {
            text.push(' ');
            text.push_str(line.trim_start());
        } else {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(line);
        }
    }
    text
}

impl Workbench {
    /// The commit of the line under the cursor, for the status bar.
    pub(super) fn current_line_hash(&self, cx: &App) -> Option<String> {
        let pane = self.panes.get(self.active)?;
        let state = self.blame.get(&pane.uri)?;
        let line = pane.editor.read(cx).cursor_position().line as usize;
        Some(state.lines.get(line)?.hash.clone())
    }

    /// The mouse came onto a blame of commit `hash`, or left it (`None`).
    /// The card opens at `anchor`, its `corner` there.
    pub(super) fn hover_blame(
        &mut self,
        hash: Option<String>,
        anchor: Point<Pixels>,
        corner: Anchor,
        cx: &mut Context<Self>,
    ) {
        // Lines not committed yet have no commit to show.
        let Some(hash) = hash.filter(|hash| !hash.bytes().all(|byte| byte == b'0')) else {
            if let Some(card) = &mut self.blame_hover.card {
                card.over_trigger = false;
            }
            self.close_blame_card_soon(cx);
            return;
        };
        if let Some(card) = self
            .blame_hover
            .card
            .as_mut()
            .filter(|card| card.hash == hash)
        {
            card.over_trigger = true;
            self.blame_hover.timer = None;
            return;
        }
        if self.blame_hover.pending.as_ref() == Some(&hash) {
            return;
        }
        if self.blame_hover.card.take().is_some() {
            cx.notify();
        }
        self.blame_hover.pending = Some(hash.clone());
        self.blame_hover.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(OPEN_AFTER).await;
            this.update(cx, |this, cx| {
                this.open_blame_card(hash, anchor, corner, cx)
            })
            .ok();
        }));
    }

    pub(super) fn open_blame_card(
        &mut self,
        hash: String,
        anchor: Point<Pixels>,
        corner: Anchor,
        cx: &mut Context<Self>,
    ) {
        self.blame_hover.pending = None;
        self.blame_hover.timer = None;
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let remote = self
            .scm
            .status
            .upstream
            .as_deref()
            .and_then(|upstream| upstream.split('/').next())
            .unwrap_or("origin")
            .to_owned();
        self.blame_hover.card = Some(BlameCard {
            hash: hash.clone(),
            anchor,
            corner,
            details: None,
            web: None,
            over_trigger: true,
            over_card: false,
        });
        let task = cx.background_spawn({
            let hash = hash.clone();
            async move {
                (
                    repo.commit_details(&hash).ok(),
                    repo.remote_web_url(&remote),
                )
            }
        });
        self.blame_hover.load = Some(cx.spawn(async move |this, cx| {
            let (details, web) = task.await;
            this.update(cx, |this, cx| {
                if let Some(card) = this
                    .blame_hover
                    .card
                    .as_mut()
                    .filter(|card| card.hash == hash)
                {
                    card.details = details;
                    card.web = web;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Closes the card unless the mouse comes back to it or its blame.
    fn close_blame_card_soon(&mut self, cx: &mut Context<Self>) {
        self.blame_hover.pending = None;
        if self.blame_hover.card.is_none() {
            self.blame_hover.timer = None;
            return;
        }
        self.blame_hover.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CLOSE_AFTER).await;
            this.update(cx, |this, cx| {
                let away = this
                    .blame_hover
                    .card
                    .as_ref()
                    .is_some_and(|card| !card.over_trigger && !card.over_card);
                if away {
                    this.close_blame_card(cx);
                }
            })
            .ok();
        }));
    }

    pub(super) fn close_blame_card(&mut self, cx: &mut Context<Self>) {
        self.blame_hover = BlameHover::default();
        cx.notify();
    }

    /// The blame in the status bar: its card opens above it.
    pub(super) fn hover_status_blame(
        &mut self,
        hovered: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let hash = self.current_line_hash(cx).filter(|_| hovered);
        let anchor = point(
            window.mouse_position().x - px(28.),
            window.viewport_size().height - px(self.look.status_height()) - px(2.),
        );
        self.hover_blame(hash, anchor, Anchor::BottomLeft, cx);
    }

    pub(super) fn render_blame_card(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let card = self.blame_hover.card.as_ref()?;
        let details = card.details.as_ref()?;
        let strings = self.strings();
        let theme = cx.theme();
        let italian = self.language == UiLanguage::Italian;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_secs() as i64);
        let when = format!(
            "{} · {}",
            format_date(details.time, italian),
            super::notices::ago(details.time, now, strings)
        );
        let (subject, body) = subject_and_body(&details.message);
        let short: String = details.hash.chars().take(8).collect();
        let initial: String = details
            .author
            .chars()
            .next()
            .map(|letter| letter.to_uppercase().collect())
            .unwrap_or_default();
        let hash = card.hash.clone();
        let web = card
            .web
            .clone()
            .map(|web| (site_name(&web), format!("{web}/commit/{hash}")));
        let mono = theme.mono_font_family.clone();

        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .size(px(28.))
                    .flex_none()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(colors.accent.opacity(0.2))
                    .text_color(colors.accent)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(initial),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_1p5()
                            .min_w_0()
                            .child(
                                div()
                                    .text_color(colors.text)
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(details.author.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(colors.faint)
                                    .child(details.email.clone()),
                            ),
                    )
                    .child(div().text_xs().text_color(colors.muted).child(when)),
            );
        let message = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_color(colors.text)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(subject.to_owned()),
            )
            .when(!body.is_empty(), |message| {
                message.child(
                    div()
                        .id("blame-card-body")
                        .max_h(px(180.))
                        .overflow_y_scroll()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(reflow(body)),
                )
            });
        let actions = div()
            .pt_2()
            .border_t_1()
            .border_color(colors.border)
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .id("blame-card-hash")
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(4.))
                    .font_family(mono)
                    .text_xs()
                    .text_color(colors.muted)
                    .bg(colors.hover)
                    .cursor_pointer()
                    .hover(|hash| hash.text_color(colors.text))
                    .tooltip({
                        let tip: SharedString = strings.blame_copy_hash.into();
                        move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                        }
                    })
                    .on_click({
                        let hash = hash.clone();
                        cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(hash.clone()));
                            this.close_blame_card(cx);
                        })
                    })
                    .child(short),
            )
            .child(div().flex_1())
            .when_some(web, |actions, (site, link)| {
                actions.child(
                    Button::new("blame-card-web")
                        .ghost()
                        .small()
                        .label(format!("{} {site}", strings.blame_open_on))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.open_url(&link);
                            this.close_blame_card(cx);
                        })),
                )
            })
            .child(
                Button::new("blame-card-open")
                    .primary()
                    .small()
                    .label(strings.blame_open_commit)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_blame_card(cx);
                        this.open_git_graph(window, cx);
                        this.select_commit(&hash, window, cx);
                    })),
            );
        Some(
            deferred(
                anchored()
                    .position(card.anchor)
                    .anchor(card.corner)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .id("blame-card")
                            .occlude()
                            .w(px(CARD_WIDTH))
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2p5()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(colors.border)
                            .bg(theme.popover)
                            .shadow_lg()
                            .text_sm()
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                if let Some(card) = &mut this.blame_hover.card {
                                    card.over_card = *hovered;
                                }
                                if *hovered {
                                    this.blame_hover.timer = None;
                                } else {
                                    this.close_blame_card_soon(cx);
                                }
                            }))
                            .child(header)
                            .child(message)
                            .child(actions),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_and_messages() {
        assert_eq!(site_name("https://github.com/me/app"), "GitHub");
        assert_eq!(site_name("https://gitlab.example.com/me/app"), "GitLab");
        assert_eq!(
            site_name("https://git.example.com/me/app"),
            "git.example.com"
        );
        assert_eq!(
            subject_and_body("fix: the drop\n\nIt went to the wrong group.\n"),
            ("fix: the drop", "It went to the wrong group.")
        );
        assert_eq!(subject_and_body("one line"), ("one line", ""));
        assert_eq!(
            reflow(
                "Window and Help menus with the list of\nwindows, and more.\n\n- one\n- two\n  still two\n1. first"
            ),
            "Window and Help menus with the list of windows, and more.\n\n- one\n- two still two\n1. first"
        );
    }
}
