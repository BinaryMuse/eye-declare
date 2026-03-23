use std::ops::{Deref, DerefMut};

use ratatui_core::{buffer::Buffer, layout::Rect};

use crate::element::Elements;
use crate::hooks::Hooks;
use crate::insets::Insets;

/// Result of handling an input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// Event was consumed by this component.
    Consumed,
    /// Event was not handled; propagate to parent.
    Ignored,
}

/// Wrapper that automatically marks state dirty on `&mut` access.
///
/// The framework wraps each component's state in `Tracked<S>`.
/// Read access (`Deref`) does not set dirty. Write access (`DerefMut`)
/// sets the dirty flag, so the framework knows to re-render.
pub struct Tracked<S> {
    inner: S,
    dirty: bool,
}

impl<S> Tracked<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            dirty: true, // start dirty so first render always happens
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

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
/// Components are stateless renderers — state lives in the associated
/// `State` type, managed by the framework via [`Tracked<S>`].
pub trait Component: Send + Sync + 'static {
    /// State type for this component. The framework wraps it in
    /// `Tracked<S>` for automatic dirty detection.
    type State: Send + Sync + 'static;

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
    fn initial_state(&self) -> Self::State;

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
    fn children(
        &self,
        _state: &Self::State,
        slot: Option<Elements>,
    ) -> Option<Elements> {
        slot
    }
}

/// A no-op container component for vertical stacking.
///
/// VStack renders nothing itself — children determine all sizing
/// and content. Used as the implicit root component of a Renderer.
pub struct VStack;

impl Component for VStack {
    type State = ();

    fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &()) {}

    fn desired_height(&self, _width: u16, _state: &()) -> u16 {
        0
    }

    fn initial_state(&self) -> () {}
}

/// A no-op container component for horizontal layout.
///
/// HStack renders nothing itself — children are laid out
/// left-to-right with widths determined by their
/// [`WidthConstraint`](crate::node::WidthConstraint).
/// The layout direction is set on the Node by the element builder.
pub struct HStack;

impl Component for HStack {
    type State = ();

    fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &()) {}

    fn desired_height(&self, _width: u16, _state: &()) -> u16 {
        0
    }

    fn initial_state(&self) -> () {}
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
