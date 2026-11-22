//! Keel's messages to you. A notice comes in at the bottom right, with its
//! buttons, and goes away by itself after a few seconds (an error stays
//! until closed); with Keel in the background it also goes to macOS's
//! Notification Center. The bell in the status bar keeps the latest ones
//! to read again, and turns on Do Not Disturb. Errors that need a decision
//! open a window that says what went wrong and offers what fixes it.

use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{AnyElement, Div, ElementId, MouseButton, StatefulInteractiveElement as _};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::notification::{Notification, NotificationDelivery, NotificationType};
use gpui_component::{Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, empty_state};
use super::*;

/// How many notices the bell keeps.
const KEPT: usize = 50;
/// The width of the bell's list.
const CENTER_WIDTH: f32 = 380.;

actions!(
    keel,
    [ToggleNotifications, ClearNotifications, ToggleDoNotDisturb]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NoticeKind {
    Info,
    Success,
    Warning,
    Error,
}

impl NoticeKind {
    fn icon(self, cx: &App) -> Icon {
        let theme = cx.theme();
        match self {
            Self::Info => Icon::new(IconName::Info).text_color(theme.info),
            Self::Success => Icon::new(IconName::CircleCheck).text_color(theme.success),
            Self::Warning => Icon::new(IconName::TriangleAlert).text_color(theme.warning),
            Self::Error => Icon::new(IconName::CircleX).text_color(theme.danger),
        }
    }

    fn library(self) -> NotificationType {
        match self {
            Self::Info => NotificationType::Info,
            Self::Success => NotificationType::Success,
            Self::Warning => NotificationType::Warning,
            Self::Error => NotificationType::Error,
        }
    }
}

/// What a notice's button does, on the window it belongs to.
pub(super) type NoticeAction = Rc<dyn Fn(&mut Workbench, &mut Window, &mut Context<Workbench>)>;

#[derive(Clone)]
pub(super) struct Notice {
    pub(super) kind: NoticeKind,
    pub(super) title: String,
    pub(super) message: Option<String>,
    pub(super) actions: Vec<(String, NoticeAction)>,
    /// Runs when the notice goes away, clicked or not.
    pub(super) on_close: Option<NoticeAction>,
    /// When it came, in seconds since 1970.
    pub(super) at: i64,
}

impl Notice {
    fn new(kind: NoticeKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            message: None,
            actions: Vec::new(),
            on_close: None,
            at: now(),
        }
    }

    pub(super) fn info(title: impl Into<String>) -> Self {
        Self::new(NoticeKind::Info, title)
    }

    pub(super) fn success(title: impl Into<String>) -> Self {
        Self::new(NoticeKind::Success, title)
    }

    pub(super) fn warning(title: impl Into<String>) -> Self {
        Self::new(NoticeKind::Warning, title)
    }

    pub(super) fn error(title: impl Into<String>) -> Self {
        Self::new(NoticeKind::Error, title)
    }

    pub(super) fn message(mut self, message: impl Into<String>) -> Self {
        let message = message.into();
        if !message.is_empty() {
            self.message = Some(message);
        }
        self
    }

    pub(super) fn action(
        mut self,
        label: impl Into<String>,
        action: impl Fn(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
    ) -> Self {
        self.actions.push((label.into(), Rc::new(action)));
        self
    }

    pub(super) fn on_close(
        mut self,
        action: impl Fn(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
    ) -> Self {
        self.on_close = Some(Rc::new(action));
        self
    }
}

/// The notices of a window, the latest first.
#[derive(Default)]
pub(super) struct Notices {
    pub(super) list: VecDeque<Notice>,
    /// Notices not seen in the bell's list yet.
    pub(super) unread: usize,
    /// Do Not Disturb: notices go only to the bell, errors aside.
    pub(super) quiet: bool,
    /// The bell's list is open.
    pub(super) open: bool,
}

pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs() as i64)
        .unwrap_or(0)
}

/// `text` with each `{name}` replaced.
pub(super) fn fill(text: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(text.to_owned(), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), value)
    })
}

