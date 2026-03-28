use std::any::{Any, TypeId};
use std::time::{Duration, Instant};

use ratatui_core::{buffer::Buffer, layout::Rect};

use crate::component::{Component, Tracked};
use crate::context::ContextMap;
use crate::element::Elements;
use crate::hooks::Hooks;
use crate::insets::Insets;

/// Opaque handle identifying a node in the component tree.
///
/// Returned by methods like [`InlineRenderer::push`](crate::InlineRenderer::push)
/// and used to reference specific components for state access, removal,
/// and other operations.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub(crate) usize);

/// Layout direction for a container's children.
///
/// Most components use the default [`Vertical`](Layout::Vertical) layout.
/// Override [`Component::layout`](crate::Component::layout) to return
/// `Horizontal` for side-by-side children (or use [`HStack`](crate::HStack)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// Children stack top-to-bottom, each receiving the full parent width.
    #[default]
    Vertical,
    /// Children lay out left-to-right, widths allocated by [`WidthConstraint`].
    Horizontal,
}

/// How a child claims horizontal space inside an [`HStack`](crate::HStack).
///
/// Set via [`ElementHandle::width`](crate::ElementHandle::width) or the
/// `width:` prop on [`Column`](crate::Column) in the `element!` macro.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WidthConstraint {
    /// Reserve exactly `n` columns for this child.
    Fixed(u16),
    /// Take remaining space, split equally among all `Fill` siblings.
    #[default]
    Fill,
}

/// Type-erased component operations.
pub(crate) trait AnyComponent: Send + Sync {
    fn render_erased(&self, area: Rect, buf: &mut Buffer, state: &dyn Any);
    fn desired_height_erased(&self, width: u16, state: &dyn Any) -> Option<u16>;
    fn handle_event_capture_erased(
        &self,
        event: &crossterm::event::Event,
        tracked_state: &mut dyn Any,
    ) -> crate::component::EventResult;
    fn handle_event_erased(
        &self,
        event: &crossterm::event::Event,
        tracked_state: &mut dyn Any,
    ) -> crate::component::EventResult;
    fn cursor_position_erased(&self, area: Rect, state: &dyn Any) -> Option<(u16, u16)>;
    fn is_focusable_erased(&self, state: &dyn Any) -> bool;
    fn content_inset_erased(&self, state: &dyn Any) -> Insets;
    fn children_erased(&self, state: &dyn Any, slot: Option<Elements>) -> Option<Elements>;
    fn uses_view_erased(&self) -> bool;
    fn view_erased(&self, state: &dyn Any, children: Elements) -> Elements;
    fn width_constraint_erased(&self) -> WidthConstraint;
    fn lifecycle_erased(
        &self,
        tracked_state: &mut dyn Any,
        context: &ContextMap,
    ) -> LifecycleOutput;
}

impl<C: Component> AnyComponent for C {
    fn render_erased(&self, area: Rect, buf: &mut Buffer, state: &dyn Any) {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in render_erased");
        self.render(area, buf, state);
    }

    fn desired_height_erased(&self, width: u16, state: &dyn Any) -> Option<u16> {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in desired_height_erased");
        self.desired_height(width, state)
    }

    fn handle_event_capture_erased(
        &self,
        event: &crossterm::event::Event,
        tracked_state: &mut dyn Any,
    ) -> crate::component::EventResult {
        let tracked = tracked_state
            .downcast_mut::<Tracked<C::State>>()
            .expect("state type mismatch in handle_event_capture_erased");
        self.handle_event_capture(event, tracked)
    }

