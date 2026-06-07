//! Generic activity-registry foundation. Consumer apps supply their own
//! `Ctx` impl; activities parameterize on a `Ctx`-trait bound so they can
//! be shared across apps without coupling to any one app's state.
//!
//! ## Quick map
//!
//! - [`Ctx`] — universal contract. Every app's Ctx struct impls it.
//! - [`Activity`] — the activity trait, generic over `C: Ctx + ?Sized`.
//!   Carries an ordered list of [`View`]s plus optional descriptors.
//! - [`View`], [`HamburgerEntry`], [`ActivityBarItem`] — surface
//!   sub-traits, each parameterized on the same `C` so activities that
//!   aren't shared bind a concrete app's umbrella ctx and shared
//!   activities bind a per-activity requirement trait.
//! - [`ActivityRegistry`] — ordered list of `Arc<dyn Activity<C>>`. One
//!   per app, built once at app startup.
//! - [`ActionError`] — error type for the inter-activity action seam.
//! - [`split_view_id`] — parse a wire view-id back into its parts (the
//!   inverse of [`Activity::view_id`]).

use std::sync::Arc;

// ---- Universal Ctx contract -----------------------------------------

/// Marker base for context types. Every consumer app's per-surface
/// context type implements it (commonly a `dyn AppCtx` trait object the
/// app defines). It carries no methods: per-activity state and every
/// other accessor live on per-activity *requirement traits* the app's
/// umbrella `Ctx` supertraits. State isn't on the universal base because
/// not every app models it as a single `dyn Any` slice — hiker, for one,
/// resolves disjoint `AppState` slices per `state_key` through its own
/// `AppCtx`. The bound exists only to constrain `Activity<C>`'s `C` to
/// context types.
pub trait Ctx {}

// ---- Activity trait + surface sub-traits -----------------------------

/// A registered activity (a container of one or more [`View`]s).
/// Singleton in the registry; per-instance state (multiple tabs of the
/// same activity) lives in tab payloads, not on the `Activity` impl
/// itself.
///
/// An activity declares its `views()`; the other surface accessors
/// default to empty/`None` so a new activity only implements what it
/// wants. An activity with no views opts out of the activity bar.
/// Generic over `C: Ctx + ?Sized` so:
///
/// - Activities owned by one app bind a concrete app-umbrella Ctx
///   (`Activity<dyn AppCtx>`).
/// - Shared activities impl `Activity<C>` for `C: FooCtx + ?Sized` where
///   `FooCtx` is the activity's own Ctx requirement trait. Each app's
///   umbrella Ctx type supertraits `FooCtx`, so the same `Arc<Foo>`
///   works in any app's registry.
pub trait Activity<C: Ctx + ?Sized + 'static>: Send + Sync {
    /// Stable kebab-case id (e.g. `"clusters"`). Used as the dispatch
    /// key from the registry + persisted in settings.
    fn id(&self) -> &'static str;

    /// Human-facing label (e.g. `"Cluster trees"`).
    fn label(&self) -> &'static str;

    /// Activity-bar / mode-button icon.
    fn icon(&self) -> egui::Image<'static>;

    /// Optional keybind chord descriptor (e.g. `"ctrl+shift+c"`); used
    /// by the keybind registry consumer. Default `None`.
    fn keybind_chord(&self) -> Option<&'static str> {
        None
    }

    /// Ordered list of render surfaces this activity contributes. A
    /// single-view activity returns one element (rendered headerless);
    /// multi-view containers return several. Empty (the default) opts
    /// out of the activity bar.
    fn views(&self) -> Vec<&dyn View<C>> {
        Vec::new()
    }

    /// View-id for one of this activity's views, per the
    /// `"<activity>/<view>"` convention. Single-view activities (or a
    /// view whose id equals the activity id) collapse to the bare
    /// activity id, keeping wire ids byte-identical; multi-view
    /// containers slash. Centralized here — call sites never hand-build
    /// the id. See [`split_view_id`] for the inverse.
    fn view_id(&self, view: &dyn View<C>) -> String {
        if self.views().len() <= 1 || view.id() == self.id() {
            self.id().to_string()
        } else {
            format!("{}/{}", self.id(), view.id())
        }
    }

    /// Whether this activity belongs on the PRIMARY (left) activity bar.
    /// Default: any activity with at least one [`View`] whose default
    /// location is the left bar. Right-bar activities render in the
    /// secondary side bar and are summoned via the right-sidebar toggle,
    /// not the activity strip.
    fn on_activity_bar(&self) -> bool {
        !self.views().is_empty()
            && matches!(self.default_location(), crate::side_bar::Location::LeftBar)
    }

    /// Which side bar this activity's views dock into by default. Left
    /// (the primary accordion driven by the activity bar) for most
    /// activities; right (the secondary accordion) for some. A placement
    /// overlay can later override this per-view, but the declaration
    /// seeds the default.
    fn default_location(&self) -> crate::side_bar::Location {
        crate::side_bar::Location::LeftBar
    }

    fn hamburger(&self) -> Option<&dyn HamburgerEntry<C>> {
        None
    }
    fn activity_bar(&self) -> Option<&dyn ActivityBarItem<C>> {
        None
    }
    fn command_palette(&self, _ctx: &mut C) -> Vec<PaletteCommand<C>> {
        Vec::new()
    }

    /// Inter-activity action dispatch. Default returns
    /// [`ActionError::UnknownAction`]. Activities that expose verbs to
    /// peers (or to the palette / hamburger) override this.
    fn dispatch_action(
        &self,
        _ctx: &mut C,
        action: &str,
        _args: serde_json::Value,
    ) -> Result<serde_json::Value, ActionError> {
        Err(ActionError::UnknownAction {
            activity: self.id().to_string(),
            action: action.to_string(),
        })
    }
}

