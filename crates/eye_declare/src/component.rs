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
///     fn desired_height(&self, _: u16, _: &()) -> u16 { 0 }
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

/// Result of handling an input event in [`Component::handle_event`].
///
/// Controls whether the event continues bubbling up the component tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// The event was handled by this component. Stops propagation.
    Consumed,
    /// The event was not handled. The framework passes it to the parent.
    Ignored,
}

/// Wrapper that automatically marks component state dirty on mutation.
///
/// The framework wraps each component's `State` in `Tracked<S>`.
/// Read access via [`Deref`] is free — it does not set the dirty flag.
/// Write access via [`DerefMut`] automatically marks the state dirty,
/// telling the framework this component needs to re-render.
///
/// You will interact with `Tracked` when using the imperative
/// [`InlineRenderer`](crate::InlineRenderer) API:
///
/// ```ignore
/// let id = renderer.push(Spinner::new("loading..."));
///
/// // DerefMut triggers dirty flag automatically
/// renderer.state_mut::<Spinner>(id).tick();
/// ```
///
/// Inside [`Component::handle_event`] and lifecycle hooks, the framework
/// provides the inner `State` directly — `Tracked` is transparent.
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
///
///     fn desired_height(&self, _width: u16, _state: &()) -> u16 { 1 }
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
    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State);

    /// How tall does this component want to be at the given width?
    fn desired_height(&self, width: u16, state: &Self::State) -> u16;

    /// Handle an input event, potentially mutating state.
    ///
    /// Return [`EventResult::Consumed`] if the event was handled,
    /// or [`EventResult::Ignored`] to let it bubble up to the parent.
    /// State mutations through the `&mut` reference automatically
    /// mark the component dirty for re-rendering.
    fn handle_event(
        &self,
        _event: &crossterm::event::Event,
        _state: &mut Self::State,
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
}

/// Vertical stack container — children render top-to-bottom.
///
/// `VStack` renders nothing itself; it exists purely to group children.
/// Each child receives the full parent width and its own
/// [`desired_height`](Component::desired_height).
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

    fn desired_height(&self, _width: u16, _state: &()) -> u16 {
        0
    }
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

    fn desired_height(&self, _width: u16, _state: &()) -> u16 {
        0
    }

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

    fn desired_height(&self, _width: u16, _state: &()) -> u16 {
        0
    }

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
