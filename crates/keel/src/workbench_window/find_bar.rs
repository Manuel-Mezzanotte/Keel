//! Find and replace in the open file (Cmd+F, Cmd+Alt+F), as VS Code's find
//! widget: case, whole word and regular expressions, the current match out
//! of all of them, and `$1` in replacements with a regular expression. The
//! editor highlights the matches; this bar sits on top of it.

use gpui::{AnimationExt as _, AnyElement, Subscription, prelude::FluentBuilder as _};
use gpui_base::input::SearchOptions;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, Replace, Search};
use gpui_component::{Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::Colors;
use super::*;

actions!(
    keel,
    [
        FindNext,
        FindPrevious,
        CloseFind,
        ToggleFindCase,
        ToggleFindWholeWord,
        ToggleFindRegex
    ]
);

/// The key context of the find bar's fields.
pub const FIND_CONTEXT: &str = "KeelFind";

/// The keys of the bar, as VS Code's on the Mac.
pub(crate) fn bindings() -> Vec<gpui::KeyBinding> {
    vec![
        gpui::KeyBinding::new("escape", CloseFind, Some(FIND_CONTEXT)),
        gpui::KeyBinding::new("cmd-g", FindNext, None),
        gpui::KeyBinding::new("cmd-shift-g", FindPrevious, None),
        gpui::KeyBinding::new("cmd-alt-c", ToggleFindCase, Some(FIND_CONTEXT)),
        gpui::KeyBinding::new("cmd-alt-w", ToggleFindWholeWord, Some(FIND_CONTEXT)),
        gpui::KeyBinding::new("cmd-alt-r", ToggleFindRegex, Some(FIND_CONTEXT)),
    ]
}

pub(super) struct FindBar {
    pub(super) open: bool,
    pub(super) query: Entity<InputState>,
    pub(super) replace: Entity<InputState>,
    pub(super) show_replace: bool,
    pub(super) options: SearchOptions,
    _subscriptions: Vec<Subscription>,
}

impl Workbench {
    pub(super) fn create_find_bar(
        window: &mut Window,
        cx: &mut Context<Self>,
        strings: &Strings,
    ) -> FindBar {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.search_placeholder));
        let replace =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.replace_placeholder));
        let subscriptions = vec![
            cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                match event {
                    // Enter also says "changed": search again only when
                    // the text did.
                    InputEvent::Change => {
                        let query = this.find.query.read(cx).value().to_string();
                        let searched = this
                            .active_editor()
                            .map(|editor| editor.read(cx).search_session().query.clone());
                        if searched.as_deref() != Some(query.as_str()) {
                            this.run_find(true, cx);
                        }
                    }
                    InputEvent::PressEnter { shift, .. } => {
                        this.find_step(!shift, window, cx);
                    }
                    _ => {}
                }
            }),
            cx.subscribe_in(
                &replace,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.replace_current(window, cx);
                    }
                },
            ),
        ];
        FindBar {
            open: false,
            query,
            replace,
            show_replace: false,
            options: SearchOptions::default(),
            _subscriptions: subscriptions,
        }
    }

    /// Opens the bar on the active editor, with the selected text as the
    /// query when it is on one line, as VS Code does.
    pub(super) fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let selected = editor.read_with(cx, |editor, _| {
            let range = editor.selected_range();
            let text = editor.value().to_string();
            text.get(range).map(str::to_owned).unwrap_or_default()
        });
        if !selected.is_empty() && !selected.contains('\n') {
            self.find
                .query
                .update(cx, |query, cx| query.set_value(selected, window, cx));
        }
        self.find.open = true;
        self.find.show_replace |= replace;
        let field = if replace && !self.find.query.read(cx).value().is_empty() {
            self.find.replace.clone()
        } else {
            self.find.query.clone()
        };
        field.update(cx, |field, cx| {
            field.focus(window, cx);
            field.select_all(window, cx);
        });
        self.run_find(true, cx);
        cx.notify();
    }

    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find.open {
            return;
        }
        self.find.open = false;
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |editor, cx| {
                editor.close_search(cx);
                editor.focus(window, cx);
            });
        }
        cx.notify();
    }

    /// Searches the active editor for the query; `select` moves the
    /// selection to the current match, as typing does.
    pub(super) fn run_find(&mut self, select: bool, cx: &mut Context<Self>) {
        if !self.find.open {
            return;
        }
        let Some(editor) = self.active_editor() else {
            return;
        };
        let query = self.find.query.read(cx).value().to_string();
        let options = self.find.options;
        editor.update(cx, |editor, cx| {
            editor.set_search_query_with(query, options, cx);
            if select {
                let matcher = &editor.search_session().matcher;
                if let Some(range) = matcher
                    .current()
                    .and_then(|index| matcher.matched_ranges().get(index).cloned())
                {
                    editor.set_selected_range(range, cx);
                }
            }
        });
        cx.notify();
    }

    /// Enter and Shift+Enter, Cmd+G and Cmd+Shift+G.
    pub(super) fn find_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find.open {
            self.open_find(false, window, cx);
            return;
        }
        let Some(editor) = self.active_editor() else {
            return;
        };
        // Another tab since the last search: search it first.
        let query = self.find.query.read(cx).value().to_string();
        let current = editor.read_with(cx, |editor, _| {
            let session = editor.search_session();
            session.is_active() && session.query == query && session.options == self.find.options
        });
        if !current {
            self.run_find(true, cx);
            return;
        }
        editor.update(cx, |editor, cx| {
            let found = if forward {
                editor.next_search_match(cx)
            } else {
                editor.previous_search_match(cx)
            };
            if let Some(range) = found {
                editor.set_selected_range(range, cx);
            }
        });
        cx.notify();
    }

    fn replace_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let replacement = self.find.replace.read(cx).value().to_string();
        editor.update(cx, |editor, cx| {
            editor.replace_current_search_match(&replacement, window, cx);
        });
        self.run_find(true, cx);
    }

    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let replacement = self.find.replace.read(cx).value().to_string();
        let count = editor.update(cx, |editor, cx| {
            editor.replace_all_search_matches(&replacement, window, cx)
        });
        self.status_message = Some(self.strings().replaced_in_file(count));
        self.run_find(false, cx);
    }

    fn toggle_find_option(
        &mut self,
        option: fn(&mut SearchOptions) -> &mut bool,
        cx: &mut Context<Self>,
    ) {
        let value = option(&mut self.find.options);
        *value = !*value;
        self.run_find(true, cx);
    }

    /// What the bar says about the matches: `2/5`, none, or why the regular
    /// expression is wrong.
    fn find_summary(&self, cx: &App) -> (String, bool) {
        let strings = self.strings();
        let Some(editor) = self.active_editor() else {
            return (String::new(), false);
        };
        let matcher = &editor.read(cx).search_session().matcher;
        if let Some(error) = matcher.pattern_error() {
            let short = error.lines().last().unwrap_or(error).trim().to_owned();
            return (short, true);
        }
        if self.find.query.read(cx).value().is_empty() {
            return (String::new(), false);
        }
        if matcher.is_empty() {
            return (strings.no_results.to_owned(), true);
        }
        (matcher.label(), false)
    }

    pub(super) fn find_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &Search, window, cx| this.open_find(false, window, cx)),
        )
        .on_action(cx.listener(|this, _: &Replace, window, cx| this.open_find(true, window, cx)))
        .on_action(cx.listener(|this, _: &FindNext, window, cx| this.find_step(true, window, cx)))
        .on_action(
            cx.listener(|this, _: &FindPrevious, window, cx| this.find_step(false, window, cx)),
        )
        .on_action(cx.listener(|this, _: &CloseFind, window, cx| this.close_find(window, cx)))
        .on_action(cx.listener(|this, _: &ToggleFindCase, _, cx| {
            this.toggle_find_option(|options| &mut options.case_sensitive, cx)
        }))
        .on_action(cx.listener(|this, _: &ToggleFindWholeWord, _, cx| {
            this.toggle_find_option(|options| &mut options.whole_word, cx)
        }))
        .on_action(cx.listener(|this, _: &ToggleFindRegex, _, cx| {
            this.toggle_find_option(|options| &mut options.regex, cx)
        }))
    }

    /// The bar, at the top right of the active editor.
    pub(super) fn render_find_bar(
        &self,
        index: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.find.open || index != self.active {
            return None;
        }
        let strings = self.strings();
        let options = self.find.options;
        let switch = |id: &'static str, icon: IconName, on: bool, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .selected(on)
                .tooltip(tooltip)
        };
        let switches = div()
            .flex()
            .items_center()
            .child(
                switch(
                    "find-case",
                    IconName::CaseSensitive,
                    options.case_sensitive,
                    strings.match_case,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_find_option(|options| &mut options.case_sensitive, cx)
                })),
            )
            .child(
                switch(
                    "find-word",
                    IconName::WholeWord,
                    options.whole_word,
                    strings.whole_word,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_find_option(|options| &mut options.whole_word, cx)
                })),
            )
            .child(
                switch(
                    "find-regex",
                    IconName::Regex,
                    options.regex,
                    strings.use_regex,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_find_option(|options| &mut options.regex, cx)
                })),
            );
        let (summary, warning) = self.find_summary(cx);
        let button = |id: &'static str, icon: IconName, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tooltip)
        };
        let bar = div()
            .absolute()
            .top_1()
            .right_4()
            .w(px(420.))
            .p_1()
            .flex()
            .gap_0p5()
            .items_start()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(cx.theme().popover)
            .shadow_md()
            .key_context(FIND_CONTEXT)
            .child(
                button(
                    "find-toggle-replace",
                    if self.find.show_replace {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                    strings.toggle_replace,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.find.show_replace = !this.find.show_replace;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&self.find.query).small().suffix(switches)),
                            )
                            .child(
                                div()
                                    .min_w(px(64.))
                                    .text_xs()
                                    .text_color(if warning {
                                        cx.theme().danger
                                    } else {
                                        colors.muted
                                    })
                                    .truncate()
                                    .child(summary),
                            )
                            .child(
                                button("find-previous", IconName::ArrowUp, strings.find_previous)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.find_step(false, window, cx)
                                    })),
                            )
                            .child(
                                button("find-next", IconName::ArrowDown, strings.find_next)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.find_step(true, window, cx)
                                    })),
                            )
                            .child(
                                button("find-close", IconName::Close, strings.close_find).on_click(
                                    cx.listener(|this, _, window, cx| this.close_find(window, cx)),
                                ),
                            ),
                    )
                    .when(self.find.show_replace, |fields| {
                        fields.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&self.find.replace).small()),
                                )
                                .child(
                                    button("find-replace", IconName::Replace, strings.replace_one)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.replace_current(window, cx)
                                        })),
                                )
                                .child(
                                    button(
                                        "find-replace-all",
                                        IconName::ReplaceAll,
                                        strings.replace_all,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.replace_all(window, cx),
                                    )),
                                ),
                        )
                    }),
            );
        // It slides in like gpui-component's menus, unless macOS asks for
        // less motion.
        if crate::motion::reduced() {
            return Some(bar.into_any_element());
        }
        Some(
            bar.with_animation(
                "find-bar",
                gpui::Animation::new(std::time::Duration::from_millis(150))
                    .with_easing(gpui::ease_out_quint()),
                |bar, delta| bar.opacity(delta).mt(px(-6. * (1. - delta))),
            )
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn summary(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| view.find_summary(cx).0)
    }

    fn text(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.active_editor().unwrap().read(cx).value().to_string()
        })
    }

    #[gpui::test]
    fn find_with_case_whole_words_and_regular_expressions(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) =
            super::super::tests::window_with_file(cx, "notes.txt", "one two One\none_two\n");
        let _ = path;
        cx.simulate_keystrokes("cmd-f");
        assert!(view.read_with(cx, |view, _| view.find.open));
        cx.simulate_input("one");
        assert_eq!(summary(&view, cx), "1/3");
        // The first match is selected, as VS Code does while typing.
        view.read_with(cx, |view, cx| {
            let editor = view.active_editor().unwrap().read(cx);
            assert_eq!(editor.selected_range(), 0..3);
        });
        cx.simulate_keystrokes("cmd-g");
        cx.simulate_keystrokes("enter");
        assert_eq!(summary(&view, cx), "3/3");
        cx.simulate_keystrokes("cmd-alt-c");
        assert_eq!(summary(&view, cx), "1/2");
        cx.simulate_keystrokes("cmd-alt-w");
        assert_eq!(summary(&view, cx), "1/1");
        cx.simulate_keystrokes("cmd-alt-w cmd-alt-c");

        // A regular expression with a group, replaced everywhere with `$1`.
        view.update_in(cx, |view, window, cx| {
            view.find
                .query
                .update(cx, |query, cx| query.set_value("(\\w+)_two", window, cx));
            view.find
                .replace
                .update(cx, |field, cx| field.set_value("$1-due", window, cx));
        });
        cx.simulate_keystrokes("cmd-alt-r");
        assert_eq!(summary(&view, cx), "1/1");
        view.update_in(cx, |view, window, cx| view.replace_all(window, cx));
        assert_eq!(text(&view, cx), "one two One\none-due\n");

        // A broken expression says why.
        view.update_in(cx, |view, window, cx| {
            view.find
                .query
                .update(cx, |query, cx| query.set_value("(", window, cx));
            view.run_find(true, cx);
        });
        assert!(
            summary(&view, cx).contains("unclosed"),
            "{}",
            summary(&view, cx)
        );

        cx.simulate_keystrokes("escape");
        assert!(!view.read_with(cx, |view, _| view.find.open));
    }
}
