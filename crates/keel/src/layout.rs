//! Which parts of the window are shown: the sidebar and its view, the
//! bottom panel and its view, and the Claude Code panel on the right.

/// The views of the sidebar, chosen from the switch at its top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideView {
    Explorer,
    Search,
    SourceControl,
    Extensions,
}

impl SideView {
    /// The name kept in the session file.
    pub fn id(self) -> &'static str {
        match self {
            SideView::Explorer => "explorer",
            SideView::Search => "search",
            SideView::SourceControl => "scm",
            SideView::Extensions => "extensions",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.id() == id)
    }

    pub const ALL: [SideView; 4] = [
        SideView::Explorer,
        SideView::Search,
        SideView::SourceControl,
        SideView::Extensions,
    ];
}

/// The views of the bottom panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelView {
    Problems,
    Output,
    Terminal,
    Ports,
    Tests,
}

impl PanelView {
    /// The name kept in the session file.
    pub fn id(self) -> &'static str {
        match self {
            PanelView::Problems => "problems",
            PanelView::Output => "output",
            PanelView::Terminal => "terminal",
            PanelView::Ports => "ports",
            PanelView::Tests => "tests",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.id() == id)
    }

    pub const ALL: [PanelView; 5] = [
        PanelView::Problems,
        PanelView::Output,
        PanelView::Terminal,
        PanelView::Ports,
        PanelView::Tests,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub sidebar: bool,
    pub side_view: SideView,
    pub panel: bool,
    pub panel_view: PanelView,
    pub agent: bool,
}

impl Default for Layout {
    /// The editors get the room: the bottom panel opens when needed.
    fn default() -> Self {
        Self {
            sidebar: true,
            side_view: SideView::Explorer,
            panel: false,
            panel_view: PanelView::Terminal,
            agent: true,
        }
    }
}

impl Layout {
    /// A command such as "View: Search" always shows its view.
    pub fn show_side_view(&mut self, view: SideView) {
        self.sidebar = true;
        self.side_view = view;
    }

    /// A click on the sidebar's switch. Clicking the view already shown
    /// does nothing, so a stray click never hides the sidebar.
    pub fn select_side_view(&mut self, view: SideView) {
        self.side_view = view;
    }

    /// A toggle such as "View: Toggle Output": shows the view, or hides
    /// the panel when that view is already in front, as VS Code does.
    pub fn toggle_panel_view(&mut self, view: PanelView) {
        if self.panel && self.panel_view == view {
            self.panel = false;
        } else {
            self.panel = true;
            self.panel_view = view;
        }
    }

    /// A command such as "View: Problems" always shows its view.
    pub fn show_panel_view(&mut self, view: PanelView) {
        self.panel = true;
        self.panel_view = view;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editors_get_the_room_at_first() {
        let layout = Layout::default();
        assert!(layout.sidebar && layout.agent && !layout.panel);
        assert_eq!(layout.side_view, SideView::Explorer);
    }

    #[test]
    fn side_view_commands_open_the_sidebar() {
        let mut layout = Layout {
            sidebar: false,
            ..Layout::default()
        };
        layout.show_side_view(SideView::Extensions);
        assert!(layout.sidebar);
        assert_eq!(layout.side_view, SideView::Extensions);
        layout.select_side_view(SideView::Extensions);
        assert!(layout.sidebar);
    }

    #[test]
    fn panel_toggles_hide_the_panel_only_from_their_own_view() {
        let mut layout = Layout::default();
        layout.toggle_panel_view(PanelView::Output);
        assert!(layout.panel);
        assert_eq!(layout.panel_view, PanelView::Output);

        layout.toggle_panel_view(PanelView::Terminal);
        assert!(layout.panel);
        assert_eq!(layout.panel_view, PanelView::Terminal);

        layout.toggle_panel_view(PanelView::Terminal);
        assert!(!layout.panel);

        layout.show_panel_view(PanelView::Problems);
        layout.show_panel_view(PanelView::Problems);
        assert!(layout.panel);
    }
}
