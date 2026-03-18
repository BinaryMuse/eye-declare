use std::ops::{Deref, DerefMut};

use ratatui_core::{buffer::Buffer, layout::Rect};

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

    /// Create the initial state for this component.
    fn initial_state(&self) -> Self::State;
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