/// A render surface contributed by an activity. The mode-switcher
/// invokes `render` for the currently-active view.
pub trait View<C: Ctx + ?Sized>: Send + Sync {
    /// Stable kebab-case id, unique within its activity (e.g.
    /// `"backlinks"`). For a single-view activity this equals the
    /// activity id. Composed into the wire view-id via
    /// [`Activity::view_id`].
    fn id(&self) -> &'static str;
    /// Key for this view's per-activity state slice on the consumer's
    /// `Ctx`. Defaults to the view id; a view backed by a
    /// differently-named state slice overrides it.
    fn state_key(&self) -> &'static str {
        self.id()
    }
    fn render(&self, ui: &mut egui::Ui, ctx: &mut C);
}

/// Top-strip hamburger menu entry.
pub trait HamburgerEntry<C: Ctx + ?Sized>: Send + Sync {
    fn label(&self) -> &'static str;
    fn keybind_id(&self) -> Option<&'static str> {
        None
    }
    fn invoke(&self, ctx: &mut C);
}

/// Activity-bar item override. By default an activity with a [`View`]
/// auto-renders an activity item using `Activity::icon` +
/// `Activity::label`; implementing this trait overrides those (e.g. for
/// a dynamic badge).
pub trait ActivityBarItem<C: Ctx + ?Sized>: Send + Sync {
    fn icon(&self) -> egui::Image<'static>;
    fn tooltip(&self) -> &'static str;
    fn invoke(&self, ctx: &mut C);
}

/// One entry returned by `Activity::command_palette`. The palette
/// dispatcher invokes `action` inside a fresh `Ctx` borrow.
pub struct PaletteCommand<C: Ctx + ?Sized> {
    pub id: &'static str,
    pub label: String,
    pub action: Box<dyn FnOnce(&mut C) + Send>,
}

// ---- Action seam errors ---------------------------------------------

