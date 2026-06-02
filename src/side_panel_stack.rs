//! Stacked, collapsible primary side-panel sections (VSCode-style
//! accordion).
//!
//! The primary side bar holds an ordered list of feature *sections*.
//! Each section has a header (twistie + title + host actions) and, when
//! expanded, a body rendered by the host's [`Host::side_bar_ui`].
//!
//! Interaction model — modelled on VSCode's primary sidebar, **not** on
//! editor tabs:
//! - An activity-bar click *switches*: it focuses the section if open,
//!   otherwise replaces the focused section in place. It never adds a
//!   split. (The workbench owns the click→switch / click-active→hide
//!   wiring; this module just exposes [`SidePanelStack::switch`].)
//! - Multiple sections stack vertically as collapsible accordion rows.
//!   Clicking a header toggles its twistie; dragging a header reorders
//!   it; the boundary between two expanded sections is a resize handle.
//! - A header's `+` opens the "add panel" menu (modes not yet open),
//!   so a second section is opt-in rather than the click default.
//!
//! There is no tab strip and no tile chrome — a lone section looks
//! identical to the pre-accordion single side bar. [feature-multi-region-sidebar]

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use egui::{Color32, CursorIcon, Layout, Rect, Sense, Stroke, vec2};

use crate::behavior::Host;
use crate::tab::Document;
use crate::theme::Palette;

/// Height of a section header row (the drag handle + twistie).
const HEADER_HEIGHT: f32 = 26.0;
/// An expanded section's body never shrinks below this via resize; to go
/// smaller the user collapses the section with its twistie.
const MIN_BODY: f32 = 48.0;
/// Hit thickness of the inter-section resize handle.
const RESIZE_GRAB: f32 = 5.0;

/// A remembered split arrangement, stashed when the user switches away
/// from it so re-selecting its anchor activity brings the split back.
#[derive(Clone)]
struct SavedGroup<Mode> {
    sections: Vec<Mode>,
    collapsed: HashSet<Mode>,
    weights: HashMap<Mode, f32>,
}

/// The splittable primary side region as a vertical accordion of
/// feature sections. Generic over `Mode` (the host's activity id).
pub struct SidePanelStack<Mode> {
    /// Open sections, top to bottom.
    sections: Vec<Mode>,
    /// Sections whose body is hidden (header only).
    collapsed: HashSet<Mode>,
    /// Relative height weight per expanded section (default 1.0). Only
    /// the ratios between currently-expanded sections matter.
    weights: HashMap<Mode, f32>,
    /// The section the user last focused — drives the activity-bar
    /// highlight. `None` when empty.
    pub(crate) focused: Option<Mode>,
    /// Multi-section arrangements remembered by anchor (top) section.
    /// Switching away from a split stashes it here; clicking the anchor
    /// activity again restores it (VSCode-style view-container memory).
    /// In-memory only — not persisted across sessions.
    saved_groups: HashMap<Mode, SavedGroup<Mode>>,
    /// Opaque label distinguishing this stack from its sibling so a
    /// *cross-stack* header drag (a section dragged from the left stack
    /// onto the right, or vice versa) can tell whose drag it is. Within a
    /// single stack the within-stack reorder path (a bare `usize` index in
    /// this stack's own ui memory) is used instead; this tag only matters
    /// when a foreign stack inspects the shared, global drag payload. The
    /// host assigns each of its stacks a distinct value (e.g. `0` = left,
    /// `1` = right) via [`Self::set_location_tag`]. Defaults to `0`.
    /// [feature-multi-region-sidebar]
    location_tag: u8,
}

/// Globally-scoped section-drag payload, published in shared `ctx.data`
/// while a header drag is in flight so the *other* stack can recognise a
/// section being dragged onto it (the within-stack reorder uses a bare
/// `usize` index scoped to its own `ui.id()`, which a foreign stack can't
/// interpret — hence this carries the view id and the source stack's tag).
/// [feature-multi-region-sidebar]
#[derive(Clone)]
struct SectionDragPayload<Mode> {
    /// `location_tag` of the stack the drag started in.
    source_tag: u8,
    /// The view id being dragged.
    view_id: Mode,
}

/// Shared id under which the global section-drag payload lives in
/// `ctx.data`. One id for both stacks (the payload's `source_tag`
/// disambiguates), so either can read the other's in-flight drag.
fn section_drag_payload_id() -> egui::Id {
    egui::Id::new("egui_workbench::section_drag_payload")
}

