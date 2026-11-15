//! A quick pick: a searchable list in a dialog, used by the command palette
//! and by extensions through `vscode.window.showQuickPick`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{App, AppContext as _, ParentElement as _, Window, px};
use gpui_component::WindowExt as _;
use gpui_component::command::{Command, CommandItem, CommandState};

/// One entry of the list.
#[derive(Clone, Debug)]
pub struct PickItem {
    pub label: String,
    /// Extra words the search also matches, such as a description.
    pub keywords: Vec<String>,
}

impl PickItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            keywords: Vec::new(),
        }
    }
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
    let (sender, receiver) = oneshot::channel();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let finish = Rc::new(move |choice: Option<usize>| {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(choice);
        }
    });
    let state = cx.new(|cx| CommandState::new(window, cx));
    let first_render = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if first_render.replace(false) {
            state.update(cx, |state, cx| state.focus(window, cx));
        }
        let confirm = finish.clone();
        let cancel = finish.clone();
        let close = finish.clone();
        let command = Command::new(&state)
            .items(items.iter().map(|item| {
                CommandItem::new()
                    .label(item.label.clone())
                    .keywords(item.keywords.clone())
            }))
            .placeholder(placeholder.clone().unwrap_or_default())
            .on_confirm(move |index, window, cx| {
                confirm(Some(index.row));
                window.close_dialog(cx);
            })
            // The dialog closes itself after a cancel.
            .on_cancel(move |_, _| cancel(None));
        let dialog = dialog
            .w(px(560.))
            .on_close(move |_, _, _| close(None))
            .child(command);
        match &title {
            Some(title) => dialog.title(title.clone()),
            None => dialog,
        }
    });
    receiver
}
