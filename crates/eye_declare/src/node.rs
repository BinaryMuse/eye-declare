use std::any::{Any, TypeId};
use std::panic::Location;
use std::time::{Duration, Instant};

use ratatui_core::{buffer::Buffer, layout::Rect};

use crate::component::{Component, Tracked};
use crate::context::{ContextMap, ProvidedContexts};
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
    /// Return the component's props as `&dyn Any` for hook callbacks.
    fn props_as_any(&self) -> &dyn Any;
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
    fn width_constraint_erased(&self) -> WidthConstraint;
    /// Combined lifecycle + view: collect hooks and produce element tree in one call.
    fn update_erased(
        &self,
        tracked_state: &mut dyn Any,
        context: &ContextMap,
        children: Elements,
    ) -> (LifecycleOutput, Elements);
}

impl<C: Component> AnyComponent for C {
    fn props_as_any(&self) -> &dyn Any {
        Component::props_as_any(self)
    }

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

    fn width_constraint_erased(&self) -> WidthConstraint {
        self.width_constraint()
    }

    fn update_erased(
        &self,
        tracked_state: &mut dyn Any,
        context: &ContextMap,
        children: Elements,
    ) -> (LifecycleOutput, Elements) {
        let tracked = tracked_state
            .downcast_mut::<Tracked<C::State>>()
            .expect("state type mismatch in update_erased");

        // Phase 1: call update() with immutable state and fresh hooks
        let (hooks_output, elements) = {
            let state: &C::State = tracked;
            let mut hooks = Hooks::<C, C::State>::new();
            let elements = self.update(&mut hooks, state, children);
            (hooks.decompose(), elements)
        };

        // Phase 2: run context consumers with mutable tracked state
        let props_any: &dyn Any = Component::props_as_any(self);
        for consumer in hooks_output.consumers {
            consumer(context, props_any, tracked);
        }

        (
            LifecycleOutput {
                effects: hooks_output.effects,
                autofocus: hooks_output.autofocus,
                focus_scope: hooks_output.focus_scope,
                provided: hooks_output.provided,
                focusable: hooks_output.focusable,
                cursor_hook: hooks_output.cursor_hook,
                event_hook: hooks_output.event_hook,
                capture_hook: hooks_output.capture_hook,
                layout: hooks_output.layout,
                width_constraint: hooks_output.width_constraint,
                height_hint: hooks_output.height_hint,
                desired_height_hook: hooks_output.desired_height_hook,
            },
            elements,
        )
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
/// signature: `Fn(&P, &mut S)`.
pub(crate) trait AnyEffectHandler: Send + Sync {
    fn call(&self, component: &dyn Any, tracked_state: &mut dyn Any);
}

/// Typed wrapper that captures a closure operating on `&P` and
/// `&mut Tracked<S>`, downcasting both at call time. The handler
/// receives `Tracked` directly so it can use `state.read()` for
/// non-mutating access (avoiding unnecessary dirty marking) or
/// mutate through `DerefMut` which marks dirty automatically.
type EffectHandlerFn<P, S> = Box<dyn Fn(&P, &mut Tracked<S>) + Send + Sync>;

pub(crate) struct TypedEffectHandler<P: 'static, S: 'static> {
    pub(crate) handler: EffectHandlerFn<P, S>,
}

impl<P: Send + Sync + 'static, S: Send + Sync + 'static> AnyEffectHandler
    for TypedEffectHandler<P, S>
{
    fn call(&self, component: &dyn Any, tracked_state: &mut dyn Any) {
        let props = component
            .downcast_ref::<P>()
            .expect("props type mismatch in effect handler");
        let tracked = tracked_state
            .downcast_mut::<Tracked<S>>()
            .expect("state type mismatch in effect handler");
        (self.handler)(props, tracked);
    }
}

/// Identifies the source location of a hook call.
///
/// Used to match effects across re-renders so that state (like `last_tick`
/// on intervals) is preserved even when effects are conditionally registered.
/// Derived from `#[track_caller]` on hook methods.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CallSite {
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
}

impl CallSite {
    pub fn from_location(loc: &'static Location<'static>) -> Self {
        Self {
            file: loc.file(),
            line: loc.line(),
            column: loc.column(),
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

/// Type-erased event handler declared via hooks.
pub(crate) trait AnyEventHook: Send + Sync {
    fn call(
        &self,
        event: &crossterm::event::Event,
        component: &dyn Any,
        tracked_state: &mut dyn Any,
    ) -> crate::component::EventResult;
}

type EventHookFn<P, S> = Box<
    dyn Fn(&crossterm::event::Event, &P, &mut Tracked<S>) -> crate::component::EventResult
        + Send
        + Sync,
>;

/// Typed wrapper for a hook-declared event handler.
pub(crate) struct TypedEventHook<P: 'static, S: 'static> {
    pub(crate) handler: EventHookFn<P, S>,
}

impl<P: Send + Sync + 'static, S: Send + Sync + 'static> AnyEventHook for TypedEventHook<P, S> {
    fn call(
        &self,
        event: &crossterm::event::Event,
        component: &dyn Any,
        tracked_state: &mut dyn Any,
    ) -> crate::component::EventResult {
        let props = component
            .downcast_ref::<P>()
            .expect("props type mismatch in event hook");
        let tracked = tracked_state
            .downcast_mut::<Tracked<S>>()
            .expect("state type mismatch in event hook");
        (self.handler)(event, props, tracked)
    }
}

/// Type-erased cursor position hook.
pub(crate) trait AnyCursorHook: Send + Sync {
    fn call(&self, area: Rect, component: &dyn Any, state: &dyn Any) -> Option<(u16, u16)>;
}

type CursorHookFn<P, S> = Box<dyn Fn(Rect, &P, &S) -> Option<(u16, u16)> + Send + Sync>;

/// Typed wrapper for a hook-declared cursor position.
pub(crate) struct TypedCursorHook<P: 'static, S: 'static> {
    pub(crate) handler: CursorHookFn<P, S>,
}

impl<P: Send + Sync + 'static, S: Send + Sync + 'static> AnyCursorHook for TypedCursorHook<P, S> {
    fn call(&self, area: Rect, component: &dyn Any, state: &dyn Any) -> Option<(u16, u16)> {
        let props = component
            .downcast_ref::<P>()
            .expect("props type mismatch in cursor hook");
        let state = state
            .downcast_ref::<S>()
            .expect("state type mismatch in cursor hook");
        (self.handler)(area, props, state)
    }
}

/// Type-erased desired height hook.
pub(crate) trait AnyDesiredHeightHook: Send + Sync {
    fn call(&self, width: u16, component: &dyn Any, state: &dyn Any) -> Option<u16>;
}

type DesiredHeightHookFn<P, S> = Box<dyn Fn(u16, &P, &S) -> Option<u16> + Send + Sync>;

/// Typed wrapper for a hook-declared desired height callback.
pub(crate) struct TypedDesiredHeightHook<P: 'static, S: 'static> {
    pub(crate) handler: DesiredHeightHookFn<P, S>,
}

impl<P: Send + Sync + 'static, S: Send + Sync + 'static> AnyDesiredHeightHook
    for TypedDesiredHeightHook<P, S>
{
    fn call(&self, width: u16, component: &dyn Any, state: &dyn Any) -> Option<u16> {
        let props = component
            .downcast_ref::<P>()
            .expect("props type mismatch in desired_height hook");
        let state = state
            .downcast_ref::<S>()
            .expect("state type mismatch in desired_height hook");
        (self.handler)(width, props, state)
    }
}

/// Output of the update (lifecycle + view) call.
pub(crate) struct LifecycleOutput {
    pub effects: Vec<Effect>,
    pub autofocus: bool,
    pub focus_scope: bool,
    pub provided: ProvidedContexts,
    pub focusable: Option<bool>,
    pub cursor_hook: Option<Box<dyn AnyCursorHook>>,
    pub event_hook: Option<Box<dyn AnyEventHook>>,
    pub capture_hook: Option<Box<dyn AnyEventHook>>,
    pub layout: Option<Layout>,
    pub width_constraint: Option<WidthConstraint>,
    pub height_hint: Option<u16>,
    pub desired_height_hook: Option<Box<dyn AnyDesiredHeightHook>>,
}

/// A registered effect for a node.
pub(crate) struct Effect {
    pub handler: Box<dyn AnyEffectHandler>,
    pub kind: EffectKind,
    pub call_site: CallSite,
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
    /// Hook-declared focusability (overrides component trait method).
    pub hook_focusable: Option<bool>,
    /// Hook-declared cursor position callback.
    pub hook_cursor: Option<Box<dyn AnyCursorHook>>,
    /// Hook-declared event handler (bubble phase).
    pub hook_event: Option<Box<dyn AnyEventHook>>,
    /// Hook-declared event handler (capture phase).
    pub hook_capture: Option<Box<dyn AnyEventHook>>,
    /// Hook-declared height hint (overrides component's desired_height).
    pub hook_height_hint: Option<u16>,
    /// Hook-declared desired height callback (overrides component's desired_height).
    /// Takes priority over `hook_height_hint` since it's width-aware.
    pub hook_desired_height: Option<Box<dyn AnyDesiredHeightHook>>,
    /// Whether this node was built with slot children from a parent.
    /// Nodes with slot children cannot be safely re-reconciled without
    /// the parent's element tree, so the pre-render refresh skips them.
    pub has_slot: bool,
}

impl Node {
    pub fn new<C: Component>(component: C) -> Self {
        let state: Box<dyn AnyTrackedState> =
            Box::new(Tracked::new(component.initial_state().unwrap_or_default()));
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
            layout_rect: None,
            element_type_id: None,
            key: None,
            layout: Layout::default(),
            width_constraint: WidthConstraint::default(),
            autofocus: false,
            focus_scope: false,
            hook_focusable: None,
            hook_cursor: None,
            hook_event: None,
            hook_capture: None,
            hook_height_hint: None,
            hook_desired_height: None,
            has_slot: false,
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
