//! A quick pick: a searchable list in a dialog, used by Quick Open, the
//! command palette and by extensions through `vscode.window.showQuickPick`.
//! Keel filters the list itself with fuzzy matching, as VS Code does.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use files::fuzzy;
use futures::channel::oneshot;
use gpui::{AnyElement, App, AppContext as _, ParentElement as _, Styled as _, Window, div, px};
use gpui_component::command::{Command, CommandItem, CommandState};
use gpui_component::{ActiveTheme as _, WindowExt as _};

/// The most entries shown at once; typing narrows the rest down.
const SHOWN: usize = 200;

/// One entry of the list.
#[derive(Clone, Debug)]
pub struct PickItem {
    pub label: String,
    /// Shown dimmed after the label, such as a file's folder.
    pub detail: Option<String>,
    /// Extra words the search also matches, such as a command's id.
    pub keywords: Vec<String>,
}

impl PickItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: None,
            keywords: Vec::new(),
        }
    }
}

/// Draws the icon in front of an entry, such as a file's. Called only for
/// the entries on screen, so a long list costs nothing more to open.
pub type IconFor = Rc<dyn Fn(&PickItem, &App) -> Option<AnyElement>>;

/// How entries are matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matching {
    /// Commands and other text: the label, then the keywords.
    Text,
    /// Files: the first keyword is the path, and the file name counts most.
    Path,
}

/// The entry the user chose, and what they typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub index: usize,
    pub query: String,
}

/// Opens the list. The receiver gets the index of the chosen entry, or
/// `None` when the user closes the dialog.
pub fn show(
    title: Option<String>,
    placeholder: Option<String>,
    items: Vec<PickItem>,
    window: &mut Window,
    cx: &mut App,
) -> oneshot::Receiver<Option<usize>> {
    let chosen = show_with(title, placeholder, items, Matching::Text, None, window, cx);
    let (sender, receiver) = oneshot::channel();
    cx.spawn(async move |_| {
        let choice = chosen.await.ok().flatten().map(|choice| choice.index);
        let _ = sender.send(choice);
    })
    .detach();
    receiver
}

/// Opens the list and also hands back the typed query, which Quick Open
/// reads for a `:line` at its end.
pub fn show_with(
    title: Option<String>,
    placeholder: Option<String>,
    items: Vec<PickItem>,
    matching: Matching,
    icon: Option<IconFor>,
    window: &mut Window,
    cx: &mut App,
) -> oneshot::Receiver<Option<Choice>> {
    let (sender, receiver) = oneshot::channel();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let finish = Rc::new(move |choice: Option<Choice>| {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(choice);
        }
    });
    let state = cx.new(|cx| CommandState::new(window, cx));
    let first_render = Cell::new(true);
    let items = Rc::new(items);
    let shown = Rc::new(RefCell::new(rank(&items, "", matching)));
    let query = Rc::new(RefCell::new(String::new()));

    window.open_dialog(cx, move |dialog, window, cx| {
        if first_render.replace(false) {
            state.update(cx, |state, cx| state.focus(window, cx));
        }
        let confirm = finish.clone();
        let cancel = finish.clone();
        let close = finish.clone();
        let confirm_shown = shown.clone();
        let confirm_query = query.clone();
        let query_items = items.clone();
        let query_shown = shown.clone();
        let query_text = query.clone();
        let rows: Vec<CommandItem> = shown
            .borrow()
            .iter()
            .map(|&index| row(&items[index], icon.as_ref()))
            .collect();
        let command = Command::new(&state)
            .filterable(false)
            .items(rows)
            .placeholder(placeholder.clone().unwrap_or_default())
            .on_query(move |typed, window, _| {
                *query_shown.borrow_mut() = rank(&query_items, typed, matching);
                *query_text.borrow_mut() = typed.to_owned();
                window.refresh();
            })
            .on_confirm(move |index, window, cx| {
                let chosen = confirm_shown.borrow().get(index.row).copied();
                if let Some(index) = chosen {
                    confirm(Some(Choice {
                        index,
                        query: confirm_query.borrow().clone(),
                    }));
                }
                window.close_dialog(cx);
            })
            // The dialog closes itself after a cancel.
            .on_cancel(move |_, _| cancel(None));
        let dialog = dialog
            .w(px(600.))
            .on_close(move |_, _, _| close(None))
            .child(command);
        match &title {
            Some(title) => dialog.title(title.clone()),
            None => dialog,
        }
    });
    receiver
}

/// Fetches the entries for what was typed, such as the project's symbols
/// from a language server.
pub type Fetch = Rc<dyn Fn(String, &mut App) -> gpui::Task<Vec<PickItem>>>;

