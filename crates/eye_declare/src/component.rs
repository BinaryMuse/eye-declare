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
/// This is the core trait of eye_declare. Components separate **props**
/// (data on `&self`, set by the parent, immutable) from **state** (the
/// associated `State` type, framework-managed via [`Tracked`]).
///
/// # Minimal implementation
///
/// ```ignore
/// use eye_declare::Component;
/// use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};
/// use ratatui_widgets::paragraph::Paragraph;
///
/// #[derive(Default)]
/// struct Badge { pub label: String }
///
/// impl Component for Badge {
///     type State = ();
///
///     fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
///         Paragraph::new(self.label.as_str()).render(area, buf);
///     }
/// }
/// ```
///
/// # Children and composition
///
/// Components that generate their own child trees override [`children`](Component::children).
/// The `slot` parameter carries externally-provided children (from the
/// `element!` macro's brace syntax). See the three patterns:
///
/// - **Pass through** — return `slot` unchanged (default). Layout containers
///   like [`VStack`] do this.
/// - **Generate own tree** — ignore `slot`, return a custom [`Elements`].
/// - **Wrap slot** — incorporate `slot` into a larger tree with headers,
///   borders, etc.
///
/// # Lifecycle
///
/// Override [`lifecycle`](Component::lifecycle) to declare effects via [`Hooks`]:
/// intervals, mount/unmount handlers, and autofocus requests.
pub trait Component: Send + Sync + 'static {
    /// State type for this component. The framework wraps it in
    /// `Tracked<S>` for automatic dirty detection.
    type State: Send + Sync + Default + 'static;

    /// Render into the given buffer region using current state.
    /// Can use any ratatui Widget internally.
    ///
    /// By default, the framework measures height from the render output
    /// (probe render). Override [`desired_height`](Component::desired_height)
    /// for components that fill their given area rather than rendering
    /// a fixed amount of content.
    ///
    /// Components that implement [`view()`](Component::view) typically
    /// leave this as the default no-op, since rendering is handled by
    /// the element tree returned from `view()`.
    fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &Self::State) {}

    /// Optional height hint. Return `Some(n)` to declare a fixed height
    /// instead of letting the framework probe-render to measure.
    ///
    /// Override this for components that expand to fill their given area
    /// (e.g., a bordered input box). Most components should leave the
    /// default (`None`) and let the framework measure automatically.
    fn desired_height(&self, _width: u16, _state: &Self::State) -> Option<u16> {
        None
    }

    /// Handle an input event during the **capture** phase (root → focused).
    ///
    /// The capture phase fires before the bubble phase, walking from the
    /// root of the tree down to the focused component. Return
    /// [`EventResult::Consumed`] to stop propagation — the event will
    /// never reach the focused component's [`handle_event`](Component::handle_event)
    /// or any bubble-phase handler.
    ///
    /// Use this for global shortcuts that should take priority over
    /// focused-component handling.
    ///
    /// The state is wrapped in [`Tracked`] — only mutable access via
    /// [`DerefMut`] marks the component dirty. Use [`Tracked::read()`]
    /// for reads that should not trigger a re-render.
    ///
    /// Default: [`EventResult::Ignored`] (pass through to the next node).
    fn handle_event_capture(
        &self,
        _event: &crossterm::event::Event,
        _state: &mut Tracked<Self::State>,
    ) -> EventResult {
        EventResult::Ignored
    }

    /// Handle an input event during the **bubble** phase (focused → root).
    ///
    /// Return [`EventResult::Consumed`] if the event was handled,
    /// or [`EventResult::Ignored`] to let it bubble up to the parent.
    ///
    /// The state is wrapped in [`Tracked`] — only mutable access via
    /// [`DerefMut`] marks the component dirty. Use [`Tracked::read()`]
    /// for reads that should not trigger a re-render.
    fn handle_event(
        &self,
        _event: &crossterm::event::Event,
        _state: &mut Tracked<Self::State>,
    ) -> EventResult {
        EventResult::Ignored
    }

    /// Whether this component can receive focus.
    ///
    /// The framework uses this for Tab cycling — only focusable
    /// components are included in the tab order (depth-first tree order).
    fn is_focusable(&self, _state: &Self::State) -> bool {
        false
    }

    /// Where to position the terminal's hardware cursor when this
    /// component has focus. Returns `(col, row)` relative to the
    /// component's render area, or `None` to hide the cursor.
    ///
    /// This is used by the framework to position the blinking
    /// terminal cursor after rendering (e.g., at the text insertion
    /// point in an input field).
    fn cursor_position(&self, _area: Rect, _state: &Self::State) -> Option<(u16, u16)> {
        None
    }

    /// Create the initial state for this component.
    ///
    /// Returns `None` to use `State::default()`. Override to provide
    /// custom initial state.
    fn initial_state(&self) -> Option<Self::State> {
        None
    }

    /// Insets for the content area within this component's render area.
    ///
    /// The framework lays out children inside the inset region. The
    /// component renders its own chrome (borders, padding) via `render()`
    /// in the full area, then children are rendered within the inner area.
    ///
    /// Default: [`Insets::ZERO`] (children get the full area).
    fn content_inset(&self, _state: &Self::State) -> Insets {
        Insets::ZERO
    }

    /// Layout direction for this component's children.
    ///
    /// Override to `Layout::Horizontal` for horizontal containers.
    /// Default: `Layout::Vertical`.
    fn layout(&self) -> Layout {
        Layout::default()
    }

    /// Width constraint for this component within a horizontal container.
    ///
    /// Override to declare a fixed or custom width. The renderer reads
    /// this at build time to allocate horizontal space.
    ///
    /// Default: [`WidthConstraint::Fill`] (take remaining space).
    fn width_constraint(&self) -> WidthConstraint {
        WidthConstraint::default()
    }

    /// Declare lifecycle effects for this component.
    ///
    /// Called by the framework after build and update. Use the `hooks`
    /// parameter to register intervals, mount/unmount handlers, etc.
    /// The framework clears old effects and applies the new set on
    /// every call.
    ///
    /// Default: no-op (no effects).
    fn lifecycle(&self, _hooks: &mut Hooks<Self::State>, _state: &Self::State) {}

    /// Return child elements for this component.
    ///
    /// The `slot` parameter contains externally-provided children
    /// (from `add_with_children`). The component decides what to do:
    ///
    /// - **Pass through (default):** Return `slot`. Layout containers
    ///   like VStack/HStack use this — they accept external children.
    /// - **Generate own tree:** Ignore slot, return custom Elements.
    ///   A Spinner generates its own HStack with frame + label.
    /// - **Wrap slot:** Incorporate slot into a larger tree. A Banner
    ///   wraps slot children in a header + content layout.
    /// - **No children:** Return None for a pure leaf component.
    fn children(&self, _state: &Self::State, slot: Option<Elements>) -> Option<Elements> {
        slot
    }

    /// Whether this component uses [`view()`](Component::view) to define
    /// its element tree.
    ///
    /// Override to return `true` when implementing `view()`. When true,
    /// the framework calls `view()` instead of `render()`, `children()`,
    /// and `content_inset()`, treating this component as a transparent
    /// container whose rendering is fully expressed in the returned tree.
    fn uses_view(&self) -> bool {
        false
    }

    /// Return the element tree for this component.
    ///
    /// Only called when [`uses_view()`](Component::uses_view) returns `true`.
    /// The `children` parameter contains slot children passed by the parent
    /// (from `element!` braces or `add_with_children`).
    ///
    /// When a component uses `view()`:
    /// - `render()` is not called (chrome is expressed via [`View`](crate::View)
    ///   or [`Canvas`](crate::Canvas) in the returned tree)
    /// - `content_inset()` is not used (insets are part of the tree)
    /// - `children()` is not called (slot children arrive here directly)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use eye_declare::{Component, Elements, View, Canvas, element};
    /// use ratatui_widgets::borders::BorderType;
    ///
    /// struct Card { title: String }
    ///
    /// impl Component for Card {
    ///     type State = ();
    ///
    ///     fn uses_view(&self) -> bool { true }
    ///
    ///     fn view(&self, _state: &(), children: Elements) -> Elements {
    ///         element! {
    ///             View(border: BorderType::Rounded, title: self.title.clone()) {
    ///                 #(children)
    ///             }
    ///         }
    ///     }
    /// }
    /// ```
    fn view(&self, _state: &Self::State, _children: Elements) -> Elements {
        Elements::new()
    }
}