/// How long ago, in a few words: «adesso», «3 min fa», «ieri», «2 mesi fa».
pub(super) fn ago(at: i64, now: i64, strings: &Strings) -> String {
    let seconds = (now - at).max(0);
    let count = |unit: i64, one: &str, many: &str| {
        let count = (seconds / unit).max(1);
        if count == 1 {
            one.to_owned()
        } else {
            fill(many, &[("count", &count.to_string())])
        }
    };
    const DAY: i64 = 86_400;
    if seconds < 60 {
        strings.notice_now.to_owned()
    } else if seconds < 3600 {
        fill(
            strings.notice_minutes_ago,
            &[("count", &(seconds / 60).to_string())],
        )
    } else if seconds < DAY {
        count(3600, strings.ago_hour_one, strings.ago_hours)
    } else if seconds < 2 * DAY {
        strings.ago_yesterday.to_owned()
    } else if seconds < 7 * DAY {
        count(DAY, strings.ago_yesterday, strings.ago_days)
    } else if seconds < 30 * DAY {
        count(7 * DAY, strings.ago_week_one, strings.ago_weeks)
    } else if seconds < 365 * DAY {
        count(30 * DAY, strings.ago_month_one, strings.ago_months)
    } else {
        count(365 * DAY, strings.ago_year_one, strings.ago_years)
    }
}

impl Workbench {
    /// Shows a notice and keeps it for the bell.
    pub(super) fn notify_user(
        &mut self,
        notice: Notice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.log(match &notice.message {
            Some(message) => format!("{} — {message}", notice.title),
            None => notice.title.clone(),
        });
        self.notices.list.push_front(notice.clone());
        self.notices.list.truncate(KEPT);
        if !self.notices.open {
            self.notices.unread += 1;
        }
        cx.notify();
        let in_background = !window.is_window_active();
        // With the bell's list open, the notice is in it already.
        if self.notices.open {
            return;
        }
        // Do Not Disturb keeps quiet what needs no answer.
        if self.notices.quiet && notice.kind != NoticeKind::Error && notice.actions.is_empty() {
            if let Some(on_close) = notice.on_close.clone() {
                on_close(self, window, cx);
            }
            return;
        }
        let mut note = Notification::new()
            .with_type(notice.kind.library())
            .title(notice.title.clone())
            .autohide(notice.kind != NoticeKind::Error);
        if let Some(message) = notice.message.clone() {
            note = note.message(message);
        }
        if in_background {
            note = note.delivery(NotificationDelivery::InAppAndSystem);
        }
        if let Some(on_close) = notice.on_close.clone() {
            let workbench = cx.weak_entity();
            note = note.on_close(move |window, cx| {
                if let Some(workbench) = workbench.upgrade() {
                    workbench.update(cx, |this, cx| on_close(this, window, cx));
                }
            });
        }
        if !notice.actions.is_empty() {
            let workbench = cx.weak_entity();
            let actions = notice.actions.clone();
            note = note.content(move |_, _, cx| {
                notice_buttons(&actions, workbench.clone(), true, cx).into_any_element()
            });
        }
        // A window without Keel's root, as in some tests, keeps the
        // notice for the bell only.
        if matches!(window.root::<gpui_component::Root>(), Some(Some(_))) {
            window.push_notification(note, cx);
        }
    }

