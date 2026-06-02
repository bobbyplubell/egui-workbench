//! Side bar — host for activity content. Implements `SPEC.md` §2/§3.

/// Which edge a side bar lives on. Default `Left`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// Where an activity's views dock. Each location renders its own
/// [`crate::side_panel_stack::SidePanelStack`]: `LeftBar` → the primary
/// accordion, `RightBar` → the secondary accordion. A `BottomPanel`
/// location is a later addition. Hosts read an activity's default
/// location to route an activity-bar click (or seed the placement
/// overlay) to the correct stack. [feature-multi-region-sidebar]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Location {
    #[default]
    LeftBar,
    RightBar,
}

/// One side bar instance. The workbench owns two of these: a primary
/// and an optional secondary (rendered on the opposite side).
pub struct SideBar {
    pub side: Side,
    pub visible: bool,
    pub width: f32,
    /// Lower bound on the user-resizable width.
    pub min_width: f32,
    /// Upper bound on the user-resizable width.
    pub max_width: f32,
}

impl Default for SideBar {
    fn default() -> Self {
        Self {
            side: Side::Left,
            visible: true,
            width: 260.0,
            min_width: 80.0,
            max_width: 600.0,
        }
    }
}

impl SideBar {
    pub fn new(side: Side) -> Self {
        Self { side, ..Self::default() }
    }

    pub const fn toggle(&mut self) {
        self.visible = !self.visible;
    }
}
