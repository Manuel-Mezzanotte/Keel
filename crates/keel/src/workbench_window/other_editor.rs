//! The editors Keel brings a person's things from: VS Code, which most
//! people come from, and Cursor, built on it. Both keep the same files in
//! the same shapes, in folders of their own.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OtherEditor {
    #[default]
    VsCode,
    Cursor,
}

impl OtherEditor {
    pub(crate) const ALL: [OtherEditor; 2] = [OtherEditor::VsCode, OtherEditor::Cursor];

    /// The name people know it by.
    pub(crate) fn name(self) -> &'static str {
        match self {
            OtherEditor::VsCode => "VS Code",
            OtherEditor::Cursor => "Cursor",
        }
    }

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    /// Its folder of settings, shortcuts, snippets and recent projects.
    pub(crate) fn user_dir(self) -> Option<PathBuf> {
        let folder = match self {
            OtherEditor::VsCode => "Code",
            OtherEditor::Cursor => "Cursor",
        };
        Self::home()
            .map(|home| {
                home.join("Library/Application Support")
                    .join(folder)
                    .join("User")
            })
            .filter(|dir| dir.is_dir())
    }

    /// Its extensions, with `extensions.json` listing them.
    pub(crate) fn extensions_dir(self) -> Option<PathBuf> {
        let folder = match self {
            OtherEditor::VsCode => ".vscode/extensions",
            OtherEditor::Cursor => ".cursor/extensions",
        };
        Self::home()
            .map(|home| home.join(folder))
            .filter(|dir| dir.is_dir())
    }

    /// Whether this Mac has it, or had it: its folders are still there.
    pub(crate) fn is_present(self) -> bool {
        self.user_dir().is_some() || self.extensions_dir().is_some()
    }

    /// Settings of its own that mean nothing in Keel, or would change what
    /// Keel looks like into something it does not have.
    pub(crate) fn skips(self, key: &str) -> bool {
        let own = match self {
            OtherEditor::VsCode => {
                key.starts_with("github.copilot") || key.starts_with("settingsSync.")
            }
            OtherEditor::Cursor => key.starts_with("cursor."),
        };
        own || key.starts_with("telemetry.")
            || key.starts_with("update.")
            || key.starts_with("remote.")
            || matches!(
                key,
                "workbench.colorTheme" | "workbench.iconTheme" | "workbench.productIconTheme"
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_editor_skips_what_is_only_its_own() {
        assert!(OtherEditor::Cursor.skips("cursor.cpp.enable"));
        assert!(!OtherEditor::VsCode.skips("cursor.cpp.enable"));
        assert!(OtherEditor::VsCode.skips("github.copilot.enable"));
        assert!(OtherEditor::VsCode.skips("workbench.colorTheme"));
        assert!(!OtherEditor::VsCode.skips("editor.fontSize"));
        assert_eq!(
            OtherEditor::ALL.map(OtherEditor::name),
            ["VS Code", "Cursor"]
        );
    }
}
