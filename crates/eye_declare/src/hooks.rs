use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::time::{Duration, Instant};

use ratatui_core::layout::Rect;

use crate::component::{EventResult, Tracked};
use crate::context::ContextMap;
use crate::node::{
    AnyCursorHook, AnyDesiredHeightHook, AnyEventHook, CallSite, Effect, EffectKind, Layout,
    TypedCursorHook, TypedDesiredHeightHook, TypedEffectHandler, TypedEventHook, WidthConstraint,
};

/// A type-erased context consumer callback.
///
/// Created by [`Hooks::use_context`] and executed by the framework
/// during reconciliation with the current context map, the component's
/// props (as `&dyn Any`), and mutable tracked state.
pub(crate) type ConsumerFn<S> = Box<dyn FnOnce(&ContextMap, &dyn Any, &mut Tracked<S>) + Send>;

/// Collected output from a [`Hooks`] instance after decomposition.
pub(crate) struct HooksOutput<S: 'static> {
    pub effects: Vec<Effect>,
    pub autofocus: bool,
    pub focus_scope: bool,
    pub provided: Vec<(TypeId, Box<dyn Any + Send + Sync>)>,
    pub consumers: Vec<ConsumerFn<S>>,
    pub focusable: Option<bool>,
    pub cursor_hook: Option<Box<dyn AnyCursorHook>>,
    pub event_hook: Option<Box<dyn AnyEventHook>>,
    pub capture_hook: Option<Box<dyn AnyEventHook>>,
    pub layout: Option<Layout>,
    pub width_constraint: Option<WidthConstraint>,
    pub height_hint: Option<u16>,
    pub desired_height_hook: Option<Box<dyn AnyDesiredHeightHook>>,
}

/// Effect collector for declarative lifecycle management.
///
/// Components receive a `Hooks` instance in their `#[component]`
/// function body and use it to declare effects. The framework runs
/// the component function after every build and update, clearing old
/// effects and applying the new set — so effects are always consistent
/// with current props and state.
///
/// The type parameter `P` is the component's props type, and `S` is
/// the component's state type. Hook callbacks receive `&P` (props)
/// adjacent to `&mut S` or `&mut Tracked<S>` (state), giving them
/// access to the component's current props without cloning.
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
/// #[component(props = Timer, state = TimerState)]
/// fn timer(props: &Timer, state: &TimerState, hooks: &mut Hooks<Timer, TimerState>) -> Elements {
///     if props.running {
///         hooks.use_interval(Duration::from_secs(1), |_props, s| s.elapsed += 1);
///     }
///     hooks.use_mount(|_props, s| s.started_at = Instant::now());
///     hooks.use_unmount(|_props, s| println!("ran for {:?}", s.started_at.elapsed()));
///     // ... return element tree
/// }
/// ```
pub struct Hooks<P: 'static, S: 'static> {
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
    height_hint: Option<u16>,
    desired_height_hook: Option<Box<dyn AnyDesiredHeightHook>>,
    // P is used only for type-level constraints on callback signatures.
    // PhantomData<fn() -> P> makes Hooks covariant in P without affecting layout.
    _marker: PhantomData<fn() -> P>,
}

// The `#[component]` macro casts `&mut Hooks<Wrapper, S>` to `&mut Hooks<Props, S>`
// for data-children wrappers. This is sound only because P is phantom. This assertion
// catches any future change that adds a P-typed field.
const _: () = {
    assert!(std::mem::size_of::<Hooks<u8, ()>>() == std::mem::size_of::<Hooks<u64, ()>>());
    assert!(std::mem::align_of::<Hooks<u8, ()>>() == std::mem::align_of::<Hooks<u64, ()>>());
};

