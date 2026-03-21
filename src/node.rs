use std::any::{Any, TypeId};
use std::time::{Duration, Instant};

use ratatui_core::{buffer::Buffer, layout::Rect};

use crate::component::{Component, Tracked};

/// Opaque handle into the node arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub(crate) usize);

/// Layout direction for a container node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// Children stack top-to-bottom, each getting full parent width.
    #[default]
    Vertical,
    /// Children lay left-to-right, width allocated per constraints.
    Horizontal,
}

/// Width constraint for a child within a horizontal container.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WidthConstraint {
    /// Exact width in columns.
    Fixed(u16),
    /// Take remaining space (split equally among Fill children).
    #[default]
    Fill,
}

/// Type-erased component operations.
pub(crate) trait AnyComponent: Send + Sync {
    fn render_erased(&self, area: Rect, buf: &mut Buffer, state: &dyn Any);
    fn desired_height_erased(&self, width: u16, state: &dyn Any) -> u16;
    fn handle_event_erased(
        &self,
        event: &crossterm::event::Event,
        tracked_state: &mut dyn Any,
    ) -> crate::component::EventResult;
    fn cursor_position_erased(&self, area: Rect, state: &dyn Any) -> Option<(u16, u16)>;
    fn is_focusable_erased(&self, state: &dyn Any) -> bool;
}

impl<C: Component> AnyComponent for C {
    fn render_erased(&self, area: Rect, buf: &mut Buffer, state: &dyn Any) {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in render_erased");
        self.render(area, buf, state);
    }

    fn desired_height_erased(&self, width: u16, state: &dyn Any) -> u16 {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in desired_height_erased");
        self.desired_height(width, state)
    }

    fn handle_event_erased(
        &self,
        event: &crossterm::event::Event,
        tracked_state: &mut dyn Any,
    ) -> crate::component::EventResult {
        let tracked = tracked_state
            .downcast_mut::<Tracked<C::State>>()
            .expect("state type mismatch in handle_event_erased");
        // DerefMut on Tracked marks dirty automatically
        self.handle_event(event, &mut *tracked)
    }

    fn cursor_position_erased(&self, area: Rect, state: &dyn Any) -> Option<(u16, u16)> {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in cursor_position_erased");
        self.cursor_position(area, state)
    }

    fn is_focusable_erased(&self, state: &dyn Any) -> bool {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in is_focusable_erased");
        self.is_focusable(state)
    }
}

/// Type-erased tracked state operations.
pub(crate) trait AnyTrackedState: Send + Sync {
    #[allow(dead_code)]
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    #[allow(dead_code)]
    fn is_dirty(&self) -> bool;
    fn clear_dirty(&mut self);
    /// Get a reference to the inner state (unwrapped from Tracked) as Any.
    fn inner_as_any(&self) -> &dyn Any;
}

impl<S: Send + Sync + 'static> AnyTrackedState for Tracked<S> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn is_dirty(&self) -> bool {
        Tracked::is_dirty(self)
    }

    fn clear_dirty(&mut self) {
        Tracked::clear_dirty(self);
    }

    fn inner_as_any(&self) -> &dyn Any {
        use std::ops::Deref;
        self.deref() as &dyn Any
    }
}

/// Type-erased effect handler. Used for all effect types (interval,
/// mount, unmount, etc.) since they all share the same callback
/// signature: `Fn(&mut C::State)`.
pub(crate) trait AnyEffectHandler: Send + Sync {
    fn call(&self, tracked_state: &mut dyn Any);
}

/// Typed wrapper that captures a closure operating on `S` and downcasts
/// the type-erased `Tracked<S>` at call time. `DerefMut` on `Tracked`
/// automatically marks state dirty when the handler fires.
pub(crate) struct TypedEffectHandler<S: 'static> {
    pub(crate) handler: Box<dyn Fn(&mut S) + Send + Sync>,
}

impl<S: Send + Sync + 'static> AnyEffectHandler for TypedEffectHandler<S> {
    fn call(&self, tracked_state: &mut dyn Any) {
        if let Some(tracked) = tracked_state.downcast_mut::<Tracked<S>>() {
            use std::ops::DerefMut;
            (self.handler)(tracked.deref_mut());
        }
    }
}

/// What kind of effect this is, and when it should fire.
pub(crate) enum EffectKind {
    /// Periodic callback. Fires when interval elapses during `tick()`.
    Interval { interval: Duration, last_tick: Instant },
    /// One-shot callback. Fires after element build completes.
    OnMount,
    /// One-shot callback. Fires when node is tombstoned.
    OnUnmount,
}

/// A registered effect for a node.
pub(crate) struct Effect {
    pub handler: Box<dyn AnyEffectHandler>,
    pub kind: EffectKind,
}

/// A node in the component tree. Framework-internal.
pub(crate) struct Node {
    pub component: Box<dyn AnyComponent>,
    pub state: Box<dyn AnyTrackedState>,
    pub frozen: bool,
    pub cached_buffer: Option<Buffer>,
    pub last_height: Option<u16>,
    pub children: Vec<NodeId>,
    pub parent: Option<NodeId>,
    /// Set by the framework to force re-render (e.g., after width change).
    /// Cleared after rendering.
    pub force_dirty: bool,
    /// The area this node was last rendered into (set by the framework).
    pub layout_rect: Option<Rect>,
    /// TypeId of the Element that created this node (for reconciliation matching).
    /// None for nodes created via the imperative API.
    pub element_type_id: Option<TypeId>,
    /// Optional key for stable identity across rebuilds.
    pub key: Option<String>,
    /// Layout direction for this container. Set by element builders.
    pub layout: Layout,
    /// Width constraint for this node within a horizontal parent.
    pub width_constraint: WidthConstraint,
}

impl Node {
    pub fn new<C: Component>(component: C) -> Self {
        let state = Tracked::new(component.initial_state());
        Self {
            component: Box::new(component),
            state: Box::new(state),
            frozen: false,
            cached_buffer: None,
            last_height: None,
            children: Vec::new(),
            parent: None,
            force_dirty: false,
            layout_rect: None,
            element_type_id: None,
            key: None,
            layout: Layout::default(),
            width_constraint: WidthConstraint::default(),
        }
    }

    /// Whether this node has children (is a container).
    pub fn is_container(&self) -> bool {
        !self.children.is_empty()
    }
}
