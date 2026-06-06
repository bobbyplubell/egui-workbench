//! Helpers that bridge `egui_tiles::Tree` to workbench concepts.
//!
//! The editor tree and panel tree store [`crate::TabId`] payloads
//! (not the tabs themselves). Payload lookup is a single hashmap probe
//! against `EditorArea::entries`.

use std::collections::HashMap;

use egui_tiles::{Container, Tile, TileId, Tree};

use crate::workspace::TabId;

/// Walk the tree and find the [`TileId`] of the [`Tabs`](egui_tiles::Container::Tabs)
/// container that currently holds `handle` as one of its children.
pub(crate) fn find_group_of<P>(tree: &Tree<P>, handle: TabId) -> Option<TileId>
where
    P: PartialEq<TabId>,
{
    for (tile_id, tile) in tree.tiles.iter() {
        if let Tile::Pane(pane) = tile
            && pane == &handle
        {
            return tree.tiles.parent_of(*tile_id);
        }
    }
    None
}

/// Find the [`TileId`] of the pane carrying `handle`.
pub(crate) fn find_pane_of<P>(tree: &Tree<P>, handle: TabId) -> Option<TileId>
where
    P: PartialEq<TabId>,
{
    for (tile_id, tile) in tree.tiles.iter() {
        if let Tile::Pane(pane) = tile
            && pane == &handle
        {
            return Some(*tile_id);
        }
    }
    None
}

/// First [`Tabs`](egui_tiles::Container::Tabs) container we encounter
/// during traversal. Used as the fallback "active group" when no group
/// has been explicitly focused yet.
pub(crate) fn first_tabs_container<P>(tree: &Tree<P>) -> Option<TileId> {
    for (tile_id, tile) in tree.tiles.iter() {
        if let Tile::Container(container) = tile
            && matches!(container, egui_tiles::Container::Tabs(_))
        {
            return Some(*tile_id);
        }
    }
    None
}

/// Collect all `TabId`s that live in the given Tabs container.
pub(crate) fn handles_in_group(tree: &Tree<TabId>, group: TileId) -> Vec<TabId> {
    let Some(Tile::Container(egui_tiles::Container::Tabs(tabs))) = tree.tiles.get(group) else {
        return Vec::new();
    };
    tabs.children
        .iter()
        .filter_map(|child| match tree.tiles.get(*child) {
            Some(Tile::Pane(h)) => Some(*h),
            _ => None,
        })
        .collect()
}

/// Iterate every `TabId` referenced anywhere in the tree.
pub(crate) fn all_handles(tree: &Tree<TabId>) -> Vec<TabId> {
    tree.tiles
        .iter()
        .filter_map(|(_, tile)| match tile {
            Tile::Pane(h) => Some(*h),
            _ => None,
        })
        .collect()
}

/// Map every handle to the `Tabs` group it currently lives in. Captured
/// *before* a frame's drag-drop so [`normalize_tab_groups`] can regroup the
/// panes left behind with the split half that kept their original siblings.
pub(crate) fn group_of_each_handle(tree: &Tree<TabId>) -> HashMap<TabId, TileId> {
    let mut map = HashMap::new();
    for (tile_id, tile) in tree.tiles.iter() {
        if let Tile::Container(Container::Tabs(tabs)) = tile {
            for child in &tabs.children {
                if let Some(Tile::Pane(h)) = tree.tiles.get(*child) {
                    map.insert(*h, *tile_id);
                }
            }
        }
    }
    map
}

