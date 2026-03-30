use std::ops::{Deref, DerefMut};

use ratatui_core::{buffer::Buffer, layout::Rect};

use crate::element::Elements;
use crate::hooks::Hooks;
use crate::insets::Insets;
use crate::node::{Layout, WidthConstraint};

/// Implement [`ChildCollector`](crate::ChildCollector) for a component so it accepts slot children in
/// the `element!` macro.
///
/// Slot children are passed to the component's [`Component::children`] method as the
/// `slot` parameter. Layout containers like [`VStack`] and [`HStack`] use this to
/// accept arbitrary child elements.
///
/// # Example
///
/// ```ignore
/// #[derive(Default)]
/// struct Card { pub title: String }
///
/// impl Component for Card {
///     type State = ();
///     fn render(&self, area: Rect, buf: &mut Buffer, _: &()) { /* draw border */ }
///     fn children(&self, _: &(), slot: Option<Elements>) -> Option<Elements> {
///         slot // pass children through
///     }
/// }
///
/// impl_slot_children!(Card);
///
/// // Now Card can accept children:
/// element! {
///     Card(title: "My Card".into()) {
///         Spinner(label: "loading...")
///         "some text"
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_slot_children {
    ($t:ty) => {
        impl $crate::ChildCollector for $t {
            type Collector = $crate::Elements;
            type Output = $crate::ComponentWithSlot<$t>;

            fn finish(self, collector: $crate::Elements) -> $crate::ComponentWithSlot<$t> {
                $crate::ComponentWithSlot::new(self, collector)
            }
        }
    };
}

/// Result of handling an input event in a component's event handler.
///
/// Controls whether event propagation continues through the component
/// tree during either the capture or bubble phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// The event was handled by this component. Stops propagation.
    Consumed,
    /// The event was not handled. Propagation continues to the next node.
    Ignored,
}

/// Wrapper that automatically marks component state dirty on mutation.
///
/// The framework wraps each component's `State` in `Tracked<S>`.
/// Write access via [`DerefMut`] automatically marks the state dirty,
/// telling the framework this component needs to re-render.
/// Read access via [`Deref`] does not set the dirty flag.
///
/// # Usage in event handlers
///
/// Event handlers ([`Component::handle_event`], [`Component::handle_event_capture`])
/// receive `&mut Tracked<State>`. Writing to state through field access or
/// method calls goes through [`DerefMut`] and marks the component dirty:
///
/// ```ignore
/// fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
///     state.text.push('a');  // DerefMut → marks dirty
///     EventResult::Consumed
/// }
/// ```
///
/// # Reading state without marking dirty
///
/// **Important:** on `&mut Tracked<S>`, Rust's auto-deref uses [`DerefMut`]
/// for *all* field access — even reads. This means `state.some_field` sets
/// dirty even when you only read the value. Use [`read()`](Tracked::read)
/// to get a shared reference that goes through [`Deref`] instead:
///
/// ```ignore
/// fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
///     // state.mode would trigger DerefMut — use read() for a clean read
///     if state.read().mode == Mode::Insert {
///         state.text.push('a');  // DerefMut → marks dirty
///         EventResult::Consumed
///     } else {
///         EventResult::Ignored  // state stays clean
///     }
/// }
/// ```
///
/// # Usage with the imperative API
///
/// ```ignore
/// let id = renderer.push(Spinner::new("loading..."));
///
/// // DerefMut triggers dirty flag automatically
/// renderer.state_mut::<Spinner>(id).tick();
/// ```
pub struct Tracked<S> {
    inner: S,
    dirty: bool,
}

impl<S> Tracked<S> {
    /// Wrap a value in dirty-tracking. Starts dirty so the first render
    /// always happens.
    pub fn new(inner: S) -> Self {
        Self { inner, dirty: true }
    }

    /// Whether the inner value has been mutated since the last
    /// [`clear_dirty`](Tracked::clear_dirty) call.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Reset the dirty flag. Called by the framework after rendering.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Get a shared reference to the inner state without marking dirty.
    ///
    /// On `&mut Tracked<S>`, direct field access like `state.field` goes
    /// through [`DerefMut`], which unconditionally sets the dirty flag —
    /// even for reads. Use `state.read().field` instead to read state
    /// without triggering a re-render.
    ///
    /// This is especially useful in event handlers that conditionally
    /// modify state, or that read state to call methods using interior
    /// mutability (e.g., sending on a channel):
    ///
    /// ```ignore
    /// fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
    ///     if let Some(tx) = &state.read().event_tx {
    ///         tx.send(MyEvent::KeyPressed).ok();
    ///     }
    ///     EventResult::Consumed
    /// }
    /// ```
    pub fn read(&self) -> &S {
        &self.inner
    }
}