/// Vertical stack container — children render top-to-bottom.
///
/// `VStack` renders nothing itself; it exists purely to group children.
/// Each child receives the full parent width and its own measured height.
///
/// This is the default layout direction and the implicit root of every
/// renderer. Use it explicitly when you need a named group:
///
/// ```ignore
/// element! {
///     VStack {
///         Spinner(label: "Step 1...")
///         Spinner(label: "Step 2...")
///     }
/// }
/// ```
#[derive(Default)]
pub struct VStack;

impl Component for VStack {
    type State = ();

    fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &()) {}
}

impl_slot_children!(VStack);

/// Horizontal stack container — children render left-to-right.
///
/// `HStack` renders nothing itself; it lays children out horizontally.
/// Each child's width is determined by its [`WidthConstraint`]:
/// [`Fixed(n)`](WidthConstraint::Fixed) reserves exactly `n` columns,
/// while [`Fill`](WidthConstraint::Fill) (the default) splits remaining
/// space equally among all `Fill` siblings.
///
/// ```ignore
/// element! {
///     HStack {
///         Column(width: Fixed(3)) {
///             Spinner(label: "")
///         }
///         Column {
///             "Status: OK"
///         }
///     }
/// }
/// ```
#[derive(Default)]
pub struct HStack;

impl Component for HStack {
    type State = ();

    fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &()) {}

    fn layout(&self) -> Layout {
        Layout::Horizontal
    }
}

impl_slot_children!(HStack);

/// A width-constrained wrapper for children inside an [`HStack`].
///
/// `Column` renders nothing itself — it passes children through and
/// declares a [`WidthConstraint`] that the `HStack` uses for layout.
/// Defaults to [`Fill`](WidthConstraint::Fill) if no width is specified.
///
/// ```ignore
/// element! {
///     HStack {
///         Column(width: Fixed(20)) {
///             "Sidebar"
///         }
///         Column {
///             // Fill: takes remaining width
///             "Main content"
///         }
///     }
/// }
/// ```
pub struct Column {
    /// The width constraint for this column. Defaults to [`Fill`](WidthConstraint::Fill).
    pub width: WidthConstraint,
}

impl Default for Column {
    fn default() -> Self {
        Self {
            width: WidthConstraint::Fill,
        }
    }
}

impl Component for Column {
    type State = ();

    fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &()) {}

    fn width_constraint(&self) -> WidthConstraint {
        self.width
    }
}

impl_slot_children!(Column);

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