/// Enforce the workbench invariant that a `Tabs` group holds **only panes** —
/// never another container. egui_tiles' drag-drop offers a split drop-zone
/// for the active pane *inside* a group, so dropping a tab on a group's body
/// wraps that one pane in a `Linear`/`Grid` split that stays nested inside the
/// `Tabs` container (and a "drop into a tab" can nest one `Tabs` inside
/// another). Both render as stacked tab strips — the "duplicated tab group"
/// bug. egui_tiles' own `simplify()` leaves them alone because the outer
/// `Tabs` has more than one child.
///
/// We lift the nested structure up to the group's own level so a body drop
/// reads as a clean split of the *whole* group: `Tabs[a, Vertical{Tabs[b],
/// Tabs[c]}]` becomes `Vertical{Tabs[a, b], Tabs[c]}`. The panes left behind
/// (`a`) rejoin the split half holding a pane that shared their group before
/// the drop (looked up in `prev_group_of`), so grouping is correct for any
/// drop direction; failing that we fall back to the split's first leaf.
///
/// Expects an already-simplified tree (leaves are `Tabs`, splits are
/// `Linear`/`Grid`). Returns `true` if it changed the tree.
pub(crate) fn normalize_tab_groups(
    tree: &mut Tree<TabId>,
    prev_group_of: &HashMap<TabId, TileId>,
) -> bool {
    let mut changed = false;
    // Each pass lifts or flattens one offending nesting; the tree's nesting
    // strictly decreases, so the loop terminates.
    while lift_one_nested_container(tree, prev_group_of) {
        changed = true;
    }
    changed
}

/// Find one `Tabs` container with a container child and resolve it. Returns
/// `false` when the tree already satisfies the invariant.
fn lift_one_nested_container(
    tree: &mut Tree<TabId>,
    prev_group_of: &HashMap<TabId, TileId>,
) -> bool {
    let mut offender: Option<(TileId, usize, TileId, bool)> = None;
    'scan: for (tile_id, tile) in tree.tiles.iter() {
        if let Tile::Container(Container::Tabs(tabs)) = tile {
            for (idx, child) in tabs.children.iter().enumerate() {
                match tree.tiles.get(*child) {
                    Some(Tile::Container(Container::Tabs(_))) => {
                        offender = Some((*tile_id, idx, *child, true));
                        break 'scan;
                    }
                    Some(Tile::Container(_)) => {
                        offender = Some((*tile_id, idx, *child, false));
                        break 'scan;
                    }
                    _ => {}
                }
            }
        }
    }

    let Some((group_id, idx, child_id, child_is_tabs)) = offender else {
        return false;
    };

    if child_is_tabs {
        flatten_nested_tabs(tree, group_id, idx, child_id);
    } else {
        lift_nested_split(tree, group_id, child_id, prev_group_of);
    }
    true
}

/// `Tabs[.. , Tabs[x, y], ..]` → `Tabs[.., x, y, ..]`: a tab group dropped
/// into another tab just merges its tabs into the host group.
fn flatten_nested_tabs(tree: &mut Tree<TabId>, group_id: TileId, idx: usize, child_id: TileId) {
    let Some(Tile::Container(Container::Tabs(inner))) = tree.tiles.remove(child_id) else {
        return;
    };
    let inner_children = inner.children;
    if let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get_mut(group_id) {
        let first_inner = inner_children.first().copied();
        tabs.children.splice(idx..=idx, inner_children);
        if tabs.active == Some(child_id) {
            tabs.active = first_inner;
        }
    }
}

/// `Tabs[loose.., split, loose..]` → lift `split` to the group's own level,
/// merging the loose panes into the split leaf that kept their pre-drop
/// siblings. `split` is a `Linear`/`Grid` child of the `Tabs`.
fn lift_nested_split(
    tree: &mut Tree<TabId>,
    group_id: TileId,
    split_id: TileId,
    prev_group_of: &HashMap<TabId, TileId>,
) {
    // Loose pane children of the group, partitioned by side of the split so
    // the merged tab order reads naturally.
    let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get(group_id) else {
        return;
    };
    let Some(split_pos) = tabs.children.iter().position(|c| *c == split_id) else {
        return;
    };
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (i, child) in tabs.children.iter().enumerate() {
        if matches!(tree.tiles.get(*child), Some(Tile::Pane(_))) {
            if i < split_pos {
                before.push(*child);
            } else {
                after.push(*child);
            }
        }
    }

    // Pick the split leaf the loose panes belong to: the first leaf `Tabs`
    // holding a pane that lived in this group before the drop.
    let leaves = leaf_tab_groups(tree, split_id);
    let target = leaves
        .iter()
        .copied()
        .find(|leaf| {
            handles_in_group(tree, *leaf)
                .iter()
                .any(|h| prev_group_of.get(h) == Some(&group_id))
        })
        .or_else(|| leaves.first().copied());

    if let Some(target) = target {
        if let Some(Tile::Container(Container::Tabs(leaf))) = tree.tiles.get_mut(target) {
            // before-panes ahead of the leaf's tabs, after-panes behind them.
            let mut merged = before;
            merged.append(&mut leaf.children);
            merged.extend(after);
            leaf.children = merged;
        }
    }

    // Replace the now-redundant group with the lifted split.
    match tree.tiles.parent_of(group_id) {
        None => tree.root = Some(split_id),
        Some(parent) => {
            if let Some(Tile::Container(container)) = tree.tiles.get_mut(parent) {
                replace_child(container, group_id, split_id);
            }
        }
    }
    tree.tiles.remove(group_id);
}

