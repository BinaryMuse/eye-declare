use std::marker::PhantomData;
use std::time::{Duration, Instant};

use crate::node::{Effect, EffectKind, TypedEffectHandler};

/// Effect collector for declarative lifecycle management.
///
/// Components receive a `Hooks` instance in their
/// [`lifecycle`](crate::Component::lifecycle) method and use it to
/// declare effects. The framework calls `lifecycle` after every build
/// and update, clearing old effects and applying the new set — so
/// effects are always consistent with current props and state.
///
/// # Available hooks
///
/// | Hook | Fires when |
/// |------|------------|
/// | [`use_interval`](Hooks::use_interval) | Periodically, at the given duration |
/// | [`use_mount`](Hooks::use_mount) | Once, after the component is first built |
/// | [`use_unmount`](Hooks::use_unmount) | Once, when the component is removed |
/// | [`use_autofocus`](Hooks::use_autofocus) | Requests focus when the component mounts |
///
/// # Example
///
/// ```ignore
/// fn lifecycle(&self, hooks: &mut Hooks<TimerState>, state: &TimerState) {
///     if self.running {
///         hooks.use_interval(Duration::from_secs(1), |s| s.elapsed += 1);
///     }
///     hooks.use_mount(|s| s.started_at = Instant::now());
///     hooks.use_unmount(|s| println!("ran for {:?}", s.started_at.elapsed()));
/// }
/// ```
pub struct Hooks<S: 'static> {
    effects: Vec<Effect>,
    autofocus: bool,
    _marker: PhantomData<S>,
}

impl<S: Send + Sync + 'static> Hooks<S> {
    pub(crate) fn new() -> Self {
        Self {
            effects: Vec::new(),
            autofocus: false,
            _marker: PhantomData,
        }
    }

    /// Register a periodic interval effect.
    ///
    /// The `handler` is called each time `interval` elapses during
    /// the framework's tick cycle. The handler receives `&mut State`
    /// and any mutations automatically mark the component dirty.
    ///
    /// Commonly used for animations (e.g., the built-in [`Spinner`](crate::Spinner)
    /// uses an 80ms interval to cycle frames).
    pub fn use_interval(
        &mut self,
        interval: Duration,
        handler: impl Fn(&mut S) + Send + Sync + 'static,
    ) {
        self.effects.push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::Interval {
                interval,
                last_tick: Instant::now(),
            },
        });
    }

    /// Register a mount effect that fires once after the component is built.
    ///
    /// Use this for one-time initialization that depends on state being
    /// available (e.g., recording a start time, fetching initial data).
    pub fn use_mount(&mut self, handler: impl Fn(&mut S) + Send + Sync + 'static) {
        self.effects.push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::OnMount,
        });
    }

    /// Register an unmount effect that fires when the component is removed
    /// from the tree.
    ///
    /// Use this for cleanup: logging, cancelling external resources, etc.
    pub fn use_unmount(&mut self, handler: impl Fn(&mut S) + Send + Sync + 'static) {
        self.effects.push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::OnUnmount,
        });
    }

    /// Request focus when this node mounts.
    ///
    /// If multiple nodes mount with autofocus in the same rebuild,
    /// the last one wins.
    pub fn use_autofocus(&mut self) {
        self.autofocus = true;
    }

    /// Whether autofocus was requested.
    pub(crate) fn autofocus(&self) -> bool {
        self.autofocus
    }

    /// Consume the hooks and return collected effects.
    pub(crate) fn into_effects(self) -> Vec<Effect> {
        self.effects
    }
}
