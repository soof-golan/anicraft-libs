//! Docking on top of `craft-ui`'s layout model: panel identities, the default Animate-like arrangement and
//! persistence.

pub use craft_ui::docking;
use craft_ui::{
    docking::{Action, DockArea, DockError, DockOutput, DockStyle, Layout, Node, Permissions},
    layout::{SplitAxis, SplitSize},
};
use egui::{Id, Ui};
use serde::{Deserialize, Serialize};

use crate::theme::{current_palette, visual};

/// The panels the apps know about. `Custom` carries an app-defined panel number (`CharCraft`'s puppet panel, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    Tools,
    Stage,
    Timeline,
    Properties,
    Library,
    Output,
    Custom(u16),
}

impl PanelId {
    /// The default tab title.
    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::Tools => "Tools".into(),
            Self::Stage => "Stage".into(),
            Self::Timeline => "Timeline".into(),
            Self::Properties => "Properties".into(),
            Self::Library => "Library".into(),
            Self::Output => "Output".into(),
            Self::Custom(n) => format!("Panel {n}"),
        }
    }

    /// What the user may do with the panel: the stage is the document canvas and stays put.
    #[must_use]
    pub const fn permissions(self) -> Permissions {
        match self {
            Self::Stage => Permissions::PROTECTED,
            Self::Tools => Permissions { movable: true, floatable: true, closable: true, accepts_tabs: false },
            _ => Permissions { movable: true, floatable: true, closable: true, accepts_tabs: true },
        }
    }
}

/// The default arrangement: tools on the left, the stage in the centre, the timeline under it, properties and
/// library stacked on the right.
#[must_use]
pub fn default_layout() -> Layout<PanelId> {
    let tabs = |panel| Node::Tabs { panels: vec![panel], active: 0 };
    let centre = Node::Split {
        axis: SplitAxis::Vertical,
        size: SplitSize::FixedSecond(260.0),
        first: Box::new(tabs(PanelId::Stage)),
        second: Box::new(tabs(PanelId::Timeline)),
    };
    let right = Node::Split {
        axis: SplitAxis::Vertical,
        size: SplitSize::Ratio(0.55),
        first: Box::new(tabs(PanelId::Properties)),
        second: Box::new(tabs(PanelId::Library)),
    };
    let middle = Node::Split { axis: SplitAxis::Horizontal, size: SplitSize::FixedSecond(300.0), first: Box::new(centre), second: Box::new(right) };
    let root = Node::Split { axis: SplitAxis::Horizontal, size: SplitSize::FixedFirst(76.0), first: Box::new(tabs(PanelId::Tools)), second: Box::new(middle) };
    Layout { root: Some(root), floating: Vec::new() }
}

/// Errors loading a saved layout.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("layout is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("layout is not usable: {0}")]
    Invalid(DockError),
}

/// Serialises a layout to JSON.
///
/// # Errors
/// Fails only if serialisation fails, which plain panel ids cannot cause.
pub fn layout_to_json(layout: &Layout<PanelId>) -> Result<String, LayoutError> {
    Ok(serde_json::to_string_pretty(layout)?)
}

/// Parses and validates a saved layout (depth, size, unique panels, geometry).
///
/// # Errors
/// Returns [`LayoutError`] when the text is not a layout or the layout fails validation.
pub fn layout_from_json(text: &str) -> Result<Layout<PanelId>, LayoutError> {
    let layout: Layout<PanelId> = serde_json::from_str(text)?;
    layout.validate().map_err(LayoutError::Invalid)?;
    Ok(layout)
}

/// The saved layout, or the default when `text` is missing or unusable.
///
/// Panels from the default layout that the saved one lacks (new in this version) are not added; the user can open
/// them from the Window menu.
#[must_use]
pub fn layout_or_default(text: Option<&str>) -> Layout<PanelId> {
    text.and_then(|t| layout_from_json(t).ok()).unwrap_or_else(default_layout)
}

/// The dock chrome in the current theme.
#[must_use]
pub fn dock_style(ui: &Ui) -> DockStyle {
    let p = current_palette(ui.ctx());
    let mut style = DockStyle::from_ui(ui);
    style.background = p.panel;
    style.tab_background = p.recessed;
    style.active_background = p.surface;
    style.text = p.text;
    style.inactive_text = p.text_weak;
    style.border = egui::Stroke::new(1.0, p.border);
    style.accent = p.accent;
    style
}

