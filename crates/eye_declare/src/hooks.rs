use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::time::{Duration, Instant};

use ratatui_core::layout::Rect;

use crate::component::{EventResult, Tracked};
use crate::context::ContextMap;
use crate::node::{
    AnyCursorHook, AnyEventHook, Effect, EffectKind, Layout, TypedCursorHook, TypedEffectHandler,
    TypedEventHook, WidthConstraint,
};

/// A type-erased context consumer callback.
///
/// Created by [`Hooks::use_context`] and executed by the framework
/// during reconciliation with the current context map and mutable
/// tracked state.
pub(crate) type ConsumerFn<S> = Box<dyn FnOnce(&ContextMap, &mut Tracked<S>) + Send>;

/// A tuple of the decomposed hooks.
pub(crate) type Decomposed<S> = (
    Vec<Effect>,
    bool,
    bool,
    Vec<(TypeId, Box<dyn Any + Send + Sync>)>,
    Vec<ConsumerFn<S>>,
    Option<bool>,
    Option<Box<dyn AnyCursorHook>>,
    Option<Box<dyn AnyEventHook>>,
    Option<Box<dyn AnyEventHook>>,
    Option<Layout>,
    Option<WidthConstraint>,
);

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
/// | [`use_focus_scope`](Hooks::use_focus_scope) | Creates a focus scope boundary for Tab cycling |
/// | [`provide_context`](Hooks::provide_context) | Makes a value available to descendants |
/// | [`use_context`](Hooks::use_context) | Reads a value provided by an ancestor |
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
    focus_scope: bool,
    provided: Vec<(TypeId, Box<dyn Any + Send + Sync>)>,
    consumers: Vec<ConsumerFn<S>>,
    focusable: Option<bool>,
    cursor_hook: Option<Box<dyn AnyCursorHook>>,
    event_hook: Option<Box<dyn AnyEventHook>>,
    capture_hook: Option<Box<dyn AnyEventHook>>,
    layout: Option<Layout>,
    width_constraint: Option<WidthConstraint>,
    _marker: PhantomData<S>,
}

impl<S: Send + Sync + 'static> Default for Hooks<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Send + Sync + 'static> Hooks<S> {
    /// Create a new empty hooks instance.
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
            autofocus: false,
            focus_scope: false,
            provided: Vec::new(),
            consumers: Vec::new(),
            focusable: None,
            cursor_hook: None,
            event_hook: None,
            capture_hook: None,
            layout: None,
            width_constraint: None,
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

    /// Mark this node as a focus scope boundary.
    ///
    /// Tab/Shift-Tab cycling is confined to focusable descendants
    /// within this scope. Scopes nest — the deepest enclosing scope
    /// wins. When this node is removed from the tree, focus is
    /// restored to whatever was focused before the scope captured it.
    pub fn use_focus_scope(&mut self) {
        self.focus_scope = true;
    }

    /// Provide a context value to all descendant components.
    ///
    /// The value is available during this reconciliation pass to any
    /// descendant that calls [`use_context`](Hooks::use_context) with
    /// the same type `T`. If an ancestor already provides `T`, this
    /// component's value shadows it for the subtree.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn lifecycle(&self, hooks: &mut Hooks<MyState>, _state: &MyState) {
    ///     hooks.provide_context(self.event_sender.clone());
    /// }
    /// ```
    pub fn provide_context<T: Any + Send + Sync>(&mut self, value: T) {
        self.provided.push((TypeId::of::<T>(), Box::new(value)));
    }

