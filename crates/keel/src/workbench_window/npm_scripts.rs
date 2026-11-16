//! The scripts of the project's `package.json`, as VS Code's NPM Scripts
//! view lists them: under the Explorer, and run in a terminal with the
//! project's package manager.

use std::path::Path;

use gpui::{ClickEvent, Div, StatefulInteractiveElement as _};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, heading};
use super::terminal_view;
use super::*;

actions!(keel, [RunNpmScript]);

/// The scripts of a `package.json` and how to run them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NpmScripts {
    /// `npm`, `pnpm`, `yarn` or `bun`, from the lock file in the folder.
    pub manager: &'static str,
    /// Each script's name and command, in the file's order.
    pub scripts: Vec<(String, String)>,
}

impl NpmScripts {
    /// Reads the scripts of the `package.json` in `folder`, if there is one
    /// with scripts.
    pub(super) fn read(folder: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(folder.join("package.json")).ok()?;
        let json: Value = serde_json::from_str(&text).ok()?;
        let scripts: Vec<(String, String)> = json
            .get("scripts")?
            .as_object()?
            .iter()
            .filter_map(|(name, command)| Some((name.clone(), command.as_str()?.to_owned())))
            .collect();
        if scripts.is_empty() {
            return None;
        }
        Some(Self {
            manager: package_manager(folder, &json),
            scripts,
        })
    }

    /// The command line that runs a script.
    pub(super) fn command(&self, script: &str) -> String {
        let quoted = if script
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || "-_:.@/".contains(char))
        {
            script.to_owned()
        } else {
            format!("'{}'", script.replace('\'', r"'\''"))
        };
        format!("{} run {quoted}", self.manager)
    }
}

/// The package manager a project uses: the `packageManager` field, then
/// the lock file, as VS Code's `npm.packageManager: auto` does.
fn package_manager(folder: &Path, json: &Value) -> &'static str {
    if let Some(declared) = json.get("packageManager").and_then(Value::as_str) {
        for manager in ["pnpm", "yarn", "bun", "npm"] {
            if declared.starts_with(manager) {
                return manager;
            }
        }
    }
    let has = |file: &str| folder.join(file).is_file();
    if has("pnpm-lock.yaml") {
        "pnpm"
    } else if has("yarn.lock") {
        "yarn"
    } else if has("bun.lockb") || has("bun.lock") {
        "bun"
    } else {
        "npm"
    }
}

impl Workbench {
    /// Reads the project's scripts again, after the files changed.
    pub(super) fn refresh_npm_scripts(&mut self) {
        self.npm_scripts = self.project.root.as_deref().and_then(NpmScripts::read);
    }

    /// Runs a script in a new terminal tab named after it, as VS Code
    /// does from its NPM Scripts view.
    pub(super) fn run_npm_script(
        &mut self,
        script: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(scripts) = &self.npm_scripts else {
            return;
        };
        let command = scripts.command(script);
        let spawn = terminal_view::shell_spawn(self.terminal_folder(), cx);
        if let Some(view) = self.add_terminal(spawn, None, true, window, cx) {
            view.read(cx).run(&command);
            self.name_terminal(&view, script.to_owned());
        }
    }

    /// "npm: Run Script": picks a script from a list.
    fn choose_npm_script(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scripts) = &self.npm_scripts else {
            self.status_message = Some(self.strings().no_npm_scripts.to_owned());
            cx.notify();
            return;
        };
        let names: Vec<String> = scripts
            .scripts
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let items = scripts
            .scripts
            .iter()
            .map(|(name, command)| {
                let mut item = PickItem::new(name.clone());
                item.detail = Some(command.clone());
                item
            })
            .collect();
        let chosen = quick_pick::show(
            None,
            Some(self.strings().run_npm_script.to_owned()),
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(index)) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.run_npm_script(&names[index], window, cx)
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn npm_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &RunNpmScript, window, cx| this.choose_npm_script(window, cx)),
        )
    }

    /// The scripts under the Explorer's tree; a click runs one.
    pub(super) fn render_npm_scripts(&self, colors: Colors, cx: &mut Context<Self>) -> Option<Div> {
        let scripts = self.npm_scripts.as_ref()?;
        let strings = self.strings();
        let rows = scripts
            .scripts
            .iter()
            .enumerate()
            .map(|(index, (name, command))| {
                let label = name.clone();
                let name = name.clone();
                div()
                    .id(("npm-script", index))
                    .h(px(24.))
                    .px_2()
                    .mx_1()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .rounded(px(6.))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.run_npm_script(&name, window, cx)
                    }))
                    .child(Icon::new(IconName::Play).xsmall().text_color(colors.faint))
                    .child(div().flex_none().child(label))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(command.clone()),
                    )
            });
        Some(
            div()
                .flex_none()
                .max_h(px(220.))
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(colors.border)
                .pb_1()
                .child(heading(strings.npm_scripts, colors))
                .child(
                    div()
                        .id("npm-scripts")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .children(rows),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn reads_scripts_and_the_package_manager() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        assert_eq!(NpmScripts::read(root), None);
        fs::write(
            root.join("package.json"),
            r#"{ "name": "app", "scripts": { "dev": "next dev", "build": "next build", "test:e2e": "playwright test" } }"#,
        )
        .unwrap();
        let scripts = NpmScripts::read(root).unwrap();
        assert_eq!(scripts.manager, "npm");
        assert_eq!(
            scripts
                .scripts
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["dev", "build", "test:e2e"]
        );
        assert_eq!(scripts.command("dev"), "npm run dev");

        fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(NpmScripts::read(root).unwrap().manager, "pnpm");
        assert_eq!(
            NpmScripts::read(root).unwrap().command("a b"),
            "pnpm run 'a b'"
        );

        fs::write(
            root.join("package.json"),
            r#"{ "packageManager": "yarn@4.1.0", "scripts": { "dev": "vite" } }"#,
        )
        .unwrap();
        assert_eq!(NpmScripts::read(root).unwrap().manager, "yarn");

        fs::write(root.join("package.json"), r#"{ "name": "lib" }"#).unwrap();
        assert_eq!(NpmScripts::read(root), None);
    }
}