/// Draws the dock in the available space.
///
/// `title` names a panel (use [`PanelId::title`] for the defaults, or map `Custom` ids to your own titles); `body`
/// draws one panel's contents. The returned actions go to [`apply_actions`] after the frame.
pub fn show_dock(ui: &mut Ui, id: Id, layout: &Layout<PanelId>, title: impl Fn(PanelId) -> String, body: impl FnMut(&mut Ui, &PanelId)) -> DockOutput<PanelId> {
    let style = dock_style(ui);
    DockArea::new(id).show(ui, layout, &style, |panel| visual(&title(*panel)).into_owned(), |panel| panel.permissions(), body)
}

/// The panel a closed `panel` is reopened next to (its place in the default layout).
#[must_use]
pub const fn default_anchor(panel: PanelId) -> PanelId {
    match panel {
        PanelId::Tools | PanelId::Properties | PanelId::Library | PanelId::Custom(_) => PanelId::Properties,
        PanelId::Stage => PanelId::Stage,
        PanelId::Timeline | PanelId::Output => PanelId::Timeline,
    }
}

/// Brings a closed panel back as a tab beside its default anchor (or, when that is gone too, beside the timeline or
/// the properties panel).
///
/// Used by a Window menu: list [`PanelId`]s, show a check for [`Layout::contains`], and call this when an unchecked
/// one is picked.
///
/// # Errors
/// Returns the layout's refusal (for example when the panel is already open).
pub fn open_panel(layout: &mut Layout<PanelId>, panel: PanelId) -> Result<(), DockError> {
    let anchor = [default_anchor(panel), PanelId::Timeline, PanelId::Properties].into_iter().find(|a| *a != panel && layout.contains(a));
    layout.apply_with_permissions(Action::Open { panel, anchor }, |p| p.permissions())
}

/// Applies the actions a dock frame produced, ignoring (and returning) the first that the layout refuses.
pub fn apply_actions(layout: &mut Layout<PanelId>, actions: Vec<Action<PanelId>>) -> Option<DockError> {
    let mut first_error = None;
    for action in actions {
        if let Err(e) = layout.apply_with_permissions(action, |p| p.permissions())
            && first_error.is_none()
        {
            first_error = Some(e);
        }
    }
    first_error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_is_valid_and_complete() {
        let layout = default_layout();
        assert_eq!(layout.validate(), Ok(()));
        for panel in [PanelId::Tools, PanelId::Stage, PanelId::Timeline, PanelId::Properties, PanelId::Library] {
            assert!(layout.contains(&panel), "{panel:?}");
        }
        assert!(!layout.contains(&PanelId::Output));
    }

    #[test]
    fn layout_round_trips_through_json() {
        let layout = default_layout();
        let json = layout_to_json(&layout).unwrap_or_default();
        assert_eq!(layout_from_json(&json).ok(), Some(layout));
    }

    #[test]
    fn garbage_layouts_fall_back_to_default() {
        assert_eq!(layout_or_default(Some("not json")), default_layout());
        assert_eq!(layout_or_default(Some("{\"root\":null,\"floating\":[{\"panels\":[],\"active\":0,\"rect\":[0,0,1,1]}]}")), default_layout());
        assert_eq!(layout_or_default(None), default_layout());
    }

    #[test]
    fn closed_panels_reopen_beside_their_anchor() {
        let mut layout = default_layout();
        assert_eq!(apply_actions(&mut layout, vec![Action::Close { panel: PanelId::Library }]), None);
        assert_eq!(open_panel(&mut layout, PanelId::Library), Ok(()));
        assert!(layout.contains(&PanelId::Library));
        assert_eq!(layout.validate(), Ok(()));
        // Opening a panel that is already open just activates it: it never appears twice.
        assert_eq!(open_panel(&mut layout, PanelId::Library), Ok(()));
        assert_eq!(layout.panels().into_iter().filter(|p| **p == PanelId::Library).count(), 1);
        // A panel the default layout never had opens too.
        assert_eq!(open_panel(&mut layout, PanelId::Output), Ok(()));
        assert!(layout.contains(&PanelId::Output));
    }

    #[test]
    fn stage_cannot_be_closed_or_moved() {
        let mut layout = default_layout();
        let err = apply_actions(&mut layout, vec![Action::Close { panel: PanelId::Stage }]);
        assert_eq!(err, Some(DockError::NotAllowed));
        assert!(layout.contains(&PanelId::Stage));
        assert_eq!(apply_actions(&mut layout, vec![Action::Close { panel: PanelId::Library }]), None);
        assert!(!layout.contains(&PanelId::Library));
    }
}