/// Errors returned by [`ActivityRegistry::invoke`] /
/// [`Activity::dispatch_action`].
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("unknown activity `{0}`")]
    UnknownActivity(String),
    #[error("activity `{activity}` does not have action `{action}`")]
    UnknownAction { activity: String, action: String },
    #[error("invalid args for `{activity}::{action}`: {reason}")]
    InvalidArgs {
        activity: String,
        action: String,
        reason: String,
    },
    #[error("{0}")]
    Failed(String),
}

// ---- Registry --------------------------------------------------------

/// Ordered list of `Arc<dyn Activity<C>>` built once at app startup.
/// Consumers iterate via [`ActivityRegistry::iter`]. Order is
/// meaningful: sidebar mode buttons + activity items render in registry
/// order.
pub struct ActivityRegistry<C: Ctx + ?Sized + 'static> {
    activities: Vec<Arc<dyn Activity<C>>>,
}

impl<C: Ctx + ?Sized + 'static> ActivityRegistry<C> {
    /// Build a registry from an ordered list of activities.
    pub fn build(activities: Vec<Arc<dyn Activity<C>>>) -> Arc<Self> {
        Arc::new(Self { activities })
    }

    /// Iterate the registered activities in their stable order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Activity<C>>> {
        self.activities.iter()
    }

    /// O(N) lookup by stable id. N is ~5-15 activities per app, so no
    /// hashmap is warranted.
    pub fn by_id(&self, id: &str) -> Option<&Arc<dyn Activity<C>>> {
        self.activities.iter().find(|f| f.id() == id)
    }

    /// Dispatch `(activity_id, action, args)` through the registry.
    /// Entry point for inter-activity calls and the command-palette /
    /// hamburger dispatch paths. Returns [`ActionError::UnknownActivity`]
    /// when the id doesn't resolve; otherwise forwards to the activity's
    /// [`Activity::dispatch_action`].
    pub fn invoke(
        &self,
        ctx: &mut C,
        activity_id: &str,
        action: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ActionError> {
        let activity = self
            .activities
            .iter()
            .find(|f| f.id() == activity_id)
            .ok_or_else(|| ActionError::UnknownActivity(activity_id.to_string()))?;
        activity.dispatch_action(ctx, action, args)
    }
}

// ---- View-id parsing -------------------------------------------------

/// Split a wire view-id into `(activity_id, view_key)`. The convention
/// is `"<activity>/<view>"`; an id with no slash is a single-view
/// activity, so both halves are the whole string. Inverse of
/// [`Activity::view_id`]. The single point where the slash convention is
/// parsed — call sites never split the string themselves.
pub fn split_view_id(view_id: &str) -> (&str, &str) {
    view_id
        .split_once('/')
        .map_or((view_id, view_id), |(a, v)| (a, v))
}

