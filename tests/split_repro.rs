//! Repro for the "duplicated tab group" bug: dropping a tab onto a group's
//! body splits the *active pane inside the group* instead of the group
//! itself, leaving a `Linear` split nested inside a `Tabs` container. That
//! renders as stacked tab strips. egui_tiles' own `simplify()` does NOT
//! fix it (the Tabs has >1 child, so it isn't single-child-collapsed).

use egui_tiles::{Container, SimplificationOptions, Tile, Tree};

fn wb_opts() -> SimplificationOptions {
    SimplificationOptions {
        prune_empty_tabs: true,
        prune_empty_containers: true,
        prune_single_child_tabs: true,
        prune_single_child_containers: true,
        all_panes_must_have_tabs: true,
        join_nested_linear_containers: true,
    }
}

fn dump(tree: &Tree<u64>) -> String {
    fn go(tree: &Tree<u64>, id: egui_tiles::TileId, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        match tree.tiles.get(id) {
            Some(Tile::Pane(h)) => out.push_str(&format!("{pad}Pane({h})\n")),
            Some(Tile::Container(c)) => {
                let kind = match c {
                    Container::Tabs(_) => "Tabs",
                    Container::Linear(l) => match l.dir {
                        egui_tiles::LinearDir::Horizontal => "Horizontal",
                        egui_tiles::LinearDir::Vertical => "Vertical",
                    },
                    Container::Grid(_) => "Grid",
                };
                out.push_str(&format!("{pad}{kind}\n"));
                for child in c.children().copied().collect::<Vec<_>>() {
                    go(tree, child, depth + 1, out);
                }
            }
            None => out.push_str(&format!("{pad}<missing {id:?}>\n")),
        }
    }
    let mut out = String::new();
    if let Some(root) = tree.root() {
        go(tree, root, 0, &mut out);
    }
    out
}

#[test]
fn body_drop_nests_a_split_inside_the_tab_group() {
    // Build the exact tree egui_tiles produces when a *new* tab `n` is
    // dropped onto the bottom half of the body of group Tabs[home, sync]
    // while `sync` is the active (rendered) pane: the `sync` pane is wrapped
    // in a Vertical split *inside* the Tabs container.
    let mut tiles = egui_tiles::Tiles::default();
    let home = tiles.insert_pane(1u64);
    let sync = tiles.insert_pane(2u64);
    let n = tiles.insert_pane(3u64);
    let split = tiles.insert_vertical_tile(vec![sync, n]); // Vertical{sync, n}
    let group = tiles.insert_tab_tile(vec![home, split]); // Tabs[home, <split>]
    let mut tree = Tree::new("t", group, tiles);

    // egui_tiles' frame-start simplify (what Tree::ui runs):
    tree.simplify(&wb_opts());

    let after = dump(&tree);
    eprintln!("post-simplify tree:\n{after}");

    // The bug: a Tabs container still holds a Vertical split as a child.
    let tabs_with_container_child = tree.tiles.iter().any(|(_, t)| {
        if let Tile::Container(Container::Tabs(tabs)) = t {
            tabs.children.iter().any(|c| {
                matches!(tree.tiles.get(*c), Some(Tile::Container(_)))
            })
        } else {
            false
        }
    });
    assert!(
        tabs_with_container_child,
        "expected the bug: a tab group nesting a split. tree:\n{after}"
    );
}

// The fix lives in the crate (`tree_adapter::normalize_tab_groups`), exercised
// through the public workbench in `tests/smoke.rs`. Here we just pin the
// *shape* the simplify policy must leave for the normalizer to act on.
#[test]
fn simplify_alone_does_not_dedupe_nested_group() {
    let mut tiles = egui_tiles::Tiles::default();
    let a = tiles.insert_pane(1u64);
    let b = tiles.insert_pane(2u64);
    let c = tiles.insert_pane(3u64);
    let split = tiles.insert_vertical_tile(vec![b, c]);
    let group = tiles.insert_tab_tile(vec![a, split]);
    let mut tree = Tree::new("t", group, tiles);
    tree.simplify(&wb_opts());
    // Still three tab groups (a, b, c) — i.e. simplify did NOT merge a+b.
    let tab_groups = tree
        .tiles
        .iter()
        .filter(|(_, t)| matches!(t, Tile::Container(Container::Tabs(_))))
        .count();
    assert_eq!(tab_groups, 3, "simplify unexpectedly merged groups:\n{}", dump(&tree));
}