/// A committed cross-stack section move, reported out of [`SidePanelStack::ui`]
/// for the owner of both stacks to reconcile: the dragged view was released
/// over THIS stack but originated in the sibling identified by `source_tag`.
/// The owner removes `view_id` from the source stack and adds it here.
/// [feature-multi-region-sidebar]
pub(crate) struct CrossStackDrop<Mode> {
    /// `location_tag` of the stack the section came from.
    pub source_tag: u8,
    /// The view id to move onto the target (this) stack.
    pub view_id: Mode,
}

impl<Mode: Clone + Eq + Hash + 'static> Default for SidePanelStack<Mode> {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            collapsed: HashSet::new(),
            weights: HashMap::new(),
            focused: None,
            saved_groups: HashMap::new(),
            location_tag: 0,
        }
    }
}

impl<Mode: Clone + Eq + Hash + 'static> SidePanelStack<Mode> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set this stack's cross-stack drag tag (see [`Self::location_tag`]).
    /// The host gives its two stacks distinct values so a section dragged
    /// from one onto the other can be recognised as a cross-stack move.
    /// [feature-multi-region-sidebar]
    pub const fn set_location_tag(&mut self, tag: u8) {
        self.location_tag = tag;
    }

    /// Whether any section is open.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Whether `mode` is currently an open section.
    pub fn contains(&self, mode: &Mode) -> bool {
        self.sections.iter().any(|m| m == mode)
    }

    /// Open sections in top-to-bottom order (for persistence).
    pub fn open_modes(&self) -> &[Mode] {
        &self.sections
    }

    /// Collapsed sections, in section order (for persistence). Iterates
    /// `sections` rather than the set so the result is deterministic.
    pub fn collapsed_modes(&self) -> Vec<Mode> {
        self.sections
            .iter()
            .filter(|m| self.collapsed.contains(*m))
            .cloned()
            .collect()
    }

    /// Height weight for `mode` (1.0 default). For persistence.
    pub fn section_weight(&self, mode: &Mode) -> f32 {
        self.weight(mode)
    }

    /// The focused section, if any.
    pub const fn focused(&self) -> Option<&Mode> {
        self.focused.as_ref()
    }

    /// Restore a persisted arrangement: `sections` top-to-bottom,
    /// `collapsed` naming the collapsed ones, `weights` the per-section
    /// height weights, `focused` the focused section. Stale entries (not
    /// in `sections`) are dropped; focus falls back to the first section.
    pub fn restore(
        &mut self,
        sections: Vec<Mode>,
        collapsed: Vec<Mode>,
        weights: Vec<(Mode, f32)>,
        focused: Option<Mode>,
    ) {
        self.collapsed = collapsed.into_iter().filter(|m| sections.contains(m)).collect();
        self.weights = weights.into_iter().filter(|(m, _)| sections.contains(m)).collect();
        self.focused = focused
            .filter(|m| sections.contains(m))
            .or_else(|| sections.first().cloned());
        self.sections = sections;
    }

    fn index_of(&self, mode: &Mode) -> Option<usize> {
        self.sections.iter().position(|m| m == mode)
    }

    fn weight(&self, mode: &Mode) -> f32 {
        self.weights.get(mode).copied().unwrap_or(1.0)
    }

    /// Activity-bar *switch* semantics (VSCode view-container switch): if
    /// `mode` is already an open section, focus it and keep the current
    /// arrangement. Otherwise the current arrangement is stashed under its
    /// anchor (top) section, and `mode`'s **remembered split is restored**
    /// — or, if it has none, `mode` is opened in full (the stack becomes
    /// just that one section). So clicking an activity that previously
    /// anchored a split brings the split back; clicking a fresh activity
    /// opens it solo. A click never adds to a split; extra sections are
    /// built with [`Self::add_section`] (the `+` menu or dragging an icon
    /// in). Always focuses `mode`.
    pub fn switch(&mut self, mode: Mode) {
        self.switch_group(mode.clone(), vec![mode]);
    }

    /// Generalised [`Self::switch`] from one mode to an ordered *group* of
    /// views sharing a container. `group` is the container id the
    /// arrangement is remembered under (for a single-view container the
    /// group id equals the lone view id, so this is byte-identical to the
    /// old `switch`).
    ///
    /// If any of `views` is already an open section, that arrangement is
    /// kept and the first already-open view is focused. Otherwise the
    /// current arrangement is stashed (under its own anchor) and the
    /// container's **remembered group is restored** — or, failing that,
    /// exactly `views` is opened (focusing the first). A switch never adds
    /// to an existing split; extra sections come from [`Self::add_section`].
    pub fn switch_group(&mut self, group: Mode, views: Vec<Mode>) {
        // Already-open: focus the first open view, keep the arrangement.
        if let Some(open) = views.iter().find(|v| self.contains(v)) {
            self.collapsed.remove(open);
            self.focused = Some(open.clone());
            return;
        }
        self.stash_current();
        if let Some(saved) = self.saved_groups.remove(&group) {
            self.sections = saved.sections;
            self.collapsed = saved.collapsed;
            self.weights = saved.weights;
        } else {
            self.sections = views.clone();
            self.collapsed = HashSet::new();
            self.weights = HashMap::new();
        }
        let focus = self.sections.first().cloned().or_else(|| views.first().cloned());
        if let Some(f) = &focus {
            self.collapsed.remove(f);
        }
        self.focused = focus;
    }

    /// Stash the current arrangement under its anchor (top) section, so a
    /// later [`Self::switch`] back to that activity restores the split.
    /// Only real splits (2+ sections) are worth remembering.
    fn stash_current(&mut self) {
        if self.sections.len() < 2 {
            return;
        }
        let Some(anchor) = self.sections.first().cloned() else {
            return;
        };
        self.saved_groups.insert(
            anchor,
            SavedGroup {
                sections: self.sections.clone(),
                collapsed: self.collapsed.clone(),
                weights: self.weights.clone(),
            },
        );
    }

    /// Add `mode` as a new section below the focused one (the `+` "add
    /// panel" path). Focuses it. No-op-but-focus if already open.
    pub fn add_section(&mut self, mode: Mode) {
        self.collapsed.remove(&mode);
        if self.contains(&mode) {
            self.focused = Some(mode);
            return;
        }
        let at = self
            .focused
            .as_ref()
            .and_then(|f| self.index_of(f))
            .map_or(self.sections.len(), |i| i + 1);
        self.sections.insert(at, mode.clone());
        self.focused = Some(mode);
    }

    /// Remove `mode`'s section (no-op when absent). Refocuses a neighbour.
    pub fn close(&mut self, mode: &Mode) {
        let Some(i) = self.index_of(mode) else { return };
        self.sections.remove(i);
        self.collapsed.remove(mode);
        self.weights.remove(mode);
        if self.focused.as_ref() == Some(mode) {
            let fallback = i.min(self.sections.len().saturating_sub(1));
            self.focused = self.sections.get(fallback).cloned();
        }
    }

    /// Replace the open set wholesale (layout restore). First mode is
    /// focused; collapse/weight state for dropped modes is discarded.
    pub fn set_open(&mut self, modes: &[Mode]) {
        self.sections = modes.to_vec();
        self.collapsed.retain(|m| self.sections.contains(m));
        self.weights.retain(|m, _| self.sections.contains(m));
        self.focused = modes.first().cloned();
    }

    fn is_collapsed(&self, mode: &Mode) -> bool {
        self.collapsed.contains(mode)
    }

    fn toggle_collapsed(&mut self, mode: &Mode) {
        if self.collapsed.contains(mode) {
            self.collapsed.remove(mode);
        } else {
            self.collapsed.insert(mode.clone());
        }
    }

    /// Render the accordion into `ui`. Returns the section whose header
    /// was activated this frame (so the workbench can sync the
    /// activity-bar highlight), or `None`.
    ///
    /// `cross_drop` is an out-param: when a section dragged FROM the
    /// sibling stack is released over this one, the pending move is written
    /// here for the owner of both stacks to reconcile (this stack can't
    /// reach into its sibling). It stays `None` for the common
    /// within-stack reorder, which commits internally as before.
    /// [feature-multi-region-sidebar]
    pub(crate) fn ui<Tab, B>(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Palette,
        behavior: &mut B,
        cross_drop: &mut Option<CrossStackDrop<Mode>>,
    ) -> Option<Mode>
    where
        Tab: Document,
        B: Host<Tab, Mode> + ?Sized,
        // `Send + Sync` only here (and on `Render`): the cross-stack drag
        // payload is published via egui's `insert_temp`, which requires it.
        // Every concrete host `Mode` (an id enum / `String`) satisfies it.
        Mode: Send + Sync,
    {
        // Even with no open sections this stack can still be the *target*
        // of a cross-stack drop (drag the last view off one side onto the
        // empty other side). Handle that before the empty-state early out.
        if self.sections.is_empty() {
            self.paint_empty_drop_target(ui, cross_drop);
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No panel open").weak());
            });
            return None;
        }
        let render = Render {
            stack: self,
            theme,
            behavior,
            clicked: None,
            add_request: None,
            close_request: None,
            cross_drop,
            _doc: std::marker::PhantomData::<Tab>,
        };
        render.run(ui)
    }

    /// When this stack has no sections it allocates no geometry, so the
    /// `Render` drop-target path never runs; mirror it here so an empty
    /// stack still lights up and accepts a foreign section-drag release.
    /// [feature-multi-region-sidebar]
    fn paint_empty_drop_target(
        &self,
        ui: &mut egui::Ui,
        cross_drop: &mut Option<CrossStackDrop<Mode>>,
    ) {
        let outer = ui.available_rect_before_wrap();
        ui.allocate_rect(outer, Sense::hover());
        let foreign = foreign_section_drag::<Mode>(ui, self.location_tag);
        let hovering = ui
            .input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()))
            .is_some_and(|p| outer.contains(p));
        if let Some(payload) = &foreign {
            if hovering {
                let painter = ui.painter();
                let accent = ui.visuals().selection.bg_fill;
                painter.rect_filled(outer, 0.0, accent.gamma_multiply(0.10));
            }
            ui.ctx().request_repaint();
            if hovering && ui.input(|i| i.pointer.any_released()) {
                *cross_drop = Some(CrossStackDrop {
                    source_tag: payload.source_tag,
                    view_id: payload.view_id.clone(),
                });
                clear_section_drag_payload::<Mode>(ui);
            }
        }
    }
}