impl<S> Deref for Tracked<S> {
    type Target = S;

    fn deref(&self) -> &S {
        &self.inner
    }
}

impl<S> DerefMut for Tracked<S> {
    fn deref_mut(&mut self) -> &mut S {
        self.dirty = true;
        &mut self.inner
    }
}

/// A component that can render itself into a terminal region.
///
/// Most users should define components with the [`#[component]`](macro@crate::component)
/// and [`#[props]`](macro@crate::props) attribute macros rather than implementing
/// this trait directly:
///
/// ```ignore
/// #[props]
/// struct CardProps {
///     title: String,
///     #[default(true)]
///     visible: bool,
/// }
///
/// #[component(props = CardProps, children = Elements)]
/// fn card(props: &CardProps, children: Elements) -> Elements {
///     element! {
///         View(border: BorderType::Rounded, title: props.title.clone()) {
///             #(children)
///         }
///     }
/// }
/// ```
///
/// The `#[component]` macro generates an `impl Component` with an
/// [`update()`](Component::update) override that calls the function
/// once per cycle with real hooks and real children.
///
/// # Direct implementation
///
/// Implement this trait directly only for framework primitives that need
/// raw buffer access ([`Canvas`](crate::Canvas)) or layout chrome
/// ([`View`](crate::View)). Most methods are `#[doc(hidden)]` — they
/// exist for the framework and primitives, not for typical component
/// authors.
pub trait Component: Send + Sync + 'static {
    /// State type for this component. The framework wraps it in
    /// `Tracked<S>` for automatic dirty detection.
    type State: Send + Sync + Default + 'static;

    /// Primitive: render into a buffer region. Only for hand-written
    /// leaf components (e.g., [`Canvas`](crate::Canvas)).
    /// `#[component]` functions return element trees instead.
    #[doc(hidden)]
    fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &Self::State) {}

    /// Primitive: optional height hint to skip probe-render measurement.
    /// Only for hand-written leaf components.
    #[doc(hidden)]
    fn desired_height(&self, _width: u16, _state: &Self::State) -> Option<u16> {
        None
    }

    /// Capture-phase event handler (root → focused). Prefer
    /// [`Hooks::use_event_capture`](crate::Hooks::use_event_capture) in
    /// `#[component]` functions.
    #[doc(hidden)]
    fn handle_event_capture(
        &self,
        _event: &crossterm::event::Event,
        _state: &mut Tracked<Self::State>,
    ) -> EventResult {
        EventResult::Ignored
    }

    /// Bubble-phase event handler (focused → root). Prefer
    /// [`Hooks::use_event`](crate::Hooks::use_event) in `#[component]`
    /// functions.
    #[doc(hidden)]
    fn handle_event(
        &self,
        _event: &crossterm::event::Event,
        _state: &mut Tracked<Self::State>,
    ) -> EventResult {
        EventResult::Ignored
    }

    /// Whether this component can receive focus. Prefer
    /// [`Hooks::use_focusable`](crate::Hooks::use_focusable) in
    /// `#[component]` functions.
    #[doc(hidden)]
    fn is_focusable(&self, _state: &Self::State) -> bool {
        false
    }

    /// Cursor position when focused. Prefer
    /// [`Hooks::use_cursor`](crate::Hooks::use_cursor) in `#[component]`
    /// functions.
    #[doc(hidden)]
    fn cursor_position(&self, _area: Rect, _state: &Self::State) -> Option<(u16, u16)> {
        None
    }

    /// Provide initial state. Returns `None` to use `State::default()`.
    /// In `#[component]`, use `initial_state = expr` on the attribute instead.
    #[doc(hidden)]
    fn initial_state(&self) -> Option<Self::State> {
        None
    }

    /// Primitive: insets for child layout within the render area.
    /// Only for hand-written chrome components (e.g., [`View`](crate::View)).
    #[doc(hidden)]
    fn content_inset(&self, _state: &Self::State) -> Insets {
        Insets::ZERO
    }

    /// Layout direction for children. Prefer
    /// [`Hooks::use_layout`](crate::Hooks::use_layout) in `#[component]`
    /// functions.
    #[doc(hidden)]
    fn layout(&self) -> Layout {
        Layout::default()
    }

    /// Width constraint within a horizontal parent. Prefer
    /// [`Hooks::use_width_constraint`](crate::Hooks::use_width_constraint)
    /// in `#[component]` functions.
    #[doc(hidden)]
    fn width_constraint(&self) -> WidthConstraint {
        WidthConstraint::default()
    }

    /// Fallback: declare lifecycle effects. Called by the default
    /// [`update()`](Component::update) implementation. `#[component]`
    /// functions handle lifecycle and view in a single `update()` override.
    #[doc(hidden)]
    fn lifecycle(&self, _hooks: &mut Hooks<Self::State>, _state: &Self::State) {}

    /// Fallback: return the element tree. Called by the default
    /// [`update()`](Component::update) implementation. `#[component]`
    /// functions handle lifecycle and view in a single `update()` override.
    #[doc(hidden)]
    fn view(&self, _state: &Self::State, children: Elements) -> Elements {
        children
    }

    /// Combined lifecycle and view in a single call.
    ///
    /// Called by the framework during reconciliation. Collects hooks
    /// and produces the element tree in one pass, so the component
    /// function runs exactly once per cycle.
    ///
    /// The default implementation calls [`lifecycle()`](Component::lifecycle)
    /// then [`view()`](Component::view) separately — correct for hand-written
    /// `impl Component` primitives. The `#[component]` macro overrides this
    /// to call the user function once with real hooks and real children.
    fn update(
        &self,
        hooks: &mut Hooks<Self::State>,
        state: &Self::State,
        children: Elements,
    ) -> Elements {
        self.lifecycle(hooks, state);
        self.view(state, children)
    }
}

