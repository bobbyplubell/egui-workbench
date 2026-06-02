//! `Host` trait — host integration surface.
//!
//! Every method has a sensible default; the only one a host must
//! implement is [`Host::pane_ui`]. New methods on this
//! trait are backwards-compatible additions as long as they ship with
//! default implementations.

use std::hash::Hash;

use crate::activity_bar::Item;
use crate::side_bar::Location;
use crate::tab::{Document, UiContext};
use crate::theme::{TabStyle, Palette};

/// Host-implemented trait that supplies the workbench with rendering,
/// state, and lifecycle hooks. See `DESIGN.md` for the full method set.
pub trait Host<Tab: Document, Mode: Clone + Eq + Hash + 'static> {
    // === Tab rendering ===

    /// Render the body of a tab in the given `Ui`. Required.
    fn pane_ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab, ctx: UiContext<'_>);

    /// Optional per-tab style override. Default returns `None` (inherit ambient theme).
    fn tab_style(&self, _tab: &Tab) -> Option<TabStyle> {
        None
    }

    // === Tab lifecycle hooks ===

    /// Called when the user clicks the close button on a tab.
    /// Return `false` to veto the close (e.g., to show a save-prompt
    /// modal); the host can later call [`crate::Workbench::close_tab`].
    fn on_tab_close(&mut self, _tab: &Tab) -> bool {
        true
    }

    /// Called when a Preview tab transitions to Regular.
    fn on_preview_promoted(&mut self, _tab: &Tab) {}

    /// Add custom items to the tab right-click context menu.
    /// The crate adds Close / Close Others / Pin / Unpin around this.
    fn tab_context_menu(&mut self, _ui: &mut egui::Ui, _tab: &Tab) {}

    // === Side bar content ===

    /// Render the content of the primary side bar for the currently
    /// active activity.
    fn side_bar_ui(&mut self, _ui: &mut egui::Ui, _mode: &Mode) {}

    /// Title shown in a side bar section header. Defaults to empty. The
    /// SAME path serves both the left (primary) and right (secondary)
    /// stacks; the mode is the section's view id. Defaults to empty.
    fn side_bar_title(&self, _mode: &Mode) -> egui::WidgetText {
        egui::WidgetText::default()
    }

    /// Extra buttons rendered in a side bar section header's right
    /// cluster. Mode-specific (e.g. "new note" for files, "new/delete
    /// session" for chat). Shared by both stacks.
    fn side_bar_action_buttons(&mut self, _ui: &mut egui::Ui, _mode: &Mode) {}

    /// Contents of the popup that opens from a side bar section's header
    /// context menu. Mode-specific (e.g. refresh / sort menu for files).
    /// Default surfaces nothing extra; the workbench still appends its
    /// own "Add panel" / "Close panel" items after. Shared by both stacks.
    fn side_bar_actions_menu(&mut self, _ui: &mut egui::Ui, _mode: &Mode) {}

    // === Activity bar ===

    /// The activities to render, in order. Default is empty (hidden bar
    /// content, though the bar itself still draws).
    fn activity_items(&self) -> Vec<Item<Mode>> {
        Vec::new()
    }

    /// The ordered view ids that an activity-bar container opens when its
    /// icon is clicked. A single-view activity returns `vec![container]`;
    /// a multi-view container returns each of its views. The workbench
    /// feeds these to [`crate::side_panel_stack::SidePanelStack::switch_group`].
    /// Default: the container id alone. [feature-multi-region-sidebar]
    fn container_views(&self, container: &Mode) -> Vec<Mode> {
        vec![container.clone()]
    }

    /// Which stack an activity-bar container drives. `LeftBar` →
    /// `primary_panels`; `RightBar` → `secondary_panels`. Default
    /// `LeftBar`. [feature-multi-region-sidebar]
    fn container_location(&self, _container: &Mode) -> Location {
        Location::LeftBar
    }

    /// Right-click context menu items for an activity.
    fn activity_context_menu(&mut self, _ui: &mut egui::Ui, _mode: &Mode) {}

    // === Status bar ===

    /// Render the status bar cells. Use `ui.with_layout` for alignment.
    fn status_bar_ui(&mut self, _ui: &mut egui::Ui) {}

    // === Theming ===

    /// Per-workbench theme overrides. Default returns the ambient
    /// `egui::Style`-derived theme.
    fn theme(&self, style: &egui::Style) -> Palette {
        Palette::from_egui_style(style)
    }
}
