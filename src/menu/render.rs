//! The single menu renderer (status: ctxmenu-renderer). [`show`] is the only code
//! that calls `ui.button` / `ui.menu_button` / `ui.close` for menus. It walks the
//! [`Menu`] top-to-bottom, emits one egui widget per [`Entry`], and returns the
//! action of whichever entry was activated. Pure: no app reference, no mutation
//! beyond the `ui`.

use crate::menu::{Enabled, Entry, Menu};

/// Render `menu` into `ui` and return the activated action, if any.
///
/// Sections render top-to-bottom with a separator between non-empty groups (no
/// leading / trailing / double separator, even when empty sections are skipped).
/// Submenus recurse through this same function. After an action is chosen the menu
/// is dismissed via `ui.close()`. Closing-on-click, nested-submenu state, and
/// escape handling ride egui's built-in menu machinery.
pub fn show<A>(ui: &mut egui::Ui, menu: Menu<A>) -> Option<A> {
    let mut chosen = None;
    let mut drawn_a_section = false;
    for section in menu.into_sections() {
        if section.is_empty() {
            continue;
        }
        if drawn_a_section {
            ui.separator();
        }
        drawn_a_section = true;
        for entry in section {
            if let Some(action) = show_entry(ui, entry) {
                chosen = Some(action);
            }
        }
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

/// Render one entry, returning its action if it was activated.
fn show_entry<A>(ui: &mut egui::Ui, entry: Entry<A>) -> Option<A> {
    match entry {
        Entry::Action {
            label,
            icon,
            enabled,
            shortcut,
            action,
        } => show_action(ui, label, icon, &enabled, shortcut, action),
        Entry::Toggle {
            label,
            checked,
            action,
        } => show_toggle(ui, &label, checked, action),
        Entry::Submenu { label, menu } => show_submenu(ui, label, menu),
        Entry::Custom(render) => render(ui),
    }
}

/// Render an `Action`: a button with a leading icon glyph and a right-aligned
/// shortcut. When disabled, it is greyed and shows the reason as a hover tooltip.
fn show_action<A>(
    ui: &mut egui::Ui,
    label: std::borrow::Cow<'static, str>,
    icon: Option<crate::menu::Icon>,
    enabled: &Enabled,
    shortcut: Option<std::borrow::Cow<'static, str>>,
    action: A,
) -> Option<A> {
    let text = match icon {
        Some(icon) => format!("{}  {}", icon.0, label),
        None => label.into_owned(),
    };
    let mut button = egui::Button::new(text);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(shortcut.into_owned());
    }
    let response = ui.add_enabled(enabled.is_enabled(), button);
    if let Enabled::No(reason) = enabled {
        return response.on_disabled_hover_text(reason.clone()).clicked().then_some(action);
    }
    response.clicked().then_some(action)
}

/// Render a `Toggle` as a selectable (checkmark) button.
fn show_toggle<A>(ui: &mut egui::Ui, label: &str, checked: bool, action: A) -> Option<A> {
    let prefix = if checked { "\u{2713} " } else { "    " };
    let button = egui::Button::new(format!("{prefix}{label}")).selected(checked);
    ui.add(button).clicked().then_some(action)
}

/// Render a `Submenu` via `ui.menu_button`, recursing into [`show`].
fn show_submenu<A>(
    ui: &mut egui::Ui,
    label: std::borrow::Cow<'static, str>,
    menu: Menu<A>,
) -> Option<A> {
    ui.menu_button(label.into_owned(), |ui| show(ui, menu)).inner.flatten()
}