/// Read the global section-drag payload, returning it only when it
/// originated in a DIFFERENT stack than `self_tag` (a genuine cross-stack
/// drag). A payload whose `source_tag == self_tag` is this stack's own
/// drag — handled by the within-stack reorder path — so it's filtered out.
/// [feature-multi-region-sidebar]
fn foreign_section_drag<Mode: Clone + 'static>(
    ui: &egui::Ui,
    self_tag: u8,
) -> Option<SectionDragPayload<Mode>> {
    ui.ctx()
        .data(|d| d.get_temp::<SectionDragPayload<Mode>>(section_drag_payload_id()))
        .filter(|p| p.source_tag != self_tag)
}

/// Drop the global section-drag payload once a cross-stack move commits.
fn clear_section_drag_payload<Mode: 'static>(ui: &egui::Ui) {
    ui.ctx()
        .data_mut(|d| d.remove::<SectionDragPayload<Mode>>(section_drag_payload_id()));
}

/// Per-frame render state, split into `&mut self` methods so each stays
/// inside the cognitive-complexity budget while sharing the stack, theme
/// and host without a wide free-function signature.
struct Render<'a, Tab, Mode, B>
where
    Tab: Document,
    Mode: Clone + Eq + Hash + 'static,
    B: Host<Tab, Mode> + ?Sized,
{
    stack: &'a mut SidePanelStack<Mode>,
    theme: &'a Palette,
    behavior: &'a mut B,
    clicked: Option<Mode>,
    /// A `+`-menu pick to apply after the render pass.
    add_request: Option<Mode>,
    /// A header-menu "Close panel" pick to apply after the render pass.
    close_request: Option<Mode>,
    /// Out-channel for a cross-stack section drop landed on this stack
    /// (reconciled by the owner of both stacks). [feature-multi-region-sidebar]
    cross_drop: &'a mut Option<CrossStackDrop<Mode>>,
    _doc: std::marker::PhantomData<Tab>,
}