/// Replace `old` with `new` in a container's child list, in place.
fn replace_child(container: &mut Container, old: TileId, new: TileId) {
    match container {
        Container::Tabs(tabs) => {
            for c in &mut tabs.children {
                if *c == old {
                    *c = new;
                }
            }
            if tabs.active == Some(old) {
                tabs.active = Some(new);
            }
        }
        Container::Linear(linear) => {
            for c in &mut linear.children {
                if *c == old {
                    *c = new;
                }
            }
            linear.shares.replace_with(old, new);
        }
        Container::Grid(grid) => {
            let idx = grid.children().position(|c| *c == old);
            if let Some(idx) = idx {
                let _ = grid.replace_at(idx, new);
            }
        }
    }
}

/// Leaf `Tabs` containers reachable under `root`, in traversal order.
fn leaf_tab_groups(tree: &Tree<TabId>, root: TileId) -> Vec<TileId> {
    let mut out = Vec::new();
    fn walk(tree: &Tree<TabId>, id: TileId, out: &mut Vec<TileId>) {
        match tree.tiles.get(id) {
            Some(Tile::Container(Container::Tabs(_))) => out.push(id),
            Some(Tile::Container(c)) => {
                for child in c.children().copied().collect::<Vec<_>>() {
                    walk(tree, child, out);
                }
            }
            _ => {}
        }
    }
    walk(tree, root, &mut out);
    out
}

/// Resolve the active tab handle inside the given Tabs container.
pub(crate) fn active_handle_in_group<P>(tree: &Tree<P>, group: TileId) -> Option<TabId>
where
    P: Copy + Into<TabId>,
{
    let Some(Tile::Container(egui_tiles::Container::Tabs(tabs))) = tree.tiles.get(group) else {
        return None;
    };
    let active = tabs.active?;
    let Tile::Pane(pane) = tree.tiles.get(active)? else {
        return None;
    };
    Some((*pane).into())
}

#[cfg(test)]
mod normalize_tests {
    use super::*;
    use egui_tiles::{LinearDir, Tiles, Tree};

    fn h(n: u64) -> TabId {
        TabId(n)
    }

    /// Top-level shape: `Kind { group with handles, ... }`.
    fn shape(tree: &Tree<TabId>) -> Vec<(String, Vec<u64>)> {
        let root = tree.root().unwrap();
        let Some(Tile::Container(c)) = tree.tiles.get(root) else {
            // single group
            return vec![("Tabs".into(), handles_in_group(tree, root).iter().map(|t| t.0).collect())];
        };
        if let Container::Tabs(_) = c {
            return vec![("Tabs".into(), handles_in_group(tree, root).iter().map(|t| t.0).collect())];
        }
        c.children()
            .copied()
            .collect::<Vec<_>>()
            .iter()
            .map(|leaf| {
                let kind = match tree.tiles.get(*leaf) {
                    Some(Tile::Container(Container::Tabs(_))) => "Tabs",
                    _ => "?",
                };
                (kind.to_string(), handles_in_group(tree, *leaf).iter().map(|t| t.0).collect())
            })
            .collect()
    }