    fn handle_event_erased(
        &self,
        event: &crossterm::event::Event,
        tracked_state: &mut dyn Any,
    ) -> crate::component::EventResult {
        let tracked = tracked_state
            .downcast_mut::<Tracked<C::State>>()
            .expect("state type mismatch in handle_event_erased");
        self.handle_event(event, tracked)
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

    fn content_inset_erased(&self, state: &dyn Any) -> Insets {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in content_inset_erased");
        self.content_inset(state)
    }

    fn children_erased(&self, state: &dyn Any, slot: Option<Elements>) -> Option<Elements> {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in children_erased");
        self.children(state, slot)
    }

    fn uses_view_erased(&self) -> bool {
        self.uses_view()
    }

    fn view_erased(&self, state: &dyn Any, children: Elements) -> Elements {
        let state = state
            .downcast_ref::<C::State>()
            .expect("state type mismatch in view_erased");
        self.view(state, children)
    }

    fn width_constraint_erased(&self) -> WidthConstraint {
        self.width_constraint()
    }

    fn lifecycle_erased(
        &self,
        tracked_state: &mut dyn Any,
        context: &ContextMap,
    ) -> LifecycleOutput {
        let tracked = tracked_state
            .downcast_mut::<Tracked<C::State>>()
            .expect("state type mismatch in lifecycle_erased");

        // Phase 1: collect hooks with immutable state reference
        let (effects, autofocus, focus_scope, provided, consumers) = {
            let state: &C::State = tracked;
            let mut hooks = Hooks::<C::State>::new();
            self.lifecycle(&mut hooks, state);
            hooks.decompose()
        };

        // Phase 2: run context consumers with mutable tracked state
        for consumer in consumers {
            consumer(context, tracked);
        }

        LifecycleOutput {
            effects,
            autofocus,
            focus_scope,
            provided,
        }
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
    Interval {
        interval: Duration,
        last_tick: Instant,
    },
    /// One-shot callback. Fires after element build completes.
    OnMount,
    /// One-shot callback. Fires when node is tombstoned.
    OnUnmount,
}

/// Output of a lifecycle() call.
pub(crate) struct LifecycleOutput {
    pub effects: Vec<Effect>,
    pub autofocus: bool,
    pub focus_scope: bool,
    pub provided: Vec<(TypeId, Box<dyn Any + Send + Sync>)>,
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
    /// Set during measure_height when a leaf was probe-rendered.
    /// render_node uses the cached probe buffer instead of rendering again.
    pub probe_rendered: bool,
    /// Whether this node uses `view()` to define its element tree.
    /// Initialised from `component.uses_view()` in `Node::new`, kept in
    /// sync by `swap_component` and `resolve_children`. When true,
    /// `render_node` skips the component's `render()` and `content_inset()`,
    /// treating the node as a transparent container.
    pub uses_view: bool,
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
    /// Whether this node should receive focus on mount.
    pub autofocus: bool,
    /// Whether this node is a focus scope boundary (Tab cycling trap).
    pub focus_scope: bool,
}

impl Node {
    pub fn new<C: Component>(component: C) -> Self {
        let state: Box<dyn AnyTrackedState> =
            Box::new(Tracked::new(component.initial_state().unwrap_or_default()));
        let uses_view = component.uses_view();

        Self {
            component: Box::new(component),
            state,
            frozen: false,
            cached_buffer: None,
            last_height: None,
            children: Vec::new(),
            parent: None,
            force_dirty: false,
            probe_rendered: false,
            uses_view,
            layout_rect: None,
            element_type_id: None,
            key: None,
            layout: Layout::default(),
            width_constraint: WidthConstraint::default(),
            autofocus: false,
            focus_scope: false,
        }
    }

    /// Whether this node has children (is a container).
    pub fn is_container(&self) -> bool {
        !self.children.is_empty()
    }
}

/// Arena that stores nodes with slot reuse.
///
/// Tombstoned nodes are freed and their slots recycled for new allocations,
/// preventing unbounded growth in long-running applications.
pub(crate) struct NodeArena {
    slots: Vec<Option<Node>>,
    free: Vec<usize>,
}

impl NodeArena {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    /// Allocate a slot for a node, reusing a freed slot if available.
    pub fn alloc(&mut self, node: Node) -> NodeId {
        if let Some(idx) = self.free.pop() {
            self.slots[idx] = Some(node);
            NodeId(idx)
        } else {
            let idx = self.slots.len();
            self.slots.push(Some(node));
            NodeId(idx)
        }
    }

    /// Free a slot, making it available for reuse.
    ///
    /// # Panics
    /// Panics if the slot is already empty (double free).
    pub fn free(&mut self, id: NodeId) {
        assert!(
            self.slots[id.0].is_some(),
            "double free of NodeId({})",
            id.0
        );
        self.slots[id.0] = None;
        self.free.push(id.0);
    }

    /// Check whether a node slot is still live (not freed).
    pub fn is_live(&self, id: NodeId) -> bool {
        self.slots.get(id.0).is_some_and(|s| s.is_some())
    }

    /// Iterate over all live nodes mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.slots.iter_mut().filter_map(|slot| slot.as_mut())
    }
}

impl std::ops::Index<NodeId> for NodeArena {
    type Output = Node;

    fn index(&self, id: NodeId) -> &Node {
        self.slots[id.0].as_ref().expect("accessed a freed NodeId")
    }
}

impl std::ops::IndexMut<NodeId> for NodeArena {
    fn index_mut(&mut self, id: NodeId) -> &mut Node {
        self.slots[id.0].as_mut().expect("accessed a freed NodeId")
    }
}
