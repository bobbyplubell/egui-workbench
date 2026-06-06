//! Unit tests for the pure-data menu builder: section/entry structure, `extend`
//! splicing, and empty-section handling. No egui involved.

use egui_workbench::menu::{Action, Enabled, Entry, Icon, Menu};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestAction {
    Open,
    Delete,
    Pin,
    Extra,
}

#[test]
fn action_builder_populates_fields() {
    let menu = Menu::new().action_with(
        Action::new("Delete", TestAction::Delete)
            .icon(Icon::glyph("x"))
            .enabled(Enabled::No("read-only".into()))
            .shortcut("Del"),
    );
    let sections = menu.sections();
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].len(), 1);
    match &sections[0][0] {
        Entry::Action {
            label,
            icon,
            enabled,
            shortcut,
            action,
        } => {
            assert_eq!(label, "Delete");
            assert_eq!(icon, &Some(Icon::glyph("x")));
            assert_eq!(enabled, &Enabled::No("read-only".into()));
            assert_eq!(shortcut.as_deref(), Some("Del"));
            assert_eq!(action, &TestAction::Delete);
        }
        _ => panic!("expected an Action entry"),
    }
}

#[test]
fn plain_action_defaults_are_enabled_and_bare() {
    let menu = Menu::new().action("Open", TestAction::Open);
    match &menu.sections()[0][0] {
        Entry::Action {
            icon,
            enabled,
            shortcut,
            ..
        } => {
            assert!(icon.is_none());
            assert_eq!(enabled, &Enabled::Yes);
            assert!(shortcut.is_none());
        }
        _ => panic!("expected an Action entry"),
    }
}

#[test]
fn section_starts_a_new_group() {
    let menu = Menu::new()
        .action("Open", TestAction::Open)
        .section()
        .toggle("Pin", true, TestAction::Pin);
    let sections = menu.sections();
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].len(), 1);
    assert_eq!(sections[1].len(), 1);
    assert!(matches!(sections[1][0], Entry::Toggle { checked: true, .. }));
}

#[test]
fn extend_splices_other_sections() {
    let base = Menu::new().action("Open", TestAction::Open);
    let extra = Menu::new().action("Extra", TestAction::Extra);
    let combined = base.section().extend(extra);
    let sections = combined.sections();
    // base section, the empty section created by `.section()`, then extra's section.
    assert_eq!(sections.len(), 3);
    assert!(matches!(sections[0][0], Entry::Action { action: TestAction::Open, .. }));
    assert!(sections[1].is_empty());
    assert!(matches!(sections[2][0], Entry::Action { action: TestAction::Extra, .. }));
}

#[test]
fn submenu_nests_a_menu() {
    let sub = Menu::new().action("Inner", TestAction::Open);
    let menu = Menu::new().submenu("More", sub);
    match &menu.sections()[0][0] {
        Entry::Submenu { label, menu } => {
            assert_eq!(label, "More");
            assert_eq!(menu.sections()[0].len(), 1);
        }
        _ => panic!("expected a Submenu entry"),
    }
}

#[test]
fn enabled_is_enabled_reflects_variant() {
    assert!(Enabled::Yes.is_enabled());
    assert!(!Enabled::No("nope".into()).is_enabled());
}
