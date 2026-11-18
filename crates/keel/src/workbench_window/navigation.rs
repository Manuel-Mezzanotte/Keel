//! Quick Open (Cmd+P) over the project's files, and Go to Line (Ctrl+G).

use std::sync::Arc;

use gpui_component::input::{Input, InputState};
use gpui_component::{Sizable as _, WindowExt as _};

use super::*;
use crate::quick_pick::{Matching, split_line};

actions!(keel, [QuickOpen, GoToLine]);

impl Workbench {
    /// Lists the project's files in the background, for Quick Open. Runs
    /// when the window opens and each time it comes back to the front.
    pub(super) fn index_files(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let excludes = super::explorer::excludes(&self.settings);
        let listing = cx
            .background_executor()
            .spawn(async move { files::list_files(&root, &excludes) });
        self.indexing = cx.spawn(async move |this, cx| {
            let files = Arc::new(listing.await);
            this.update(cx, |this, _| this.file_index = Some(files))
                .ok();
        });
    }

    /// Recently used files first, then the rest of the project, as
    /// relative paths.
    pub(super) fn quick_open_paths(&self) -> Vec<String> {
        let Some(root) = &self.project.root else {
            return Vec::new();
        };
        let relative = |uri: &str| {
            let path = Uri::parse(uri)?.file_path()?;
            Some(path.strip_prefix(root).ok()?.to_string_lossy().into_owned())
        };
        let mut recent: Vec<String> = Vec::new();
        let active = self.panes.get(self.active).map(|pane| pane.uri.clone());
        let tabs = self
            .groups
            .iter()
            .flat_map(|group| &group.tabs)
            .map(|tab| tab.uri.clone());
        // The file in view last, since you are already looking at it.
        let used = self
            .closed
            .iter()
            .rev()
            .cloned()
            .chain(tabs)
            .filter(|uri| Some(uri) != active.as_ref())
            .chain(active.clone());
        for uri in used {
            if let Some(path) = relative(&uri)
                && !recent.contains(&path)
            {
                recent.push(path);
            }
        }
        let rest = self.file_index.iter().flat_map(|index| index.iter());
        let mut paths = recent.clone();
        paths.extend(rest.filter(|path| !recent.contains(path)).cloned());
        paths
    }

    pub(super) fn quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.root.clone() else {
            self.show_commands(&ShowCommands, window, cx);
            return;
        };
        let paths = self.quick_open_paths();
        let items = paths
            .iter()
            .map(|path| {
                let (folder, name) = match path.rsplit_once('/') {
                    Some((folder, name)) => (Some(folder.to_owned()), name.to_owned()),
                    None => (None, path.clone()),
                };
                let mut item = PickItem::new(name);
                item.detail = folder;
                item.keywords.push(path.clone());
                item
            })
            .collect();
        let placeholder = Some(self.strings().quick_open_placeholder.to_owned());
        // Each file with its icon, as in the Explorer.
        let workbench = cx.entity().downgrade();
        let icon: quick_pick::IconFor = Rc::new(move |item, cx| {
            let path = item.keywords.first()?;
            let workbench = workbench.upgrade()?;
            Some(workbench.read(cx).file_icon_element(Path::new(path), cx))
        });
        let chosen = quick_pick::show_with(
            None,
            placeholder,
            items,
            Matching::Path,
            Some(icon),
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(choice)) = chosen.await else {
                return;
            };
            let path = root.join(&paths[choice.index]);
            let (_, line) = split_line(&choice.query);
            this.update_in(cx, |this, window, cx| {
                this.open_file(&path, window, cx);
                if let Some((line, column)) = line {
                    this.go_to(line, column.unwrap_or(1), window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Puts the cursor on a line and column, counted from 1, in the
    /// active editor.
    pub(super) fn go_to(
        &mut self,
        line: u32,
        column: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        editor.update(cx, |editor, cx| {
            let position = gpui_component::input::Position::new(
                line.saturating_sub(1),
                column.saturating_sub(1),
            );
            editor.set_cursor_position(position, window, cx);
            editor.focus(window, cx);
        });
        self.publish_selection_if_changed(self.active, cx);
    }

    /// Asks for a line, and a column after a colon, then goes there.
    pub(super) fn ask_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let (current, total) = editor.read_with(cx, |editor, _| {
            let current = editor.cursor_position().line as usize + 1;
            (current, editor.value().lines().count().max(1))
        });
        let placeholder = self.strings().go_to_line_placeholder(current, total);
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let this = cx.weak_entity();
        let field = input.clone();
        // The dialog takes Enter as its OK, which reads the field.
        window.open_dialog(cx, move |dialog, _, _| {
            let this = this.clone();
            let shown = field.clone();
            let field = field.clone();
            dialog
                .w(px(420.))
                .on_ok(move |_, window, cx| {
                    let typed = field.read(cx).value().to_string();
                    let typed = format!(":{}", typed.trim().trim_start_matches(':'));
                    if let (_, Some((line, column))) = split_line(&typed) {
                        this.update(cx, |this, cx| {
                            this.go_to(line, column.unwrap_or(1), window, cx)
                        })
                        .ok();
                    }
                    true
                })
                .child(Input::new(&shown).small())
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    /// A project in a window with gpui-component's Root, as dialogs need.
    fn project(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("Hospitality");
        fs::create_dir_all(root.join("src/components")).unwrap();
        fs::create_dir_all(root.join("src/lib")).unwrap();
        fs::write(
            root.join("src/components/booking-card.tsx"),
            "line one\nline two\nline three\n",
        )
        .unwrap();
        fs::write(root.join("src/lib/booking.ts"), "").unwrap();
        fs::write(root.join("README.md"), "a\nb\nc\nd\ne\n").unwrap();
        let settings_file = base.join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), Some(root.join("README.md")));
        let mut workbench = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Workbench::new(opening, settings, None, window, cx));
            workbench = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        cx.run_until_parked();
        (folder, root, workbench.unwrap(), cx)
    }

    fn cursor(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> (u32, u32) {
        view.read_with(cx, |view, cx| {
            let position = view.panes[view.active].editor.read(cx).cursor_position();
            (position.line, position.character)
        })
    }

    #[gpui::test]
    fn cmd_p_finds_files_by_their_letters_and_goes_to_a_line(cx: &mut TestAppContext) {
        let (_folder, _root, view, cx) = project(cx);
        let paths = view.read_with(cx, |view, _| view.quick_open_paths());
        assert_eq!(
            paths,
            [
                "README.md",
                "src/components/booking-card.tsx",
                "src/lib/booking.ts"
            ]
        );

        cx.simulate_keystrokes("cmd-p");
        cx.run_until_parked();
        cx.simulate_input("bkcard:2");
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let title = view.read_with(cx, |view, _| view.window_title());
        assert_eq!(title, "booking-card.tsx — Hospitality");
        assert_eq!(cursor(&view, cx), (1, 0));
    }

    #[gpui::test]
    fn ctrl_g_goes_to_a_line_and_column(cx: &mut TestAppContext) {
        let (_folder, _root, view, cx) = project(cx);
        cx.simulate_keystrokes("ctrl-g");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        cx.simulate_input("4:1");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(cursor(&view, cx), (3, 0));
    }
}
