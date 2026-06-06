//! egui_kittest test driving [`show`] through a real `Ui`: clicking an action
//! button returns that action.

use egui_kittest::kittest::Queryable;
use egui_workbench::menu::{Menu, show};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestAction {
    Open,
    Delete,
}

#[test]
fn clicking_an_action_returns_it() {
    use std::cell::Cell;
    use std::rc::Rc;

    let returned: Rc<Cell<Option<TestAction>>> = Rc::new(Cell::new(None));
    let sink = Rc::clone(&returned);
    let mut harness = egui_kittest::Harness::new_ui(move |ui| {
        let menu = Menu::new()
            .action("Open", TestAction::Open)
            .section()
            .action("Delete", TestAction::Delete);
        if let Some(action) = show(ui, menu) {
            sink.set(Some(action));
        }
    });
    // First frame lays the buttons out; click "Open", then run again so the
    // click registers and `show` returns the action.
    harness.run();
    harness.get_by_label("Open").click();
    harness.run();
    assert_eq!(returned.get(), Some(TestAction::Open));
}