    /// An error that needs a decision, in a window of its own: what went
    /// wrong, git's own words, and the buttons that fix it. The last
    /// button closes it; «Copy the error» puts the text on the clipboard.
    pub(super) fn show_error(
        &mut self,
        title: String,
        explanation: String,
        detail: String,
        fixes: Vec<(String, NoticeAction)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        self.notices.list.push_front(Notice {
            kind: NoticeKind::Error,
            title: title.clone(),
            message: Some(explanation.clone()),
            actions: fixes.clone(),
            on_close: None,
            at: now(),
        });
        self.notices.list.truncate(KEPT);
        self.log(format!("{title}: {detail}"));
        let text = if detail.trim().is_empty() {
            explanation
        } else {
            format!("{explanation}\n\n{}", detail.trim())
        };
        let mut labels: Vec<String> = fixes.iter().map(|(label, _)| label.clone()).collect();
        let show_log = labels.len();
        labels.push(strings.notice_show_log.to_owned());
        let copy = labels.len();
        labels.push(strings.notice_copy_error.to_owned());
        labels.push(strings.notice_close.to_owned());
        let buttons: Vec<&str> = labels.iter().map(String::as_str).collect();
        let answer = window.prompt(PromptLevel::Critical, &title, Some(&text), &buttons, cx);
        let copied = format!("{title}\n{}", detail.trim());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(index) = answer.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                if let Some((_, fix)) = fixes.get(index) {
                    fix(this, window, cx);
                } else if index == show_log {
                    this.output_channel = panel_views::OutputChannel::Keel;
                    this.layout.panel_view = PanelView::Output;
                    this.layout.panel = true;
                    cx.notify();
                } else if index == copy {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied));
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Opens or closes the bell's list. Open, it takes the notices in the
    /// corner with it, so they do not cover it.
    pub(super) fn toggle_notice_center(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.notices.open = !self.notices.open;
        if self.notices.open {
            self.notices.unread = 0;
            if matches!(window.root::<gpui_component::Root>(), Some(Some(_))) {
                window.clear_notifications(cx);
            }
        }
        cx.notify();
    }

    pub(super) fn notice_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &ToggleNotifications, window, cx| {
            this.toggle_notice_center(window, cx)
        }))
        .on_action(cx.listener(|this, _: &ClearNotifications, _, cx| {
            this.notices.list.clear();
            this.notices.unread = 0;
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &ToggleDoNotDisturb, _, cx| {
            this.notices.quiet = !this.notices.quiet;
            cx.notify();
        }))
    }

    /// The bell at the right of the status bar: a dot when something new
    /// came, crossed out with Do Not Disturb.
    pub(super) fn render_bell(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let icon = if self.notices.quiet {
            IconName::BellOff
        } else if self.notices.unread > 0 {
            IconName::BellDot
        } else {
            IconName::Bell
        };
        let tooltip = if self.notices.quiet {
            strings.notice_bell_quiet
        } else {
            strings.notice_bell
        };
        div()
            .id("status-bell")
            .h(px(20.))
            .px(px(6.))
            .rounded(px(5.))
            .flex()
            .items_center()
            .cursor_pointer()
            .hover(|item| item.bg(colors.hover).text_color(colors.text))
            .when(self.notices.open, |item| item.bg(colors.hover))
            .when(self.notices.unread > 0 && !self.notices.quiet, |item| {
                item.text_color(colors.accent)
            })
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx)
            })
            // A press opens it, so the press outside that closes it does not
            // reopen it at once.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_notice_center(window, cx);
                }),
            )
            .child(Icon::new(icon).xsmall())
            .into_any_element()
    }

    /// The bell's list, above the status bar at the right.
    pub(super) fn render_notice_center(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = look::glide(
            "notices-open",
            if self.notices.open { 1. } else { 0. },
            self.look.duration(160),
            window,
            cx,
        );
        if open <= 0. {
            return None;
        }
        let strings = self.strings();
        let now = now();
        let quiet = self.notices.quiet;
        let header = div()
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            .h(px(40.))
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(strings.notice_center_title),
            )
            .child(
                Button::new("notices-quiet")
                    .ghost()
                    .small()
                    .icon(Icon::new(if quiet {
                        IconName::BellOff
                    } else {
                        IconName::Bell
                    }))
                    .when(quiet, |button| button.selected(true))
                    .tooltip(if quiet {
                        strings.notice_quiet_off
                    } else {
                        strings.notice_quiet_on
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notices.quiet = !this.notices.quiet;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("notices-clear")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Eraser))
                    .tooltip(strings.notice_clear)
                    .disabled(self.notices.list.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notices.list.clear();
                        cx.notify();
                    })),
            );
        let body: AnyElement = if self.notices.list.is_empty() {
            div()
                .py_6()
                .child(empty_state(
                    IconName::Bell,
                    strings.notice_none,
                    if quiet {
                        strings.notice_none_quiet_hint
                    } else {
                        strings.notice_none_hint
                    },
                    colors,
                ))
                .into_any_element()
        } else {
            div()
                .id("notices-list")
                .max_h(px(420.))
                .overflow_y_scroll()
                .py_1()
                .children(self.notices.list.iter().enumerate().map(|(index, notice)| {
                    let workbench = cx.weak_entity();
                    div()
                        .id(("notice", index))
                        .mx_1()
                        .px_2()
                        .py_2()
                        .rounded(px(8.))
                        .flex()
                        .gap_2p5()
                        .hover(|row| row.bg(colors.hover))
                        .child(div().pt(px(2.)).child(notice.kind.icon(cx).small()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(notice.title.clone()),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_xs()
                                                .text_color(colors.faint)
                                                .child(ago(notice.at, now, strings)),
                                        ),
                                )
                                .when_some(notice.message.clone(), |column, message| {
                                    column.child(
                                        div().text_xs().text_color(colors.muted).child(message),
                                    )
                                })
                                .when(!notice.actions.is_empty(), |column| {
                                    column.child(notice_buttons(
                                        &notice.actions,
                                        workbench,
                                        false,
                                        cx,
                                    ))
                                }),
                        )
                }))
                .into_any_element()
        };
        let lift = (1. - open) * 8.;
        Some(
            div()
                .id("notice-center")
                .absolute()
                .right(px(12.))
                .bottom(px(self.look.status_height() + 6. - lift))
                .w(px(CENTER_WIDTH))
                .opacity(open)
                .occlude()
                .rounded(px(12.))
                .border_1()
                .border_color(colors.border)
                .bg(cx.theme().popover)
                .shadow_lg()
                .overflow_hidden()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if this.notices.open {
                        this.notices.open = false;
                        cx.notify();
                    }
                }))
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .child(header)
                .child(body)
                .into_any_element(),
        )
    }
}