/// Vertical stack container.
///
/// Children are laid out top-to-bottom, each receiving the full
/// parent width. This is the default layout — [`VStack`] is mainly
/// used for explicit grouping or to anchor a keyed subtree.
///
/// ```ignore
/// element! {
///     VStack {
///         "Hello"
///         "World"
///     }
/// }
/// ```
#[derive(Debug, Default, Clone)]
pub struct VStack;

impl VStack {
    pub fn builder() -> Self {
        Self
    }

    pub fn build(self) -> Self {
        self
    }
}

#[eye_declare_macros::component(props = VStack, children = Elements, crate_path = crate)]
fn vstack(_props: &VStack, children: Elements) -> Elements {
    children
}

/// Horizontal stack container.
///
/// Children are laid out left-to-right. Use [`Column`] inside to
/// control individual child widths.
///
/// ```ignore
/// element! {
///     HStack {
///         Column(width: Fixed(10)) { "left" }
///         Column(width: Fill) { "right" }
///     }
/// }
/// ```
#[derive(Debug, Default, Clone)]
pub struct HStack;

impl HStack {
    pub fn builder() -> Self {
        Self
    }

    pub fn build(self) -> Self {
        self
    }
}

#[eye_declare_macros::component(props = HStack, children = Elements, crate_path = crate)]
fn hstack(_props: &HStack, hooks: &mut Hooks<()>, children: Elements) -> Elements {
    hooks.use_layout(Layout::Horizontal);
    children
}

/// Width-constrained wrapper for use inside [`HStack`].
///
/// ```ignore
/// element! {
///     HStack {
///         Column(width: Fixed(10)) { "fixed" }
///         Column(width: Fill) { "flexible" }
///     }
/// }
/// ```
#[derive(Debug, Default, Clone, typed_builder::TypedBuilder)]
pub struct Column {
    #[builder(default, setter(into))]
    pub width: WidthConstraint,
}

#[eye_declare_macros::component(props = Column, children = Elements, crate_path = crate)]
fn column(props: &Column, hooks: &mut Hooks<()>, children: Elements) -> Elements {
    hooks.use_width_constraint(props.width);
    children
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_starts_dirty() {
        let t = Tracked::new(42u32);
        assert!(t.is_dirty());
    }

    #[test]
    fn tracked_deref_does_not_set_dirty() {
        let mut t = Tracked::new(42u32);
        t.clear_dirty();
        assert!(!t.is_dirty());

        // Read access via Deref
        let _val = *t;
        assert!(!t.is_dirty());
    }

    #[test]
    fn tracked_deref_mut_sets_dirty() {
        let mut t = Tracked::new(42u32);
        t.clear_dirty();
        assert!(!t.is_dirty());

        // Write access via DerefMut
        *t = 99;
        assert!(t.is_dirty());
    }

    #[test]
    fn tracked_clear_dirty_resets() {
        let mut t = Tracked::new(42u32);
        assert!(t.is_dirty());
        t.clear_dirty();
        assert!(!t.is_dirty());
    }
}