/// Opens a list whose entries come from `fetch` as the user types; the
/// receiver gets the chosen entry itself.
pub fn show_dynamic(
    placeholder: Option<String>,
    fetch: Fetch,
    window: &mut Window,
    cx: &mut App,
) -> oneshot::Receiver<Option<PickItem>> {
    let (sender, receiver) = oneshot::channel();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let finish = Rc::new(move |choice: Option<PickItem>| {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(choice);
        }
    });
    let state = cx.new(|cx| CommandState::new(window, cx));
    let first_render = Cell::new(true);
    let items: Rc<RefCell<Vec<PickItem>>> = Rc::default();
    // The latest request; an older one still running is dropped with it.
    let pending: Rc<RefCell<Option<gpui::Task<()>>>> = Rc::default();
    let handle = window.window_handle();
    let request = {
        let items = items.clone();
        let pending = pending.clone();
        Rc::new(move |typed: String, cx: &mut App| {
            let task = fetch(typed, cx);
            let items = items.clone();
            let task = cx.spawn(async move |cx| {
                let found = task.await;
                *items.borrow_mut() = found;
                handle.update(cx, |_, window, _| window.refresh()).ok();
            });
            *pending.borrow_mut() = Some(task);
        })
    };
    request(String::new(), cx);

    window.open_dialog(cx, move |dialog, window, cx| {
        if first_render.replace(false) {
            state.update(cx, |state, cx| state.focus(window, cx));
        }
        let confirm = finish.clone();
        let cancel = finish.clone();
        let close = finish.clone();
        let chosen_items = items.clone();
        let request = request.clone();
        let rows: Vec<CommandItem> = items
            .borrow()
            .iter()
            .take(SHOWN)
            .map(|item| row(item, None))
            .collect();
        let command = Command::new(&state)
            .filterable(false)
            .items(rows)
            .placeholder(placeholder.clone().unwrap_or_default())
            .on_query(move |typed, _, cx| request(typed.to_owned(), cx))
            .on_confirm(move |index, window, cx| {
                let chosen = chosen_items.borrow().get(index.row).cloned();
                if chosen.is_some() {
                    confirm(chosen);
                }
                window.close_dialog(cx);
            })
            .on_cancel(move |_, _| cancel(None));
        dialog
            .w(px(600.))
            .on_close(move |_, _, _| close(None))
            .child(command)
    });
    receiver
}

fn row(item: &PickItem, icon: Option<&IconFor>) -> CommandItem {
    let entry = CommandItem::new()
        .label(item.label.clone())
        .keywords(item.keywords.clone());
    if item.detail.is_none() && icon.is_none() {
        return entry;
    }
    let shown = item.clone();
    let icon = icon.cloned();
    entry.child(move |_, cx| {
        let picture = icon.as_ref().and_then(|icon| icon(&shown, cx));
        let theme = cx.theme();
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .children(picture)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .min_w_0()
                    .child(div().flex_none().child(shown.label.clone()))
                    .children(shown.detail.clone().map(|detail| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(detail)
                    })),
            )
    })
}

/// The entries matching a query, best first, at most [`SHOWN`].
fn rank(items: &[PickItem], query: &str, matching: Matching) -> Vec<usize> {
    let (query, _) = split_line(query);
    let query = query.trim();
    if query.is_empty() {
        return (0..items.len().min(SHOWN)).collect();
    }
    let mut scored: Vec<(i32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| score(item, query, matching).map(|score| (score, index)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(SHOWN)
        .map(|(_, index)| index)
        .collect()
}

fn score(item: &PickItem, query: &str, matching: Matching) -> Option<i32> {
    match matching {
        Matching::Path => {
            let path = item.keywords.first().unwrap_or(&item.label);
            fuzzy::score_path(query, path)
        }
        Matching::Text => {
            let label = fuzzy::score(query, &item.label).map(|score| score + 10);
            let keywords = item
                .keywords
                .iter()
                .filter_map(|keyword| fuzzy::score(query, keyword))
                .max();
            label.max(keywords)
        }
    }
}

/// Splits `app.ts:42:5` into the search and the line and column to go to.
pub fn split_line(query: &str) -> (&str, Option<(u32, Option<u32>)>) {
    let number = |text: &str| -> Option<u32> {
        (!text.is_empty() && text.chars().all(|character| character.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    };
    let mut parts = query.rsplitn(3, ':');
    let last = parts.next().unwrap_or_default();
    let middle = parts.next();
    let rest = parts.next();
    match (number(last), middle.and_then(number), middle, rest) {
        (Some(column), Some(line), _, Some(rest)) => (rest, Some((line, Some(column)))),
        (Some(line), _, Some(_), _) => {
            let base = &query[..query.len() - last.len() - 1];
            (base, Some((line, None)))
        }
        _ => (query, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Vec<PickItem> {
        paths
            .iter()
            .map(|path| {
                let mut item = PickItem::new(path.rsplit('/').next().unwrap());
                item.keywords.push(path.to_string());
                item
            })
            .collect()
    }

    #[test]
    fn lines_are_read_from_the_end_of_the_query() {
        assert_eq!(split_line("card.tsx:42"), ("card.tsx", Some((42, None))));
        assert_eq!(
            split_line("card.tsx:42:7"),
            ("card.tsx", Some((42, Some(7))))
        );
        assert_eq!(split_line(":12"), ("", Some((12, None))));
        assert_eq!(split_line("File: Save"), ("File: Save", None));
        assert_eq!(split_line("card"), ("card", None));
    }

    #[test]
    fn files_rank_by_name_and_commands_by_label() {
        let items = files(&[
            "src/lib/booking.ts",
            "src/components/booking-card.tsx",
            "README.md",
        ]);
        let shown = rank(&items, "bkcard:3", Matching::Path);
        assert_eq!(shown, [1]);
        assert_eq!(rank(&items, "", Matching::Path), [0, 1, 2]);

        let commands = vec![
            PickItem::new("File: Save"),
            PickItem::new("View: Toggle Sidebar"),
            PickItem::new("Preferences: Color Theme"),
        ];
        assert_eq!(rank(&commands, "colth", Matching::Text)[0], 2);
    }
}
