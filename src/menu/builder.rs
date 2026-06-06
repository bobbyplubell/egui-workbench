//! The fluent builder surface for [`Menu`]. Pure data construction — no `ui`
//! borrow, so builders are unit-testable.

use std::borrow::Cow;

use crate::menu::{Action, Enabled, Entry, Icon, Menu};

impl<A> Action<A> {
    /// Start an action builder with a label and the action it yields.
    pub fn new(label: impl Into<Cow<'static, str>>, action: A) -> Self {
        Self {
            label: label.into(),
            icon: None,
            enabled: Enabled::Yes,
            shortcut: None,
            action,
        }
    }

    /// Set the leading-slot icon.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set the enabled state (use [`Enabled::No`] with a reason to grey it out).
    #[must_use]
    pub fn enabled(mut self, enabled: Enabled) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the right-aligned shortcut hint.
    #[must_use]
    pub fn shortcut(mut self, shortcut: impl Into<Cow<'static, str>>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    fn into_entry(self) -> Entry<A> {
        Entry::Action {
            label: self.label,
            icon: self.icon,
            enabled: self.enabled,
            shortcut: self.shortcut,
            action: self.action,
        }
    }
}

impl<A> Default for Menu<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A> Menu<A> {
    /// An empty menu with one (empty) trailing section ready for entries.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sections: vec![Vec::new()],
        }
    }

    /// Push an entry onto the current (last) section.
    fn push(&mut self, entry: Entry<A>) {
        // `new()` guarantees at least one section, so `last_mut` is always `Some`.
        if let Some(section) = self.sections.last_mut() {
            section.push(entry);
        }
    }

    /// Append a plain clickable action yielding `action`.
    #[must_use]
    pub fn action(mut self, label: impl Into<Cow<'static, str>>, action: A) -> Self {
        self.push(Action::new(label, action).into_entry());
        self
    }

    /// Append an action built with the [`Action`] builder (icon / enabled / shortcut).
    #[must_use]
    pub fn action_with(mut self, action: Action<A>) -> Self {
        self.push(action.into_entry());
        self
    }

    /// Append a checkable toggle row reflecting `checked`.
    #[must_use]
    pub fn toggle(mut self, label: impl Into<Cow<'static, str>>, checked: bool, action: A) -> Self {
        self.push(Entry::Toggle {
            label: label.into(),
            checked,
            action,
        });
        self
    }

    /// Append a nested submenu.
    #[must_use]
    pub fn submenu(mut self, label: impl Into<Cow<'static, str>>, menu: Menu<A>) -> Self {
        self.push(Entry::Submenu {
            label: label.into(),
            menu,
        });
        self
    }

    /// Append a [`Entry::Custom`] escape-hatch row that renders its own widget.
    #[must_use]
    pub fn custom(
        mut self,
        render: impl FnOnce(&mut egui::Ui) -> Option<A> + 'static,
    ) -> Self {
        self.push(Entry::Custom(Box::new(render)));
        self
    }

    /// Start a new separator-delimited section. Entries added after this land in
    /// the new group; the renderer draws a separator between non-empty groups.
    #[must_use]
    pub fn section(mut self) -> Self {
        self.sections.push(Vec::new());
        self
    }

    /// Splice another menu's sections onto this one, for contextual composition
    /// (a host appends its own section to a shared base menu). The other menu's
    /// sections are kept as distinct groups (status: ctxmenu-contextual-extend).
    #[must_use]
    pub fn extend(mut self, other: Menu<A>) -> Self {
        self.sections.extend(other.sections);
        self
    }

    /// The sections, in order. Each inner `Vec` is one separator-delimited group;
    /// the renderer skips empty groups so composition never leaves a dangling rule.
    #[must_use]
    pub fn sections(&self) -> &[Vec<Entry<A>>] {
        &self.sections
    }

    /// Consume the menu into its sections (used by the renderer).
    pub(crate) fn into_sections(self) -> Vec<Vec<Entry<A>>> {
        self.sections
    }
}