/// A notice's buttons, in a row under its text.
/// In a toast the first button stands out; in the bell's list they are
/// all quiet, so the list reads as a list.
fn notice_buttons(
    actions: &[(String, NoticeAction)],
    workbench: gpui::WeakEntity<Workbench>,
    toast: bool,
    cx: &App,
) -> Div {
    let _ = cx;
    div()
        .pt_1p5()
        .flex()
        .flex_wrap()
        .gap_1p5()
        .children(actions.iter().enumerate().map(|(index, (label, action))| {
            let action = action.clone();
            let workbench = workbench.clone();
            Button::new(ElementId::NamedInteger(
                "notice-action".into(),
                index as u64,
            ))
            .label(label.clone())
            .small()
            .when(toast && index == 0, |button| button.primary())
            .when(!toast || index > 0, |button| button.outline())
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                if let Some(workbench) = workbench.upgrade() {
                    workbench.update(cx, |this, cx| action(this, window, cx));
                }
            })
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{ENGLISH, ITALIAN};

    #[test]
    fn placeholders_are_filled() {
        assert_eq!(
            fill(
                "Push di {count} commit su {remote}",
                &[("count", "3"), ("remote", "origin")]
            ),
            "Push di 3 commit su origin"
        );
    }

    #[test]
    fn times_read_as_words() {
        assert_eq!(ago(1000, 1030, &ITALIAN), "adesso");
        assert_eq!(ago(1000, 1000 + 180, &ITALIAN), "3 min fa");
        assert_eq!(ago(1000, 1000 + 180, &ENGLISH), "3 min ago");
        assert_eq!(ago(0, 2 * 3600, &ITALIAN), "2 ore fa");
        assert_eq!(ago(0, 30 * 3600, &ITALIAN), "ieri");
        assert_eq!(ago(0, 3 * 86_400, &ENGLISH), "3 days ago");
        assert_eq!(ago(0, 70 * 86_400, &ITALIAN), "2 mesi fa");
    }
}
