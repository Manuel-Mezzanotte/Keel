//! The images Keel draws: gpui-component's icons, the few extra Lucide
//! icons the layout uses, and Keel's own logo.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

gpui_kit_assets::icon_assets!(
    ExtraIcons,
    [
        Files,
        GitBranch,
        Blocks,
        Sparkles,
        ArrowUp,
        X,
        FilePlus,
        FolderPlus,
        FileCode,
        FileBraces,
        FileText,
        FileImage,
        RefreshCw,
        ChevronsDownUp
    ]
);

/// Keel's mark, the outline of a paper boat.
pub const LOGO: &str = "brand/keel-mark.svg";
const LOGO_SVG: &[u8] = include_bytes!("../../../assets/brand/keel-mark.svg");

pub struct KeelAssets;

impl AssetSource for KeelAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO {
            return Ok(Some(Cow::Borrowed(LOGO_SVG)));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        if LOGO.starts_with(path) {
            paths.push(LOGO.into());
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit_assets::IconName;

    use super::*;

    #[test]
    fn the_layout_icons_and_the_logo_load() {
        for icon in [
            IconName::Files,
            IconName::GitBranch,
            IconName::Blocks,
            IconName::Sparkles,
            IconName::ArrowUp,
            IconName::X,
            IconName::FilePlus,
            IconName::FolderPlus,
            IconName::FileCode,
            IconName::FileBraces,
            IconName::FileText,
            IconName::FileImage,
            IconName::File,
            IconName::RefreshCw,
            IconName::ChevronsDownUp,
            IconName::ChevronRight,
            IconName::ChevronDown,
            IconName::FolderOpen,
            IconName::Folder,
            IconName::Search,
            IconName::PanelLeft,
            IconName::PanelBottom,
            IconName::PanelRight,
        ] {
            let path = icon.path();
            assert!(
                KeelAssets.load(&path).unwrap().is_some(),
                "{path} is missing"
            );
        }
        let logo = KeelAssets.load(LOGO).unwrap().unwrap();
        assert!(std::str::from_utf8(&logo).unwrap().contains("<svg"));
    }
}
