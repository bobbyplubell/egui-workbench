//! Generalized context-menu primitive (status: ctxmenu-crate).
//!
//! A menu is **pure data** — a [`Menu<A>`] is sections of [`Entry<A>`], generic
//! over the caller's action type `A`. Building one borrows no `ui`, so builders
//! are unit-testable. One renderer, [`show`], is the only code that touches egui
//! for menus: separators, disabling, submenus, icons, and closing all live there.
//! The chosen action is returned to the caller for per-domain dispatch.
//!
//! ```ignore
//! let menu = Menu::new()
//!     .action("Open", Verb::Open)
//!     .toggle("Pinned", pinned, Verb::TogglePin);
//! if let Some(verb) = response
//!     .context_menu(|ui| egui_workbench::menu::show(ui, menu))
//!     .and_then(|r| r.inner)
//!     .flatten()
//! {
//!     dispatch(verb);
//! }
//! ```

use std::borrow::Cow;

mod builder;
mod render;

/// A small, egui-agnostic icon: a glyph string drawn in the entry's leading slot
/// so labels align. A newtype now; richer icon kinds can be added later without
/// changing entry shapes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon(pub Cow<'static, str>);

impl Icon {
    /// Wrap a glyph (e.g. an emoji or symbol char) as a leading-slot icon.
    pub fn glyph(glyph: impl Into<Cow<'static, str>>) -> Self {
        Self(glyph.into())
    }
}

/// Whether an [`Entry::Action`] can be activated. `No` carries the reason the
/// action is unavailable, rendered as the disabled entry's hover tooltip so the
/// menu teaches *why* instead of hiding the option (status: ctxmenu-disabled-reason).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enabled {
    /// The action is available and clickable.
    Yes,
    /// The action is greyed out; the string is the tooltip reason.
    No(Cow<'static, str>),
}

impl Enabled {
    /// `true` when the action is clickable.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        matches!(self, Self::Yes)
    }
}

/// One row of a menu (status: ctxmenu-entry-kinds). Generic over the caller's
/// action type `A`, which the renderer returns when the entry is activated.
pub enum Entry<A> {
    /// The common case: a clickable button yielding `action`. Optionally carries
    /// a leading `icon`, an [`Enabled`] state (greyed + tooltip when `No`), and a
    /// right-aligned `shortcut` hint.
    Action {
        /// The button label.
        label: Cow<'static, str>,
        /// Optional leading-slot glyph.
        icon: Option<Icon>,
        /// Whether the action is clickable, with a reason when not.
        enabled: Enabled,
        /// Optional right-aligned shortcut hint.
        shortcut: Option<Cow<'static, str>>,
        /// The action returned when clicked.
        action: A,
    },
    /// A checkable row: rendered with a checkmark reflecting `checked`, yielding
    /// `action` when toggled.
    Toggle {
        /// The toggle label.
        label: Cow<'static, str>,
        /// Current checked state.
        checked: bool,
        /// The action returned when clicked.
        action: A,
    },
    /// A nested submenu opened on hover; the chosen action bubbles up through the
    /// same [`show`] recursion.
    Submenu {
        /// The submenu button label.
        label: Cow<'static, str>,
        /// The nested menu.
        menu: Menu<A>,
    },
    /// Escape hatch for an entry that must render a live widget (a zoom row, a
    /// WIP-limit radio). The closure draws into the `ui` and yields an `A` when used.
    Custom(CustomRender<A>),
}

/// The boxed closure behind [`Entry::Custom`]: draws into the `ui` and yields the
/// caller's action `A` when the custom widget is used.
pub type CustomRender<A> = Box<dyn FnOnce(&mut egui::Ui) -> Option<A>>;

/// A context menu: an ordered list of sections, each a list of [`Entry`].
/// Sections render with a separator between non-empty groups. Pure data — build
/// it with the fluent methods, then hand it to [`show`] (status: ctxmenu-menu-spec).
pub struct Menu<A> {
    sections: Vec<Vec<Entry<A>>>,
}

/// A fluent builder for one [`Entry::Action`], used when an action needs an icon,
/// a disabled reason, or a shortcut hint. Hand the finished builder to
/// [`Menu::action_with`].
///
/// ```ignore
/// menu.action_with(Action::new("Delete", Verb::Delete)
///     .icon(Icon::glyph("🗑"))
///     .enabled(Enabled::No("read-only vault".into()))
///     .shortcut("Del"));
/// ```
pub struct Action<A> {
    pub(crate) label: Cow<'static, str>,
    pub(crate) icon: Option<Icon>,
    pub(crate) enabled: Enabled,
    pub(crate) shortcut: Option<Cow<'static, str>>,
    pub(crate) action: A,
}

/// Render `menu` into `ui` and return the activated action, if any.
///
/// Sections render top-to-bottom with a separator between non-empty groups (no
/// leading / trailing / double separator, even when empty sections are skipped).
/// Submenus recurse through this same function. After an action is chosen the menu
/// is dismissed via `ui.close()`. Closing-on-click, nested-submenu state, and
/// escape handling ride egui's built-in menu machinery. The function is pure: no
/// app reference, no mutation beyond the `ui`.
pub fn show<A>(ui: &mut egui::Ui, menu: Menu<A>) -> Option<A> {
    render::show(ui, menu)
}