impl<Tab, Mode, B> Render<'_, Tab, Mode, B>
where
    Tab: Document,
    // `Send + Sync`: `render_header` publishes the global cross-stack drag
    // payload via egui's `insert_temp`. Constructed only from
    // `SidePanelStack::ui`, which carries the same bound.
    Mode: Clone + Eq + Hash + Send + Sync + 'static,
    B: Host<Tab, Mode> + ?Sized,
{
    fn run(mut self, ui: &mut egui::Ui) -> Option<Mode> {
        let outer = ui.available_rect_before_wrap();
        // Claim the whole region up front: the sections paint via the
        // painter / child UIs and never allocate in this `ui`, so without
        // this the enclosing `SidePanel` sees empty content and collapses
        // to its min width.
        ui.allocate_rect(outer, Sense::hover());
        let layout = self.compute_layout(outer);
        for (i, geom) in layout.iter().enumerate() {
            self.render_section(ui, i, geom);
        }
        self.resize_handles(ui, &layout);
        self.reorder(ui, &layout, outer);
        self.paint_drop_target(ui, outer);
        if let Some(mode) = self.add_request.take() {
            self.stack.add_section(mode);
        }
        if let Some(mode) = self.close_request.take() {
            self.stack.close(&mode);
        }
        if let Some(m) = &self.clicked {
            self.stack.focused = Some(m.clone());
        }
        self.clicked.clone()
    }

    /// Compute each section's header + body rects. Collapsed sections get
    /// only a header; expanded sections share the remaining height by
    /// weight.
    fn compute_layout(&self, outer: Rect) -> Vec<SectionGeom> {
        let n = self.stack.sections.len();
        let expanded: Vec<&Mode> = self
            .stack
            .sections
            .iter()
            .filter(|m| !self.stack.is_collapsed(m))
            .collect();
        let weight_sum: f32 = expanded
            .iter()
            .map(|m| self.stack.weight(m))
            .sum::<f32>()
            .max(f32::EPSILON);
        let body_total = (outer.height() - n as f32 * HEADER_HEIGHT).max(0.0);

        let mut geoms = Vec::with_capacity(n);
        let mut y = outer.top();
        for mode in &self.stack.sections {
            let header = Rect::from_min_size(
                egui::pos2(outer.left(), y),
                vec2(outer.width(), HEADER_HEIGHT),
            );
            y += HEADER_HEIGHT;
            let body = if self.stack.is_collapsed(mode) {
                None
            } else {
                let h = body_total * self.stack.weight(mode) / weight_sum;
                let r = Rect::from_min_size(egui::pos2(outer.left(), y), vec2(outer.width(), h));
                y += h;
                Some(r)
            };
            geoms.push(SectionGeom { header, body });
        }
        geoms
    }

    fn render_section(&mut self, ui: &mut egui::Ui, idx: usize, geom: &SectionGeom) {
        let mode = self.stack.sections[idx].clone();
        let collapsed = self.stack.is_collapsed(&mode);
        self.render_header(ui, idx, &mode, geom.header, collapsed);
        if let Some(body) = geom.body {
            let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
            body_ui.set_clip_rect(body);
            self.behavior.side_bar_ui(&mut body_ui, &mode);
        }
    }

    /// Header: twistie + title (the drag handle + collapse toggle) on the
    /// left and the host's action buttons on the right. All headers look
    /// identical (no focus highlight) so an added section reads as a
    /// peer, not a selected sub-item. Right-click opens the panel menu
    /// (host actions + Add panel + Close panel).
    fn render_header(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        mode: &Mode,
        rect: Rect,
        collapsed: bool,
    ) {
        let resp = ui
            .interact(rect, header_id(ui, idx), Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::Grab);
        if resp.drag_started() {
            // Within-stack reorder path: a bare index scoped to THIS
            // stack's ui id (precise; unchanged).
            let src_id = ui.id().with("egui_workbench::section_drag_src");
            ui.memory_mut(|m| m.data.insert_temp::<usize>(src_id, idx));
            // Cross-stack path: ALSO publish a global payload carrying the
            // view id + this stack's tag, so the sibling stack can light up
            // and accept the section if the drag is released over it. The
            // index path above still drives any same-stack reorder.
            // [feature-multi-region-sidebar]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    section_drag_payload_id(),
                    SectionDragPayload {
                        source_tag: self.stack.location_tag,
                        view_id: mode.clone(),
                    },
                );
            });
        }

        if ui.is_rect_visible(rect) {
            ui.painter().rect_filled(rect, 0.0, self.theme.side_bar_bg);
            ui.painter().hline(
                rect.x_range(),
                rect.bottom(),
                (1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
            paint_twistie(ui, rect, collapsed, ui.visuals().text_color());
            let title = self.behavior.side_bar_title(mode);
            let galley = title.into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                (rect.width() - 50.0).max(0.0),
                egui::TextStyle::Button.resolve(ui.style()),
            );
            ui.painter().galley(
                egui::pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0),
                galley,
                ui.visuals().text_color(),
            );
            // Host action buttons in a small right cluster (e.g. Files'
            // new-note). Rendered after the drag interact so their clicks
            // win over it (matches the activity-bar pattern).
            let cluster = Rect::from_min_max(
                egui::pos2(rect.right() - 72.0, rect.top()),
                rect.right_bottom(),
            );
            let mut right = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(cluster)
                    .layout(Layout::right_to_left(egui::Align::Center)),
            );
            self.behavior.side_bar_action_buttons(&mut right, mode);
        }

        self.header_context_menu(&resp, mode);

        // A plain click toggles the twistie + focuses; a click that lands
        // on a host action button is consumed there first.
        if resp.clicked() {
            self.stack.toggle_collapsed(mode);
            self.clicked = Some(mode.clone());
        }
    }

    /// Right-click panel menu for a section header: the host's per-panel
    /// actions (what the old `…` button opened), an "Add panel" submenu
    /// (activities not yet open), and "Close panel". Picks are deferred
    /// to local vars to avoid borrowing `self` inside nested menus.
    fn header_context_menu(&mut self, resp: &egui::Response, mode: &Mode) {
        let addable: Vec<(Mode, String)> = self
            .behavior
            .activity_items()
            .into_iter()
            .filter(|it| !self.stack.contains(&it.mode))
            .map(|it| {
                let label = if it.label.is_empty() { "(unnamed)".to_string() } else { it.label };
                (it.mode, label)
            })
            .collect();
        let mode_cl = mode.clone();
        let mut add_pick: Option<Mode> = None;
        let mut close_pick = false;
        resp.context_menu(|ui| {
            self.behavior.side_bar_actions_menu(ui, &mode_cl);
            if !addable.is_empty() {
                ui.menu_button("Add panel", |ui| {
                    for (m, label) in &addable {
                        if ui.button(label).clicked() {
                            add_pick = Some(m.clone());
                            ui.close();
                        }
                    }
                });
            }
            ui.separator();
            if ui.button("Close panel").clicked() {
                close_pick = true;
                ui.close();
            }
        });
        if let Some(m) = add_pick {
            self.add_request = Some(m);
        }
        if close_pick {
            self.close_request = Some(mode.clone());
        }
    }

    /// Draw a resize handle at the bottom of each expanded section that
    /// has another expanded section below it, transferring height weight
    /// between the two as it is dragged.
    fn resize_handles(&mut self, ui: &mut egui::Ui, layout: &[SectionGeom]) {
        let expanded: Vec<usize> = (0..self.stack.sections.len())
            .filter(|&i| layout[i].body.is_some())
            .collect();
        let body_total: f32 = expanded.iter().map(|&i| layout[i].body.unwrap().height()).sum();
        let weight_sum: f32 = expanded
            .iter()
            .map(|&i| self.stack.weight(&self.stack.sections[i]))
            .sum::<f32>()
            .max(f32::EPSILON);
        for pair in expanded.windows(2) {
            let (upper, lower) = (pair[0], pair[1]);
            let body = layout[upper].body.unwrap();
            let handle = Rect::from_min_size(
                egui::pos2(body.left(), body.bottom() - RESIZE_GRAB / 2.0),
                vec2(body.width(), RESIZE_GRAB),
            );
            let id = ui.id().with(("egui_workbench::section_resize", upper));
            let resp = ui
                .interact(handle, id, Sense::drag())
                .on_hover_cursor(CursorIcon::ResizeVertical);
            if resp.dragged() && body_total > 0.0 {
                let dy = resp.drag_delta().y;
                let dw = dy / body_total * weight_sum;
                self.transfer_weight(upper, lower, dw, body_total, weight_sum);
            }
        }
    }

    /// Move `dw` of height weight from the `lower` section to the `upper`
    /// one (positive `dw` grows the upper), clamped so neither expanded
    /// body drops below `MIN_BODY`.
    fn transfer_weight(
        &mut self,
        upper: usize,
        lower: usize,
        dw: f32,
        body_total: f32,
        weight_sum: f32,
    ) {
        let up_mode = self.stack.sections[upper].clone();
        let lo_mode = self.stack.sections[lower].clone();
        let min_w = MIN_BODY / body_total * weight_sum;
        let up_w = self.stack.weight(&up_mode);
        let lo_w = self.stack.weight(&lo_mode);
        // Range of `dw` that keeps both bodies at/above `min_w`. When the
        // window is too short for two min-height bodies the bounds invert
        // — there's no room to resize, so bail (avoids a `clamp` panic).
        let (lo_bound, hi_bound) = (min_w - up_w, lo_w - min_w);
        if lo_bound > hi_bound {
            return;
        }
        let dw = dw.clamp(lo_bound, hi_bound);
        self.stack.weights.insert(up_mode, up_w + dw);
        self.stack.weights.insert(lo_mode, lo_w - dw);
    }

    /// Drag-to-reorder of section headers. The dragged index was stashed
    /// in ui memory by `render_header`; here we preview an insertion line
    /// and commit the move on release. Kept lean — sections are few.
    fn reorder(&mut self, ui: &mut egui::Ui, layout: &[SectionGeom], outer: Rect) {
        let src_id = ui.id().with("egui_workbench::section_drag_src");
        let Some(src) = ui.memory(|m| m.data.get_temp::<usize>(src_id)) else {
            return;
        };
        let pointer = ui.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
        let target = pointer.map(|p| self.target_index(p.y, layout));
        if let Some(tgt) = target {
            self.paint_insertion_line(ui, layout, tgt);
            ui.ctx().request_repaint();
        }
        if ui.input(|i| i.pointer.any_released()) {
            // Only reorder if the release landed over THIS stack — a release
            // outside it (over the sibling stack) is a cross-stack move,
            // handled by the target stack's `paint_drop_target`, so leave
            // the sections here untouched and let the owner remove the view.
            let released_here = pointer.is_some_and(|p| outer.contains(p));
            if released_here
                && let Some(tgt) = target
                && tgt != src
                && src < self.stack.sections.len()
            {
                let m = self.stack.sections.remove(src);
                let tgt = tgt.min(self.stack.sections.len());
                self.stack.sections.insert(tgt, m);
            }
            ui.memory_mut(|m| m.data.remove::<usize>(src_id));
            // Drop the global payload only on a release that landed back
            // inside THIS stack. If the release was elsewhere it may be a
            // cross-stack move onto the sibling — leave the payload so the
            // target stack's `paint_drop_target` can still read and consume
            // it this frame (it clears it on commit). A release into dead
            // space leaves a stale payload, but the next drag overwrites it
            // and no stack will accept it (pointer isn't over either).
            if released_here {
                clear_section_drag_payload::<Mode>(ui);
            }
        }
    }

    /// Draw a drop-target highlight + accept releases when something is
    /// being dragged onto this region. Two drag sources qualify:
    /// - an **activity-bar icon** mid-drag (the workbench turns its
    ///   release into an `add_section`), keyed on the activity-bar flag;
    /// - a **section header from the sibling stack** (a cross-stack move),
    ///   recognised via the global section-drag payload whose `source_tag`
    ///   differs from this stack's. On release of the latter the pending
    ///   move is written to `self.cross_drop` for the owner to reconcile.
    ///
    /// [feature-multi-region-sidebar]
    fn paint_drop_target(&mut self, ui: &egui::Ui, outer: Rect) {
        let icon_dragging = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(crate::activity_bar::drag_active_id()))
            .unwrap_or(false);
        let foreign = foreign_section_drag::<Mode>(ui, self.stack.location_tag);
        let dragging = icon_dragging || foreign.is_some();
        let hovering = ui
            .input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()))
            .is_some_and(|p| outer.contains(p));
        if dragging && hovering {
            let painter = ui.painter();
            painter.rect_filled(outer, 0.0, self.theme.accent.gamma_multiply(0.10));
            painter.rect_stroke(
                outer.shrink(1.0),
                2.0,
                Stroke::new(2.0, self.theme.accent),
                egui::StrokeKind::Inside,
            );
        }
        // Commit a cross-stack section drop released over this stack.
        if let Some(payload) = foreign {
            ui.ctx().request_repaint();
            if hovering && ui.input(|i| i.pointer.any_released()) {
                *self.cross_drop = Some(CrossStackDrop {
                    source_tag: payload.source_tag,
                    view_id: payload.view_id.clone(),
                });
                clear_section_drag_payload::<Mode>(ui);
            }
        }
    }

    /// Insertion index for a pointer at height `y` — the first section
    /// whose header midpoint sits below `y`, else past the end.
    fn target_index(&self, y: f32, layout: &[SectionGeom]) -> usize {
        for (i, geom) in layout.iter().enumerate() {
            if y < geom.header.center().y {
                return i;
            }
        }
        layout.len()
    }

    fn paint_insertion_line(&self, ui: &egui::Ui, layout: &[SectionGeom], tgt: usize) {
        let y = layout
            .get(tgt)
            .map_or_else(|| layout.last().map_or(0.0, SectionGeom::bottom), |g| g.header.top());
        if let Some(first) = layout.first() {
            ui.painter().hline(
                first.header.x_range(),
                y,
                Stroke::new(2.0, self.theme.accent),
            );
        }
    }
}