    /// Read a context value provided by an ancestor component.
    ///
    /// The `handler` is called with `Option<&T>` (the context value,
    /// or `None` if no ancestor provides `T`) and `&mut Tracked<S>`
    /// (the component's mutable state). The handler always fires —
    /// use the `Option` to handle the absent case.
    ///
    /// The handler runs during reconciliation, after the component's
    /// `lifecycle` method returns.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn lifecycle(&self, hooks: &mut Hooks<MyState>, _state: &MyState) {
    ///     hooks.use_context::<Sender<AppEvent>>(|sender, state| {
    ///         state.tx = sender.cloned();
    ///     });
    /// }
    /// ```
    pub fn use_context<T: Any + Send + Sync + 'static>(
        &mut self,
        handler: impl FnOnce(Option<&T>, &mut Tracked<S>) + Send + 'static,
    ) {
        let type_id = TypeId::of::<T>();
        self.consumers.push(Box::new(
            move |context: &ContextMap, tracked: &mut Tracked<S>| {
                let value = context
                    .get_by_type_id(type_id)
                    .and_then(|v| v.downcast_ref::<T>());
                handler(value, tracked);
            },
        ));
    }

    /// Declare this component as focusable (or not).
    ///
    /// Focusable components participate in Tab cycling. This overrides
    /// the component's [`is_focusable`](crate::Component::is_focusable)
    /// trait method.
    pub fn use_focusable(&mut self, focusable: bool) {
        self.focusable = Some(focusable);
    }

    /// Declare a cursor position callback for when this component has focus.
    ///
    /// Returns `(col, row)` relative to the component's render area,
    /// or `None` to hide the cursor. This overrides the component's
    /// [`cursor_position`](crate::Component::cursor_position) trait method.
    pub fn use_cursor(
        &mut self,
        handler: impl Fn(Rect, &S) -> Option<(u16, u16)> + Send + Sync + 'static,
    ) {
        self.cursor_hook = Some(Box::new(TypedCursorHook {
            handler: Box::new(handler),
        }));
    }

    /// Declare an event handler for the bubble phase (focused → root).
    ///
    /// Return [`EventResult::Consumed`](crate::EventResult::Consumed)
    /// to stop propagation. This overrides the component's
    /// [`handle_event`](crate::Component::handle_event) trait method.
    ///
    /// The handler receives `&mut Tracked<S>` — only mutations through
    /// `DerefMut` mark the component dirty, matching the trait API behavior.
    pub fn use_event(
        &mut self,
        handler: impl Fn(&crossterm::event::Event, &mut Tracked<S>) -> EventResult
        + Send
        + Sync
        + 'static,
    ) {
        self.event_hook = Some(Box::new(TypedEventHook {
            handler: Box::new(handler),
        }));
    }

    /// Declare an event handler for the capture phase (root → focused).
    ///
    /// The capture phase fires before the bubble phase. Return
    /// [`EventResult::Consumed`](crate::EventResult::Consumed) to
    /// prevent the event from reaching the focused component.
    pub fn use_event_capture(
        &mut self,
        handler: impl Fn(&crossterm::event::Event, &mut Tracked<S>) -> EventResult
        + Send
        + Sync
        + 'static,
    ) {
        self.capture_hook = Some(Box::new(TypedEventHook {
            handler: Box::new(handler),
        }));
    }

    /// Declare this component's layout direction.
    ///
    /// Override the component's [`layout`](crate::Component::layout) trait method.
    /// Use `Layout::Horizontal` for side-by-side children.
    pub fn use_layout(&mut self, layout: Layout) {
        self.layout = Some(layout);
    }

    /// Declare this component's width constraint within a horizontal parent.
    ///
    /// Override the component's [`width_constraint`](crate::Component::width_constraint) trait method.
    pub fn use_width_constraint(&mut self, constraint: WidthConstraint) {
        self.width_constraint = Some(constraint);
    }

    /// Consume the hooks, returning effects, provided contexts, and consumers.
    pub(crate) fn decompose(self) -> Decomposed<S> {
        (
            self.effects,
            self.autofocus,
            self.focus_scope,
            self.provided,
            self.consumers,
            self.focusable,
            self.cursor_hook,
            self.event_hook,
            self.capture_hook,
            self.layout,
            self.width_constraint,
        )
    }
}