    fn root_kind(tree: &Tree<TabId>) -> &'static str {
        match tree.tiles.get(tree.root().unwrap()) {
            Some(Tile::Container(Container::Tabs(_))) => "Tabs",
            Some(Tile::Container(Container::Linear(l))) => match l.dir {
                LinearDir::Vertical => "Vertical",
                LinearDir::Horizontal => "Horizontal",
            },
            Some(Tile::Container(Container::Grid(_))) => "Grid",
            _ => "?",
        }
    }

    // Tabs[a, Vertical{Tabs[b], Tabs[c]}] with a,b from the original group and
    // c the newcomer (bottom drop) → Vertical{Tabs[a,b], Tabs[c]}.
    #[test]
    fn bottom_drop_lifts_split_and_regroups() {
        let mut tiles = Tiles::default();
        let a = tiles.insert_pane(h(1));
        let b = tiles.insert_pane(h(2));
        let c = tiles.insert_pane(h(3));
        let gb = tiles.insert_tab_tile(vec![b]);
        let gc = tiles.insert_tab_tile(vec![c]);
        let split = tiles.insert_vertical_tile(vec![gb, gc]);
        let group = tiles.insert_tab_tile(vec![a, split]);
        let mut tree = Tree::new("t", group, tiles);

        let mut prev = HashMap::new();
        prev.insert(h(1), group);
        prev.insert(h(2), group);
        prev.insert(h(3), TileId::from_u64(9999)); // newcomer, elsewhere

        assert!(normalize_tab_groups(&mut tree, &prev));
        assert_eq!(root_kind(&tree), "Vertical");
        assert_eq!(
            shape(&tree),
            vec![
                ("Tabs".to_string(), vec![1, 2]),
                ("Tabs".to_string(), vec![3]),
            ]
        );
    }

    // Top drop puts the newcomer first: Tabs[a, Vertical{Tabs[c], Tabs[b]}].
    // `a` must rejoin `b` (its pre-drop sibling), not `c`.
    #[test]
    fn top_drop_regroups_with_original_sibling() {
        let mut tiles = Tiles::default();
        let a = tiles.insert_pane(h(1));
        let b = tiles.insert_pane(h(2));
        let c = tiles.insert_pane(h(3));
        let gc = tiles.insert_tab_tile(vec![c]);
        let gb = tiles.insert_tab_tile(vec![b]);
        let split = tiles.insert_vertical_tile(vec![gc, gb]);
        let group = tiles.insert_tab_tile(vec![a, split]);
        let mut tree = Tree::new("t", group, tiles);

        let mut prev = HashMap::new();
        prev.insert(h(1), group);
        prev.insert(h(2), group);
        prev.insert(h(3), TileId::from_u64(9999));

        assert!(normalize_tab_groups(&mut tree, &prev));
        assert_eq!(
            shape(&tree),
            vec![
                ("Tabs".to_string(), vec![3]),
                ("Tabs".to_string(), vec![1, 2]),
            ]
        );
    }

    // Tabs[a, Tabs[b, c]] (a tab group dropped into a tab) → Tabs[a, b, c].
    #[test]
    fn nested_tab_group_flattens() {
        let mut tiles = Tiles::default();
        let a = tiles.insert_pane(h(1));
        let b = tiles.insert_pane(h(2));
        let c = tiles.insert_pane(h(3));
        let inner = tiles.insert_tab_tile(vec![b, c]);
        let group = tiles.insert_tab_tile(vec![a, inner]);
        let mut tree = Tree::new("t", group, tiles);

        assert!(normalize_tab_groups(&mut tree, &HashMap::new()));
        assert_eq!(root_kind(&tree), "Tabs");
        assert_eq!(handles_in_group(&tree, tree.root().unwrap()).iter().map(|t| t.0).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    // A well-formed split is left untouched.
    #[test]
    fn clean_split_is_a_noop() {
        let mut tiles = Tiles::default();
        let a = tiles.insert_pane(h(1));
        let b = tiles.insert_pane(h(2));
        let ga = tiles.insert_tab_tile(vec![a]);
        let gb = tiles.insert_tab_tile(vec![b]);
        let split = tiles.insert_vertical_tile(vec![ga, gb]);
        let mut tree = Tree::new("t", split, tiles);
        assert!(!normalize_tab_groups(&mut tree, &HashMap::new()));
    }
}