/// Laid-out rects for one section this frame.
#[derive(Clone, Copy)]
struct SectionGeom {
    header: Rect,
    /// `None` when the section is collapsed.
    body: Option<Rect>,
}

impl SectionGeom {
    fn bottom(&self) -> f32 {
        self.body.map_or(self.header.bottom(), |b| b.bottom())
    }
}

/// Stable per-section header interaction id. Keyed on the section index
/// (stable for the duration of a drag, since reorder commits only on
/// release) so egui's drag tracking survives the layout shifts that
/// collapse/resize cause within a frame.
fn header_id(ui: &egui::Ui, idx: usize) -> egui::Id {
    ui.id().with(("egui_workbench::section_header", idx))
}

/// Paint a collapse twistie (chevron) at the left of a header rect:
/// pointing down when expanded, right when collapsed.
fn paint_twistie(ui: &egui::Ui, header: Rect, collapsed: bool, color: Color32) {
    let c = egui::pos2(header.left() + 11.0, header.center().y);
    let r = 3.5;
    let pts = if collapsed {
        // ">" pointing right
        [
            egui::pos2(c.x - r * 0.6, c.y - r),
            egui::pos2(c.x + r * 0.6, c.y),
            egui::pos2(c.x - r * 0.6, c.y + r),
        ]
    } else {
        // "v" pointing down
        [
            egui::pos2(c.x - r, c.y - r * 0.6),
            egui::pos2(c.x, c.y + r * 0.6),
            egui::pos2(c.x + r, c.y - r * 0.6),
        ]
    };
    let stroke = Stroke::new(1.5, color);
    ui.painter().line_segment([pts[0], pts[1]], stroke);
    ui.painter().line_segment([pts[1], pts[2]], stroke);
}

