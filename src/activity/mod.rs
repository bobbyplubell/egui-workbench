//! Generic activity-registry foundation. Consumer apps supply their own
//! `Ctx` impl; activities parameterize on a `Ctx`-trait bound so they can
//! be shared across apps without coupling to any one app's state.
//!
//! ## Quick map
//!
//! - [`Ctx`] — universal contract. Every app's Ctx struct impls it.
//! - [`Activity`] — the activity trait, generic over `C: Ctx + ?Sized`.
//! - [`SidebarSurface`], [`HamburgerEntry`], [`ActivityBarItem`] — surface
//!   sub-traits, each parameterized on the same `C` so activities that
//!   aren't shared bind a concrete app's umbrella ctx and shared
//!   activities bind a per-activity requirement trait.
//! - [`ActivityRegistry`] — ordered list of `Arc<dyn Activity<C>>`. One
//!   per app, built once at app startup.
//! - [`ActionError`] — error type for the inter-activity action seam.

use std::any::Any;
use std::sync::Arc;

// ---- Universal Ctx contract -----------------------------------------

/// The minimum contract every consumer app's per-surface Ctx impl
/// satisfies. Activities see callers as `&mut C` where `C: Ctx + ?Sized`
/// (often a `dyn AppCtx` trait object the app defines). The only
/// universal method is the per-activity opaque state slot; every other
/// accessor lives on a per-activity *requirement trait* the app's
/// umbrella Ctx supertraits.
///
/// Why `state()` is on the universal trait: the registry's host needs
/// to thread a per-activity state slice into the call regardless of
/// which activity is rendering. Putting it here keeps that universal
/// without forcing every per-activity requirement trait to redeclare it.
pub trait Ctx {
    /// Per-activity opaque state slice. Each activity downcasts to its
    /// own concrete state struct via
    /// `ctx.state().downcast_mut::<FooState>()`.
    fn state(&mut self) -> &mut dyn Any;
}

// ---- Activity trait + surface sub-traits -----------------------------

/// A registered activity. Singleton in the registry; per-instance state
/// (multiple tabs of the same activity) lives in tab payloads, not on
/// the `Activity` impl itself.
///
/// Surface accessors default to `None` so a new activity only implements
/// what it wants. Generic over `C: Ctx + ?Sized` so:
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

    fn sidebar(&self) -> Option<&dyn SidebarSurface<C>> {
        None
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

/// Sidebar mode body. The mode-switcher invokes `render` for the
/// currently-active activity's sidebar surface.
pub trait SidebarSurface<C: Ctx + ?Sized>: Send + Sync {
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

/// Activity-bar item override. By default an activity with a
/// `SidebarSurface` auto-renders an activity item using `Activity::icon`
/// + `Activity::label`; implementing this trait overrides those (e.g.
/// for a dynamic badge).
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

// ---- Tests -----------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Synthetic Ctx impl for tests — owns its state as a Box so the
    /// trait-object lifetime stays `'static` (`dyn Ctx` defaults to
    /// `dyn Ctx + 'static`, so Ctx impls coerced to `&mut dyn Ctx`
    /// must themselves be `'static`). Real apps build their Ctx impl
    /// with disjoint `&mut` borrows of AppState fields — they sidestep
    /// the lifetime issue because the impl struct's lifetime is the
    /// scope of the surface invocation, and the trait object is
    /// reborrowed fresh each call.
    struct TestCtx {
        state: Box<dyn Any>,
    }
    impl Ctx for TestCtx {
        fn state(&mut self) -> &mut dyn Any {
            self.state.as_mut()
        }
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
            egui::Image::new(egui::ImageSource::Bytes {
                uri: "tests/echo".into(),
                bytes: egui::load::Bytes::Static(&[]),
            })
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

    fn run<R>(f: impl FnOnce(&mut (dyn Ctx + 'static)) -> R) -> R {
        let mut ctx = TestCtx {
            state: Box::new(()),
        };
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
}
