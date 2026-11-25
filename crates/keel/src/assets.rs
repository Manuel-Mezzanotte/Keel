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
        ChevronsDownUp,
        Pin,
        WholeWord,
        Regex,
        ReplaceAll,
        Columns2,
        Play,
        Trash,
        CircleX,
        TriangleAlert,
        Info,
        Pencil,
        GitMerge,
        GitPullRequest,
        GitPullRequestClosed,
        GitPullRequestDraft,
        Bug,
        Pause,
        RedoDot,
        ArrowDownToDot,
        ArrowUpFromDot,
        RotateCcw,
        Square,
        Clock,
        Square,
        GitGraph,
        Keyboard,
        Tag,
        TextSearch,
        Hash,
        AtSign,
        CornerDownRight,
        Eraser,
        Funnel,
        Download,
        PackagePlus,
        Import,
        RotateCcw,
        Layers,
        LayoutPanelLeft,
        Palette,
        Gauge,
        Command,
        MessageSquare,
        CircleDot,
        Circle,
        ShieldCheck,
        Bell,
        BellDot,
        BellOff,
        CloudUpload,
        CloudDownload,
        ExternalLink,
        Undo2,
        Wrench,
        SquareCode,
        FolderCog,
        Languages,
        CircleArrowDown,
        Puzzle,
        Key,
        Lightbulb,
        SquareSlash,
        TextCursorInput,
        ListChecks,
        ShieldAlert,
        Zap
    ]
);

/// Keel's mark, the outline of a paper boat.
pub const LOGO: &str = "brand/keel-mark.svg";
const LOGO_SVG: &[u8] = include_bytes!("../../../assets/brand/keel-mark.svg");
/// Claude's mark in Keel: a spark of eight rays.
pub const CLAUDE_MARK: &str = "brand/claude-mark.svg";
const CLAUDE_MARK_SVG: &[u8] = include_bytes!("../../../assets/brand/claude-mark.svg");

/// The drawings of the first start's welcome: the boat in layers, one
/// color each, and a gull.
const INTRO: &[(&str, &[u8])] = &[
    (
        "intro/boat-sail.svg",
        include_bytes!("../../../assets/intro/boat-sail.svg"),
    ),
    (
        "intro/boat-sail-shade.svg",
        include_bytes!("../../../assets/intro/boat-sail-shade.svg"),
    ),
    (
        "intro/boat-hull.svg",
        include_bytes!("../../../assets/intro/boat-hull.svg"),
    ),
    (
        "intro/boat-hull-shade.svg",
        include_bytes!("../../../assets/intro/boat-hull-shade.svg"),
    ),
    (
        "intro/boat-ink-0.svg",
        include_bytes!("../../../assets/intro/boat-ink-0.svg"),
    ),
    (
        "intro/boat-ink-1.svg",
        include_bytes!("../../../assets/intro/boat-ink-1.svg"),
    ),
    (
        "intro/boat-ink-2.svg",
        include_bytes!("../../../assets/intro/boat-ink-2.svg"),
    ),
    (
        "intro/boat-ink-3.svg",
        include_bytes!("../../../assets/intro/boat-ink-3.svg"),
    ),
    (
        "intro/boat-ink-4.svg",
        include_bytes!("../../../assets/intro/boat-ink-4.svg"),
    ),
    (
        "intro/gull.svg",
        include_bytes!("../../../assets/intro/gull.svg"),
    ),
];

pub struct KeelAssets;

impl AssetSource for KeelAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO {
            return Ok(Some(Cow::Borrowed(LOGO_SVG)));
        }
        if path == CLAUDE_MARK {
            return Ok(Some(Cow::Borrowed(CLAUDE_MARK_SVG)));
        }
        if let Some((_, bytes)) = INTRO.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
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
        if CLAUDE_MARK.starts_with(path) {
            paths.push(CLAUDE_MARK.into());
        }
        paths.extend(
            INTRO
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit_assets::IconName;

    use super::*;

    /// Every `IconName::…` in Keel's code draws: an icon left out of the
    /// bundle shows as an empty square, and nothing else would notice.
    #[test]
    fn every_icon_keel_uses_is_bundled() {
        fn files(folder: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    files(&path, found);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    found.push(path);
                }
            }
        }
        let mut sources = Vec::new();
        files(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut sources,
        );
        let mut missing = Vec::new();
        for source in sources {
            let text = std::fs::read_to_string(&source).unwrap();
            for (at, _) in text.match_indices("IconName::") {
                let name: String = text[at + "IconName::".len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                let Some(icon) = IconName::ALL
                    .iter()
                    .find(|icon| format!("{icon:?}") == name)
                else {
                    continue;
                };
                let loads = matches!(KeelAssets.load(&icon.path()), Ok(Some(_)));
                if !loads && !missing.contains(&name) {
                    missing.push(name);
                }
            }
        }
        assert!(missing.is_empty(), "not bundled: {missing:?}");
    }

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
            IconName::Pin,
            IconName::WholeWord,
            IconName::Regex,
            IconName::ReplaceAll,
            IconName::Columns2,
            IconName::SquareTerminal,
            IconName::Plus,
            IconName::Maximize,
            IconName::Minimize,
            IconName::Replace,
            IconName::CaseSensitive,
            IconName::Ellipsis,
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