impl<P: Send + Sync + 'static, S: Send + Sync + 'static> Default for Hooks<P, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Send + Sync + 'static, S: Send + Sync + 'static> Hooks<P, S> {
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
            height_hint: None,
            desired_height_hook: None,
            _marker: PhantomData,
        }
    }

    /// Register a periodic interval effect.
    ///
    /// The `handler` is called each time `interval` elapses during
    /// the framework's tick cycle. The handler receives the component's
    /// current props and `&mut Tracked<State>`. Mutations through
    /// [`DerefMut`](std::ops::DerefMut) automatically mark the component
    /// dirty; use [`Tracked::read()`] to access state without triggering
    /// a re-render.
    ///
    /// Commonly used for animations (e.g., the built-in [`Spinner`](crate::Spinner)
    /// uses an 80ms interval to cycle frames).
    #[track_caller]
    pub fn use_interval(
        &mut self,
        interval: Duration,
        handler: impl Fn(&P, &mut Tracked<S>) + Send + Sync + 'static,
    ) {
        self.effects.push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::Interval {
                interval,
                last_tick: Instant::now(),
            },
            call_site: CallSite::from_location(std::panic::Location::caller()),
        });
    }

    /// Register a mount effect that fires once after the component is built.
    ///
    /// Use this for one-time initialization that depends on state being
    /// available (e.g., recording a start time, fetching initial data).
    #[track_caller]
    pub fn use_mount(&mut self, handler: impl Fn(&P, &mut Tracked<S>) + Send + Sync + 'static) {
        self.effects.push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::OnMount,
            call_site: CallSite::from_location(std::panic::Location::caller()),
        });
    }

    /// Register an unmount effect that fires when the component is removed
    /// from the tree.
    ///
    /// Use this for cleanup: logging, cancelling external resources, etc.
    #[track_caller]
    pub fn use_unmount(&mut self, handler: impl Fn(&P, &mut Tracked<S>) + Send + Sync + 'static) {
        self.effects.push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::OnUnmount,
            call_site: CallSite::from_location(std::panic::Location::caller()),
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
    /// #[component(props = MyProvider, children = Elements)]
    /// fn my_provider(props: &MyProvider, hooks: &mut Hooks<MyProvider, ()>, children: Elements) -> Elements {
    ///     hooks.provide_context(props.event_sender.clone());
    ///     children
    /// }
    /// ```
    pub fn provide_context<T: Any + Send + Sync>(&mut self, value: T) {
        self.provided.push((TypeId::of::<T>(), Box::new(value)));
    }

    /// Read a context value provided by an ancestor component.
    ///
    /// The `handler` is called with `Option<&T>` (the context value,
    /// or `None` if no ancestor provides `T`), `&P` (the component's
    /// current props), and `&mut Tracked<S>` (the component's mutable
    /// state). The handler always fires — use the `Option` to handle
    /// the absent case.
    ///
    /// The handler runs during reconciliation, after the component
    /// function returns.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[component(props = MyButton, state = MyState)]
    /// fn my_button(props: &MyButton, hooks: &mut Hooks<MyButton, MyState>) -> Elements {
    ///     hooks.use_context::<Sender<AppEvent>>(|sender, _props, state| {
    ///         state.tx = sender.cloned();
    ///     });
    ///     // ... return element tree
    /// }
    /// ```
    pub fn use_context<T: Any + Send + Sync + 'static>(
        &mut self,
        handler: impl FnOnce(Option<&T>, &P, &mut Tracked<S>) + Send + 'static,
    ) {
        let type_id = TypeId::of::<T>();
        self.consumers.push(Box::new(
            move |context: &ContextMap, component: &dyn Any, tracked: &mut Tracked<S>| {
                let props = component
                    .downcast_ref::<P>()
                    .expect("props type mismatch in use_context");
                let value = context
                    .get_by_type_id(type_id)
                    .and_then(|v| v.downcast_ref::<T>());
                handler(value, props, tracked);
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
        handler: impl Fn(Rect, &P, &S) -> Option<(u16, u16)> + Send + Sync + 'static,
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
    /// The handler receives the event, the component's current props,
    /// and `&mut Tracked<S>` — only mutations through `DerefMut` mark
    /// the component dirty, matching the trait API behavior.
    pub fn use_event(
        &mut self,
        handler: impl Fn(&crossterm::event::Event, &P, &mut Tracked<S>) -> EventResult
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
        handler: impl Fn(&crossterm::event::Event, &P, &mut Tracked<S>) -> EventResult
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

    /// Declare a fixed height for this component.
    ///
    /// The framework skips probe-render measurement and uses this value
    /// directly. Useful for components that fill their given area (e.g.,
    /// bordered inputs) or that know their height upfront.
    pub fn use_height_hint(&mut self, height: u16) {
        self.height_hint = Some(height);
    }

    /// Declare a dynamic height callback for this component.
    ///
    /// The handler receives the available width, the component's current
    /// props, and state, and returns the desired height (or `None` to
    /// fall back to [`use_height_hint`](Hooks::use_height_hint) if set,
    /// or probe-render measurement otherwise).
    ///
    /// This takes priority over [`use_height_hint`](Hooks::use_height_hint)
    /// since it is width-aware. Use `use_height_hint` instead when the
    /// height is fixed and does not depend on width or state.
    pub fn use_desired_height(
        &mut self,
        handler: impl Fn(u16, &P, &S) -> Option<u16> + Send + Sync + 'static,
    ) {
        self.desired_height_hook = Some(Box::new(TypedDesiredHeightHook {
            handler: Box::new(handler),
        }));
    }

    /// Consume the hooks, returning effects, provided contexts, and consumers.
    pub(crate) fn decompose(self) -> HooksOutput<S> {
        HooksOutput {
            effects: self.effects,
            autofocus: self.autofocus,
            focus_scope: self.focus_scope,
            provided: self.provided,
            consumers: self.consumers,
            focusable: self.focusable,
            cursor_hook: self.cursor_hook,
            event_hook: self.event_hook,
            capture_hook: self.capture_hook,
            layout: self.layout,
            width_constraint: self.width_constraint,
            height_hint: self.height_hint,
            desired_height_hook: self.desired_height_hook,
        }
    }
}