#[cfg(test)]
mod tests {
    use super::SidePanelStack;

    #[test]
    fn switch_restores_anchored_split() {
        // The reported flow: open A, drag B in (split), switch to C, then
        // click A again — the A+B split must come back.
        let mut s = SidePanelStack::<u32>::new();
        s.switch(1); // [1]
        s.add_section(2); // [1, 2]
        assert_eq!(s.open_modes(), &[1, 2]);
        s.switch(3); // stash [1,2] under anchor 1; open 3 solo
        assert_eq!(s.open_modes(), &[3]);
        s.switch(1); // anchor 1 has a remembered split -> restore it
        assert_eq!(s.open_modes(), &[1, 2]);
        assert_eq!(s.focused(), Some(&1));
    }

    #[test]
    fn switch_to_fresh_activity_opens_solo() {
        let mut s = SidePanelStack::<u32>::new();
        s.switch(1);
        s.switch(2); // 2 never anchored a split -> solo
        assert_eq!(s.open_modes(), &[2]);
        assert_eq!(s.focused(), Some(&2));
    }

    #[test]
    fn switch_to_open_section_keeps_split_and_focuses() {
        let mut s = SidePanelStack::<u32>::new();
        s.switch(1);
        s.add_section(2); // [1, 2], focused 2
        s.switch(1); // 1 is already open -> just focus, keep the split
        assert_eq!(s.open_modes(), &[1, 2]);
        assert_eq!(s.focused(), Some(&1));
    }