// ---- Tests -----------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Synthetic Ctx for tests. `Ctx` is a marker base, so this is a
    /// trivially-`'static` unit struct coerced to `&mut dyn Ctx`; the
    /// synthetic activities below ignore their ctx entirely.
    struct TestCtx;
    impl Ctx for TestCtx {}

    /// A throwaway icon — tests never actually render, so an empty
    /// image is fine.
    fn empty_icon() -> egui::Image<'static> {
        egui::Image::new(egui::ImageSource::Bytes {
            uri: "tests/icon".into(),
            bytes: egui::load::Bytes::Static(&[]),
        })
    }

    /// Activity whose `dispatch_action` records the call count and
    /// echoes the args. Bound to `dyn Ctx` so it works against any
    /// app's Ctx — the simplest case.
    struct EchoActivity {
        id: &'static str,
        calls: Arc<AtomicUsize>,
    }
    impl Activity<dyn Ctx> for EchoActivity {
        fn id(&self) -> &'static str {
            self.id
        }
        fn label(&self) -> &'static str {
            "Echo"
        }
        fn icon(&self) -> egui::Image<'static> {
            empty_icon()
        }
        fn dispatch_action(
            &self,
            _ctx: &mut (dyn Ctx + 'static),
            action: &str,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, ActionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match action {
                "echo" => Ok(json!({ "from": self.id, "args": args })),
                _ => Err(ActionError::UnknownAction {
                    activity: self.id.to_string(),
                    action: action.to_string(),
                }),
            }
        }
    }

    /// A leaf view that records each `render` call so a test can prove
    /// the dispatched surface ran. Generic over `C` so the same view
    /// works under any Ctx the host binds.
    struct CountingView {
        id: &'static str,
        renders: Arc<AtomicUsize>,
    }
    impl<C: Ctx + ?Sized> View<C> for CountingView {
        fn id(&self) -> &'static str {
            self.id
        }
        fn render(&self, _ui: &mut egui::Ui, _ctx: &mut C) {
            self.renders.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Multi-view activity: owns two `CountingView`s so the test can
    /// exercise `views()`/`view_id()`/`on_activity_bar()` and the
    /// `split_view_id` round-trip. Its default location is configurable
    /// so a right-bar variant proves `on_activity_bar()` flips off.
    struct MultiView {
        id: &'static str,
        location: crate::side_bar::Location,
        a: CountingView,
        b: CountingView,
    }
    impl Activity<dyn Ctx> for MultiView {
        fn id(&self) -> &'static str {
            self.id
        }
        fn label(&self) -> &'static str {
            "Multi"
        }
        fn icon(&self) -> egui::Image<'static> {
            empty_icon()
        }
        fn views(&self) -> Vec<&dyn View<dyn Ctx>> {
            vec![&self.a, &self.b]
        }
        fn default_location(&self) -> crate::side_bar::Location {
            self.location
        }
    }

    /// Single-view activity whose lone view's id equals the activity
    /// id — proves the bare-id collapse in `view_id`.
    struct SingleView {
        id: &'static str,
        view: CountingView,
    }
    impl Activity<dyn Ctx> for SingleView {
        fn id(&self) -> &'static str {
            self.id
        }
        fn label(&self) -> &'static str {
            "Single"
        }
        fn icon(&self) -> egui::Image<'static> {
            empty_icon()
        }
        fn views(&self) -> Vec<&dyn View<dyn Ctx>> {
            vec![&self.view]
        }
    }

    fn run<R>(f: impl FnOnce(&mut (dyn Ctx + 'static)) -> R) -> R {
        let mut ctx = TestCtx;
        f(&mut ctx)
    }

    #[test]
    fn invoke_routes_to_named_activity() {
        let calls_a = Arc::new(AtomicUsize::new(0));
        let calls_b = Arc::new(AtomicUsize::new(0));
        let reg: Arc<ActivityRegistry<dyn Ctx>> = ActivityRegistry::build(vec![
            Arc::new(EchoActivity {
                id: "alpha",
                calls: calls_a.clone(),
            }) as Arc<dyn Activity<dyn Ctx>>,
            Arc::new(EchoActivity {
                id: "beta",
                calls: calls_b.clone(),
            }) as Arc<dyn Activity<dyn Ctx>>,
        ]);
        let out = run(|ctx| reg.invoke(ctx, "beta", "echo", json!({"n": 1})).unwrap());
        assert_eq!(out["from"], "beta");
        assert_eq!(calls_b.load(Ordering::SeqCst), 1);
        assert_eq!(calls_a.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn invoke_unknown_activity_errors() {
        let reg: Arc<ActivityRegistry<dyn Ctx>> = ActivityRegistry::build(vec![]);
        let err = run(|ctx| reg.invoke(ctx, "nope", "x", json!(null)).unwrap_err());
        assert!(matches!(err, ActionError::UnknownActivity(ref s) if s == "nope"));
    }

    #[test]
    fn invoke_unknown_action_errors() {
        let calls = Arc::new(AtomicUsize::new(0));
        let reg: Arc<ActivityRegistry<dyn Ctx>> =
            ActivityRegistry::build(vec![Arc::new(EchoActivity { id: "alpha", calls })
                as Arc<dyn Activity<dyn Ctx>>]);
        let err = run(|ctx| {
            reg.invoke(ctx, "alpha", "no_such_action", json!(null))
                .unwrap_err()
        });
        match err {
            ActionError::UnknownAction { activity, action } => {
                assert_eq!(activity, "alpha");
                assert_eq!(action, "no_such_action");
            }
            other => panic!("got {other:?}"),
        }
    }

    /// `view_id` slashes a multi-view container's views, and
    /// `split_view_id` round-trips the wire id back to its parts.
    #[test]
    fn multi_view_id_round_trips() {
        let act = MultiView {
            id: "multi",
            location: crate::side_bar::Location::LeftBar,
            a: CountingView {
                id: "alpha",
                renders: Arc::new(AtomicUsize::new(0)),
            },
            b: CountingView {
                id: "beta",
                renders: Arc::new(AtomicUsize::new(0)),
            },
        };
        let views = act.views();
        assert_eq!(act.view_id(views[0]), "multi/alpha");
        assert_eq!(act.view_id(views[1]), "multi/beta");
        assert_eq!(split_view_id("multi/beta"), ("multi", "beta"));
    }

    /// A single-view activity (or a view whose id equals the activity
    /// id) collapses to the bare activity id, and `split_view_id`
    /// returns the whole string for both halves.
    #[test]
    fn single_view_id_collapses_to_bare_id() {
        let act = SingleView {
            id: "solo",
            view: CountingView {
                id: "solo",
                renders: Arc::new(AtomicUsize::new(0)),
            },
        };
        let views = act.views();
        assert_eq!(act.view_id(views[0]), "solo");
        assert_eq!(split_view_id("solo"), ("solo", "solo"));
    }

    /// `on_activity_bar` is true for a left-bar activity with views and
    /// false for a right-bar one; an activity with no views also opts
    /// out.
    #[test]
    fn on_activity_bar_follows_views_and_location() {
        let left = MultiView {
            id: "left",
            location: crate::side_bar::Location::LeftBar,
            a: CountingView {
                id: "alpha",
                renders: Arc::new(AtomicUsize::new(0)),
            },
            b: CountingView {
                id: "beta",
                renders: Arc::new(AtomicUsize::new(0)),
            },
        };
        assert!(left.on_activity_bar());

        let right = MultiView {
            id: "right",
            location: crate::side_bar::Location::RightBar,
            a: CountingView {
                id: "alpha",
                renders: Arc::new(AtomicUsize::new(0)),
            },
            b: CountingView {
                id: "beta",
                renders: Arc::new(AtomicUsize::new(0)),
            },
        };
        assert!(!right.on_activity_bar());

        // No views (the EchoActivity default) opts out regardless.
        let echo = EchoActivity {
            id: "echo",
            calls: Arc::new(AtomicUsize::new(0)),
        };
        assert!(!echo.on_activity_bar());
    }

    /// A dispatched view renders against a real `egui::Ui`, proving the
    /// `View::render` seam threads a Ctx through to a multi-view
    /// activity's surface.
    #[test]
    fn view_render_runs_against_ctx() {
        let renders = Arc::new(AtomicUsize::new(0));
        let act = MultiView {
            id: "multi",
            location: crate::side_bar::Location::LeftBar,
            a: CountingView {
                id: "alpha",
                renders: renders.clone(),
            },
            b: CountingView {
                id: "beta",
                renders: Arc::new(AtomicUsize::new(0)),
            },
        };
        let egui_ctx = egui::Context::default();
        let _ = egui_ctx.run(Default::default(), |egui_ctx| {
            egui::CentralPanel::default().show(egui_ctx, |ui| {
                run(|ctx| act.views()[0].render(ui, ctx));
            });
        });
        assert_eq!(renders.load(Ordering::SeqCst), 1);
    }
}