    #[test]
    fn cross_stack_move_transfers_section() {
        // The pure-state half of a cross-stack drag (2f): a view present in
        // stack A is "moved" to stack B via the same close+add_section pair
        // the workbench's reconcile path runs. A must lose it; B must gain
        // it and focus it; A's focus/contents stay sane.
        let mut a = SidePanelStack::<u32>::new();
        a.set_location_tag(0);
        a.switch(1); // A: [1]
        a.add_section(2); // A: [1, 2], focused 2
        let mut b = SidePanelStack::<u32>::new();
        b.set_location_tag(1);
        b.switch(9); // B: [9]

        // Move view 2 from A onto B.
        a.close(&2);
        b.add_section(2);

        assert!(!a.contains(&2), "source no longer holds the moved view");
        assert_eq!(a.open_modes(), &[1]);
        assert_eq!(a.focused(), Some(&1), "source refocuses a neighbour");
        assert!(b.contains(&2), "target gained the moved view");
        assert_eq!(b.open_modes(), &[9, 2]);
        assert_eq!(b.focused(), Some(&2), "target focuses the dropped view");
    }

    #[test]
    fn cross_stack_move_of_last_section_empties_source() {
        // Dragging a stack's only view onto the other empties the source
        // and the target accepts it (the empty-target drop path).
        let mut a = SidePanelStack::<u32>::new();
        a.switch(1); // A: [1]
        let mut b = SidePanelStack::<u32>::new();

        a.close(&1);
        b.add_section(1);

        assert!(a.is_empty(), "source emptied");
        assert_eq!(a.focused(), None);
        assert_eq!(b.open_modes(), &[1]);
        assert_eq!(b.focused(), Some(&1));
    }

    #[test]
    fn restored_split_can_be_re_stashed() {
        // Restore a split, switch away again, switch back — still intact.
        let mut s = SidePanelStack::<u32>::new();
        s.switch(1);
        s.add_section(2);
        s.switch(3);
        s.switch(1); // restore [1,2]
        s.switch(4); // stash [1,2] under 1 again; open 4 solo
        assert_eq!(s.open_modes(), &[4]);
        s.switch(1);
        assert_eq!(s.open_modes(), &[1, 2]);
    }
}
