use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
};

use crate::component::{Component, EventResult, Tracked, VStack};
use crate::element::{ElementEntry, Elements};
use crate::frame::Frame;
use crate::node::{Effect, EffectKind, Layout, Node, NodeId, TypedEffectHandler, WidthConstraint};

/// Manages a tree of components and renders them into a Frame.
///
/// The tree has an implicit root node (a VStack) created automatically.
/// Components are added as children of the root or of other nodes.
/// Children are laid out vertically within their parent's area.
pub struct Renderer {
    nodes: Vec<Node>,
    root: NodeId,
    width: u16,
    focused: Option<NodeId>,
    /// After rendering, the absolute cursor position for the focused
    /// component (if it returns one from cursor_position).
    cursor_hint: Option<(u16, u16)>,
    /// Registered effects per node. Multiple effects per node supported.
    effects: HashMap<NodeId, Vec<Effect>>,
}

impl Renderer {
    /// Create a new renderer with the given terminal width.
    /// An implicit VStack root node is created automatically.
    pub fn new(width: u16) -> Self {
        let mut nodes = Vec::new();
        let root = NodeId(0);
        nodes.push(Node::new(VStack));
        // Root starts clean since VStack has no visible content
        nodes[0].state.clear_dirty();
        Self { nodes, root, width, focused: None, cursor_hint: None, effects: HashMap::new() }
    }

    /// The root node's ID.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Add a component as a child of the given parent. Returns its NodeId.
    pub fn append_child<C: Component>(&mut self, parent: NodeId, component: C) -> NodeId {
        let id = NodeId(self.nodes.len());
        let mut node = Node::new(component);
        node.parent = Some(parent);
        self.nodes.push(node);
        self.nodes[parent.0].children.push(id);
        id
    }

    /// Shorthand: add a component as a child of the root. Returns its NodeId.
    pub fn push<C: Component>(&mut self, component: C) -> NodeId {
        self.append_child(self.root, component)
    }

    /// Access a component's tracked state for mutation.
    ///
    /// Mutation via `DerefMut` automatically marks the state dirty.
    ///
    /// # Panics
    /// Panics if the NodeId is invalid or the state type doesn't match.
    pub fn state_mut<C: Component>(&mut self, id: NodeId) -> &mut Tracked<C::State> {
        let node = &mut self.nodes[id.0];
        node.state
            .as_any_mut()
            .downcast_mut::<Tracked<C::State>>()
            .expect("state type mismatch in state_mut")
    }

    /// Freeze a component. Frozen components use their cached buffer
    /// and are not re-rendered on subsequent frames.
    pub fn freeze(&mut self, id: NodeId) {
        self.nodes[id.0].frozen = true;
    }

    /// Set the layout direction for a container node.
    pub fn set_layout(&mut self, id: NodeId, layout: Layout) {
        self.nodes[id.0].layout = layout;
    }

    /// List the children of a node.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id.0].children
    }

    /// Set which component has focus for event routing.
    pub fn set_focus(&mut self, id: NodeId) {
        self.focused = Some(id);
    }

    /// Clear focus (no component receives events).
    pub fn clear_focus(&mut self) {
        self.focused = None;
    }

    /// The currently focused component, if any.
    pub fn focus(&self) -> Option<NodeId> {
        self.focused
    }

    /// Deliver an event to the focused component.
    ///
    /// Tab and Shift-Tab are intercepted for focus cycling among
    /// focusable components (depth-first tree order). All other events
    /// are delivered to the focused component with bubble-up to parents.
    ///
    /// Returns [`EventResult::Ignored`] if no component is focused
    /// or no component consumed the event.
    pub fn handle_event(&mut self, event: &crossterm::event::Event) -> EventResult {
        // Intercept Tab / Shift-Tab for focus cycling
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        }) = event
        {
            let is_tab = *code == KeyCode::Tab && !modifiers.contains(KeyModifiers::SHIFT);
            let is_backtab = *code == KeyCode::BackTab
                || (*code == KeyCode::Tab && modifiers.contains(KeyModifiers::SHIFT));

            if is_tab || is_backtab {
                self.cycle_focus(is_backtab);
                return EventResult::Consumed;
            }
        }

        let Some(focused) = self.focused else {
            return EventResult::Ignored;
        };

        // Try the focused node, then bubble up through parents
        let mut current = Some(focused);
        while let Some(id) = current {
            let node = &mut self.nodes[id.0];
            if node.frozen {
                current = node.parent;
                continue;
            }
            let state_any = node.state.as_any_mut();
            let result = node.component.handle_event_erased(event, state_any);
            if result == EventResult::Consumed {
                return EventResult::Consumed;
            }
            current = self.nodes[id.0].parent;
        }

        EventResult::Ignored
    }

    /// Collect focusable node IDs in depth-first tree order.
    fn focusable_nodes(&self) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.collect_focusable(self.root, &mut result);
        result
    }

    fn collect_focusable(&self, id: NodeId, result: &mut Vec<NodeId>) {
        let node = &self.nodes[id.0];
        if node.frozen {
            return;
        }
        let state = node.state.inner_as_any();
        if node.component.is_focusable_erased(state) {
            result.push(id);
        }
        for &child in &node.children {
            self.collect_focusable(child, result);
        }
    }

    /// Cycle focus to the next (or previous) focusable component.
    fn cycle_focus(&mut self, reverse: bool) {
        let focusable = self.focusable_nodes();
        if focusable.is_empty() {
            return;
        }

        let current_idx = self
            .focused
            .and_then(|f| focusable.iter().position(|&id| id == f));

        let next_idx = match current_idx {
            Some(idx) => {
                if reverse {
                    if idx == 0 { focusable.len() - 1 } else { idx - 1 }
                } else {
                    (idx + 1) % focusable.len()
                }
            }
            None => 0, // No current focus → focus first focusable
        };

        self.focused = Some(focusable[next_idx]);
    }

    /// Remove a node and all its descendants from the tree.
    ///
    /// # Panics
    /// Panics if trying to remove the root node.
    pub fn remove(&mut self, id: NodeId) {
        assert!(id != self.root, "cannot remove root node");

        // Remove from parent's children list
        if let Some(parent) = self.nodes[id.0].parent {
            self.nodes[parent.0].children.retain(|&child| child != id);
        }

        self.tombstone_subtree(id);
    }

    /// Replace all children of `parent` with nodes built from `elements`.
    ///
    /// Existing children are removed. New nodes are created from the
    /// element descriptions. This is the core of the declarative layer:
    /// view functions return `Elements`, and `rebuild` materializes them.
    ///
    /// ```ignore
    /// fn my_view(state: &AppState) -> Elements {
    ///     let mut els = Elements::new();
    ///     els.add(TextBlockEl::new().unstyled("Hello"));
    ///     els
    /// }
    ///
    /// renderer.rebuild(container, my_view(&state));
    /// ```
    pub fn rebuild(&mut self, parent: NodeId, elements: Elements) {
        self.reconcile_children(parent, elements.into_items());
    }

    /// Find a direct child of `parent` by its key.
    ///
    /// Returns `None` if no child has the given key.
    pub fn find_by_key(&self, parent: NodeId, key: &str) -> Option<NodeId> {
        self.nodes[parent.0]
            .children
            .iter()
            .find(|&&child_id| self.nodes[child_id.0].key.as_deref() == Some(key))
            .copied()
    }

    // --- Effect registration ---

    /// Register a periodic tick handler for a node.
    ///
    /// The handler receives `&mut C::State` and is called by [`tick()`]
    /// when the interval has elapsed. The `Tracked` wrapper marks state
    /// dirty automatically when the handler fires.
    ///
    /// Only one tick per node. Re-registering replaces the previous one.
    pub fn register_tick<C: Component>(
        &mut self,
        id: NodeId,
        interval: Duration,
        handler: impl Fn(&mut C::State) + Send + Sync + 'static,
    ) {
        let effects = self.effects.entry(id).or_default();
        // Remove any existing Interval (one tick per node)
        effects.retain(|e| !matches!(e.kind, EffectKind::Interval { .. }));
        effects.push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::Interval {
                interval,
                last_tick: Instant::now(),
            },
        });
    }

    /// Remove a tick registration for a node. No-op if none exists.
    pub fn unregister_tick(&mut self, id: NodeId) {
        if let Some(effects) = self.effects.get_mut(&id) {
            effects.retain(|e| !matches!(e.kind, EffectKind::Interval { .. }));
            if effects.is_empty() {
                self.effects.remove(&id);
            }
        }
    }

    /// Register a mount handler for a node.
    ///
    /// The handler fires once after the element's `build()` completes
    /// and the node is placed in the tree. It is then removed (one-shot).
    pub fn on_mount<C: Component>(
        &mut self,
        id: NodeId,
        handler: impl Fn(&mut C::State) + Send + Sync + 'static,
    ) {
        self.effects.entry(id).or_default().push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::OnMount,
        });
    }

    /// Register an unmount handler for a node.
    ///
    /// The handler fires once when the node is tombstoned (removed from
    /// the tree), before children are recursively cleaned up.
    pub fn on_unmount<C: Component>(
        &mut self,
        id: NodeId,
        handler: impl Fn(&mut C::State) + Send + Sync + 'static,
    ) {
        let effects = self.effects.entry(id).or_default();
        // Replace any existing OnUnmount
        effects.retain(|e| !matches!(e.kind, EffectKind::OnUnmount));
        effects.push(Effect {
            handler: Box::new(TypedEffectHandler {
                handler: Box::new(handler),
            }),
            kind: EffectKind::OnUnmount,
        });
    }

    /// Advance all registered tick handlers based on elapsed wall clock time.
    ///
    /// Returns `true` if any handler fired (state was mutated, re-render
    /// likely needed).
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();

        // Collect (node_id, effect_index) pairs for due interval effects
        let mut due: Vec<(NodeId, usize)> = Vec::new();
        for (&id, effects) in &self.effects {
            for (idx, effect) in effects.iter().enumerate() {
                if let EffectKind::Interval { last_tick, interval } = &effect.kind {
                    if now.duration_since(*last_tick) >= *interval {
                        due.push((id, idx));
                    }
                }
            }
        }

        if due.is_empty() {
            return false;
        }

        for (id, idx) in due {
            // Remove the effect, call handler, reinsert with updated last_tick
            let mut effects = self.effects.remove(&id).unwrap();
            let effect = &mut effects[idx];
            if let EffectKind::Interval { ref mut last_tick, .. } = effect.kind {
                *last_tick = now;
            }
            effects[idx].handler.call(self.nodes[id.0].state.as_any_mut());
            self.effects.insert(id, effects);
        }

        true
    }

    /// Whether there are any active tick registrations.
    pub fn has_active(&self) -> bool {
        self.effects.values().any(|effects| {
            effects.iter().any(|e| matches!(e.kind, EffectKind::Interval { .. }))
        })
    }

    /// Fire and remove all OnMount effects for a node.
    fn fire_mount(&mut self, id: NodeId) {
        if let Some(effects) = self.effects.remove(&id) {
            // Extract mount handlers, keep the rest
            let (mounts, remaining): (Vec<_>, Vec<_>) = effects
                .into_iter()
                .partition(|e| matches!(e.kind, EffectKind::OnMount));

            // Fire mount handlers
            for effect in mounts {
                effect.handler.call(self.nodes[id.0].state.as_any_mut());
            }

            // Reinsert remaining effects (if any)
            if !remaining.is_empty() {
                self.effects.insert(id, remaining);
            }
        }
    }

    /// Fire and remove all OnUnmount effects for a node.
    fn fire_unmount(&mut self, id: NodeId) {
        if let Some(effects) = self.effects.remove(&id) {
            let (unmounts, remaining): (Vec<_>, Vec<_>) = effects
                .into_iter()
                .partition(|e| matches!(e.kind, EffectKind::OnUnmount));

            for effect in unmounts {
                effect.handler.call(self.nodes[id.0].state.as_any_mut());
            }

            if !remaining.is_empty() {
                self.effects.insert(id, remaining);
            }
        }
    }

    /// Reconcile old children of `parent` against new element entries.
    ///
    /// Reuses existing nodes where possible (matching by key or by
    /// position+type), preserving their local component state. Calls
    /// `Element::update` on reused nodes and `Element::build` on new ones.
    fn reconcile_children(&mut self, parent: NodeId, new_entries: Vec<ElementEntry>) {
        let old_children: Vec<NodeId> = std::mem::take(&mut self.nodes[parent.0].children);

        // Separate old children into keyed and unkeyed
        let mut old_by_key: HashMap<String, NodeId> = HashMap::new();
        let mut old_unkeyed: VecDeque<NodeId> = VecDeque::new();

        for &child_id in &old_children {
            if let Some(ref key) = self.nodes[child_id.0].key {
                old_by_key.insert(key.clone(), child_id);
            } else {
                old_unkeyed.push_back(child_id);
            }
        }

        let mut new_children: Vec<NodeId> = Vec::with_capacity(new_entries.len());

        for entry in new_entries {
            let matched = if let Some(ref key) = entry.key {
                // Keyed: match by key + type
                match old_by_key.remove(key) {
                    Some(old_id)
                        if self.nodes[old_id.0].element_type_id == Some(entry.type_id) =>
                    {
                        Some(old_id)
                    }
                    Some(old_id) => {
                        // Key exists but wrong type — tombstone old
                        self.tombstone_subtree(old_id);
                        None
                    }
                    None => None,
                }
            } else {
                // Unkeyed matching is strictly positional. If types don't match
                // at this position, the old node is discarded even if a matching
                // type exists later in the list. Use keys for stable identity
                // across reorders.
                if let Some(front_id) = old_unkeyed.pop_front() {
                    if self.nodes[front_id.0].element_type_id == Some(entry.type_id) {
                        Some(front_id)
                    } else {
                        self.tombstone_subtree(front_id);
                        None
                    }
                } else {
                    None
                }
            };

            let node_id = if let Some(old_id) = matched {
                // REUSE: update props, preserve local state
                entry.element.update(self, old_id);
                self.nodes[old_id.0].parent = Some(parent);
                self.nodes[old_id.0].width_constraint = entry.width_constraint;
                // Guarantee re-render after props update
                self.nodes[old_id.0].force_dirty = true;
                self.apply_lifecycle(old_id);

                // Resolve children: component decides (slot = external children)
                let resolved = self.resolve_children(old_id, entry.children);
                if let Some(els) = resolved {
                    self.reconcile_children(old_id, els.into_items());
                }

                old_id
            } else {
                // BUILD: create new node
                let id = entry.element.build(self, parent);
                self.nodes[id.0].element_type_id = Some(entry.type_id);
                self.nodes[id.0].key = entry.key;
                self.nodes[id.0].width_constraint = entry.width_constraint;
                self.apply_lifecycle(id);
                self.fire_mount(id);

                // Resolve children: component decides (slot = external children)
                let resolved = self.resolve_children(id, entry.children);
                if let Some(els) = resolved {
                    self.build_elements(id, els);
                }

                id
            };

            new_children.push(node_id);
        }

        // Tombstone remaining unmatched old children
        for old_id in old_unkeyed {
            self.tombstone_subtree(old_id);
        }
        for (_, old_id) in old_by_key {
            self.tombstone_subtree(old_id);
        }

        self.nodes[parent.0].children = new_children;
    }

    /// Build elements into the tree as children of `parent`.
    ///
    /// Sets element_type_id and key on newly created nodes so they
    /// can participate in future reconciliation.
    fn build_elements(&mut self, parent: NodeId, elements: Elements) {
        for entry in elements.into_items() {
            let node_id = entry.element.build(self, parent);
            self.nodes[node_id.0].element_type_id = Some(entry.type_id);
            self.nodes[node_id.0].key = entry.key;
            self.nodes[node_id.0].width_constraint = entry.width_constraint;
            self.apply_lifecycle(node_id);
            self.fire_mount(node_id);

            let resolved = self.resolve_children(node_id, entry.children);
            if let Some(els) = resolved {
                self.build_elements(node_id, els);
            }
        }
    }

    /// Resolve children for a node: pass external slot through the
    /// component's `children()` method. Returns the final child elements.
    fn resolve_children(&self, id: NodeId, slot: Option<Elements>) -> Option<Elements> {
        let node = &self.nodes[id.0];
        let state = node.state.inner_as_any();
        node.component.children_erased(state, slot)
    }

    /// Run the component's lifecycle method and apply resulting effects.
    ///
    /// If the component called `manage_effects()`, existing effects
    /// are replaced with the new set. Otherwise effects are untouched
    /// (backward compatibility with imperative registration).
    fn apply_lifecycle(&mut self, id: NodeId) {
        let effects = {
            let node = &self.nodes[id.0];
            node.component.lifecycle_erased(node.state.inner_as_any())
        };
        if effects.is_empty() {
            self.effects.remove(&id);
        } else {
            self.effects.insert(id, effects);
        }
    }

    /// Tombstone a node and all its descendants without touching the
    /// parent's children list (caller is responsible for that).
    fn tombstone_subtree(&mut self, id: NodeId) {
        // Children unmount first (bottom-up), then parent
        let children = std::mem::take(&mut self.nodes[id.0].children);
        for child_id in children {
            self.tombstone_subtree(child_id);
        }
        self.fire_unmount(id);
        self.effects.remove(&id);
        let node = &mut self.nodes[id.0];
        node.parent = None;
        node.frozen = true;
        node.last_height = Some(0);
        node.cached_buffer = None;
    }

    /// Set the rendering width (e.g., on terminal resize).
    /// Invalidates all cached buffers and marks all non-frozen nodes
    /// dirty so they re-render at the new width.
    pub fn set_width(&mut self, width: u16) {
        if self.width != width {
            self.width = width;
            for node in &mut self.nodes {
                node.cached_buffer = None;
                node.last_height = None;
                // Force dirty so non-frozen nodes re-render even if
                // state wasn't mutated via DerefMut
                if !node.frozen {
                    // We can't call DerefMut on the type-erased state,
                    // so we need a force_dirty method
                    node.force_dirty = true;
                }
            }
        }
    }

    /// Current rendering width.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Render the component tree into a Frame.
    ///
    /// Recursively measures and renders from the root.
    pub fn render(&mut self) -> Frame {
        let total_height = self.measure_height(self.root, self.width);

        if total_height == 0 || self.width == 0 {
            self.cursor_hint = None;
            return Frame::new(Buffer::empty(Rect::new(0, 0, self.width, 0)));
        }

        let area = Rect::new(0, 0, self.width, total_height);
        let mut buffer = Buffer::empty(area);

        self.render_node(self.root, area, &mut buffer);

        // Compute cursor hint from the focused component
        self.cursor_hint = None;
        if let Some(focused) = self.focused {
            let node = &self.nodes[focused.0];
            if let Some(layout_rect) = node.layout_rect {
                let state = node.state.inner_as_any();
                if let Some((rel_col, rel_row)) =
                    node.component.cursor_position_erased(layout_rect, state)
                {
                    // Convert to absolute buffer coordinates
                    self.cursor_hint = Some((
                        layout_rect.x + rel_col,
                        layout_rect.y + rel_row,
                    ));
                }
            }
        }

        Frame::new(buffer)
    }

    /// After rendering, the absolute cursor position hint from the
    /// focused component. `None` means hide the cursor.
    pub fn cursor_hint(&self) -> Option<(u16, u16)> {
        self.cursor_hint
    }

    /// Recursively measure the height of a node and its children.
    fn measure_height(&self, id: NodeId, width: u16) -> u16 {
        let node = &self.nodes[id.0];

        if node.frozen {
            return node.last_height.unwrap_or(0);
        }

        if node.is_container() {
            let insets = node.component.content_inset_erased(node.state.inner_as_any());
            let inner_width = width.saturating_sub(insets.horizontal());

            let children_height = match node.layout {
                Layout::Vertical => {
                    node.children
                        .iter()
                        .map(|&child| self.measure_height(child, inner_width))
                        .sum()
                }
                Layout::Horizontal => {
                    let constraints: Vec<WidthConstraint> = node
                        .children
                        .iter()
                        .map(|&cid| self.nodes[cid.0].width_constraint)
                        .collect();
                    let widths = allocate_widths(&constraints, inner_width);
                    node.children
                        .iter()
                        .zip(widths.iter())
                        .map(|(&child, &w)| self.measure_height(child, w))
                        .max()
                        .unwrap_or(0)
                }
            };

            children_height + insets.vertical()
        } else {
            // Leaf: ask the component
            let state = node.state.inner_as_any();
            node.component.desired_height_erased(width, state)
        }
    }

    /// Recursively render a node and its children into the buffer.
    fn render_node(&mut self, id: NodeId, area: Rect, buffer: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Store layout rect for cursor positioning
        self.nodes[id.0].layout_rect = Some(area);

        let node = &self.nodes[id.0];
        let is_container = node.is_container();

        // Frozen or clean leaf: use cached buffer
        let needs_render = node.force_dirty || node.state.is_dirty();
        if node.frozen || (!is_container && !needs_render) {
            if let Some(ref cached) = node.cached_buffer {
                copy_buffer(cached, buffer, area);
            }
            return;
        }

        if is_container {
            // Render the container's own component first (background/border/chrome)
            let state = self.nodes[id.0].state.inner_as_any();
            self.nodes[id.0].component.render_erased(area, buffer, state);

            // Compute inner area for children using content insets
            let insets = self.nodes[id.0]
                .component
                .content_inset_erased(self.nodes[id.0].state.inner_as_any());
            let inner = Rect::new(
                area.x.saturating_add(insets.left),
                area.y.saturating_add(insets.top),
                area.width.saturating_sub(insets.horizontal()),
                area.height.saturating_sub(insets.vertical()),
            );

            let children: Vec<NodeId> = self.nodes[id.0].children.clone();
            let layout = self.nodes[id.0].layout;

            match layout {
                Layout::Vertical => {
                    let mut y_offset = inner.y;
                    for child_id in &children {
                        let child_height = self.measure_height(*child_id, inner.width);
                        if child_height == 0 {
                            continue;
                        }
                        let child_area =
                            Rect::new(inner.x, y_offset, inner.width, child_height);
                        self.render_node(*child_id, child_area, buffer);
                        y_offset = y_offset.saturating_add(child_height);
                    }
                }
                Layout::Horizontal => {
                    let constraints: Vec<WidthConstraint> = children
                        .iter()
                        .map(|&cid| self.nodes[cid.0].width_constraint)
                        .collect();
                    let widths = allocate_widths(&constraints, inner.width);
                    let mut x_offset = inner.x;
                    for (child_id, &child_width) in children.iter().zip(widths.iter()) {
                        if child_width == 0 {
                            continue;
                        }
                        let child_area =
                            Rect::new(x_offset, inner.y, child_width, inner.height);
                        self.render_node(*child_id, child_area, buffer);
                        x_offset = x_offset.saturating_add(child_width);
                    }
                }
            }

            // Cache and clean
            let mut node_buf = Buffer::empty(area);
            copy_buffer_region(buffer, &mut node_buf, area);
            self.nodes[id.0].cached_buffer = Some(node_buf);
            self.nodes[id.0].last_height = Some(area.height);
            self.nodes[id.0].state.clear_dirty();
            self.nodes[id.0].force_dirty = false;
        } else {
            // Leaf: render the component
            let state = self.nodes[id.0].state.inner_as_any();
            self.nodes[id.0].component.render_erased(area, buffer, state);

            // Cache and clean
            let mut node_buf = Buffer::empty(area);
            copy_buffer_region(buffer, &mut node_buf, area);
            self.nodes[id.0].cached_buffer = Some(node_buf);
            self.nodes[id.0].last_height = Some(area.height);
            self.nodes[id.0].state.clear_dirty();
            self.nodes[id.0].force_dirty = false;
        }
    }
}

/// Allocate widths among children based on their constraints.
///
/// Fixed children get their requested width (clamped to available).
/// Fill children split the remaining space equally, with remainder
/// distributed to the first Fill children.
fn allocate_widths(constraints: &[WidthConstraint], total: u16) -> Vec<u16> {
    let fixed_sum: u16 = constraints
        .iter()
        .filter_map(|c| match c {
            WidthConstraint::Fixed(w) => Some(*w),
            _ => None,
        })
        .sum();
    let fill_count = constraints
        .iter()
        .filter(|c| matches!(c, WidthConstraint::Fill))
        .count() as u16;

    let remaining = total.saturating_sub(fixed_sum);
    let per_fill = if fill_count > 0 { remaining / fill_count } else { 0 };
    let mut remainder = if fill_count > 0 { remaining % fill_count } else { 0 };

    constraints
        .iter()
        .map(|c| match c {
            WidthConstraint::Fixed(w) => (*w).min(total),
            WidthConstraint::Fill => {
                let extra = if remainder > 0 {
                    remainder -= 1;
                    1
                } else {
                    0
                };
                per_fill + extra
            }
        })
        .collect()
}

/// Copy cells from a source buffer into a destination buffer at the given area.
fn copy_buffer(src: &Buffer, dst: &mut Buffer, area: Rect) {
    let src_area = src.area;
    for y in 0..area.height {
        for x in 0..area.width {
            let src_x = src_area.x + x;
            let src_y = src_area.y + y;
            let dst_x = area.x + x;
            let dst_y = area.y + y;

            if src_x < src_area.x + src_area.width
                && src_y < src_area.y + src_area.height
                && dst_x < dst.area.x + dst.area.width
                && dst_y < dst.area.y + dst.area.height
            {
                dst[(dst_x, dst_y)] = src[(src_x, src_y)].clone();
            }
        }
    }
}

/// Copy a region from one buffer to another buffer.
fn copy_buffer_region(src: &Buffer, dst: &mut Buffer, region: Rect) {
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if x < src.area.x + src.area.width
                && y < src.area.y + src.area.height
                && x < dst.area.x + dst.area.width
                && y < dst.area.y + dst.area.height
            {
                dst[(x, y)] = src[(x, y)].clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use ratatui_core::text::Line;
    use ratatui_widgets::paragraph::Paragraph;

    struct TextBlock;

    impl Component for TextBlock {
        type State = Vec<String>;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let text: Vec<Line> = state.iter().map(|s| Line::raw(s.as_str())).collect();
            let para = Paragraph::new(text);
            ratatui_core::widgets::Widget::render(para, area, buf);
        }

        fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
            state.len() as u16
        }

        fn initial_state(&self) -> Vec<String> {
            vec![]
        }
    }

    // --- Existing tests (flat API, should still pass) ---

    #[test]
    fn render_empty_renderer() {
        let mut r = Renderer::new(80);
        let frame = r.render();
        assert_eq!(frame.area().height, 0);
    }

    #[test]
    fn render_single_component() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 1);
        assert_eq!(frame.area().width, 10);

        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), "h");
    }

    #[test]
    fn render_two_components_stacked() {
        let mut r = Renderer::new(10);
        let id1 = r.push(TextBlock);
        let id2 = r.push(TextBlock);

        r.state_mut::<TextBlock>(id1).push("top".to_string());
        r.state_mut::<TextBlock>(id2).push("bot".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 2);

        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), "t");
        assert_eq!(buf[(0, 1)].symbol(), "b");
    }

    #[test]
    fn dirty_flag_cleared_after_render() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        assert!(r.nodes[id.0].state.is_dirty());
        let _ = r.render();
        assert!(!r.nodes[id.0].state.is_dirty());
    }

    #[test]
    fn frozen_component_uses_cached_buffer() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        let _frame1 = r.render();
        r.freeze(id);

        let frame2 = r.render();
        assert_eq!(frame2.area().height, 1);
        assert_eq!(frame2.buffer()[(0, 0)].symbol(), "h");
    }

    #[test]
    fn component_height_changes_with_state() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);

        let frame1 = r.render();
        assert_eq!(frame1.area().height, 0);

        r.state_mut::<TextBlock>(id).push("line1".to_string());
        let frame2 = r.render();
        assert_eq!(frame2.area().height, 1);

        r.state_mut::<TextBlock>(id).push("line2".to_string());
        let frame3 = r.render();
        assert_eq!(frame3.area().height, 2);
    }

    // --- New tree tests ---

    #[test]
    fn root_exists() {
        let r = Renderer::new(80);
        let root = r.root();
        assert_eq!(root, NodeId(0));
        assert!(r.children(root).is_empty());
    }

    #[test]
    fn append_child_creates_tree() {
        let mut r = Renderer::new(10);
        let root = r.root();
        let child = r.append_child(root, TextBlock);

        assert_eq!(r.children(root), &[child]);
    }

    #[test]
    fn nested_containers() {
        let mut r = Renderer::new(10);

        // Root -> container -> two text blocks
        let container = r.push(VStack);
        let child1 = r.append_child(container, TextBlock);
        let child2 = r.append_child(container, TextBlock);

        r.state_mut::<TextBlock>(child1).push("first".to_string());
        r.state_mut::<TextBlock>(child2).push("second".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 2);

        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), "f"); // "first"
        assert_eq!(buf[(0, 1)].symbol(), "s"); // "second"
    }

    #[test]
    fn deeply_nested_tree() {
        let mut r = Renderer::new(10);

        // Root -> outer -> inner -> text
        let outer = r.push(VStack);
        let inner = r.append_child(outer, VStack);
        let text = r.append_child(inner, TextBlock);

        r.state_mut::<TextBlock>(text).push("deep".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 1);
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "d");
    }

    #[test]
    fn mixed_flat_and_nested() {
        let mut r = Renderer::new(10);

        // Root has: a flat text block + a container with two children
        let flat = r.push(TextBlock);
        r.state_mut::<TextBlock>(flat).push("flat".to_string());

        let container = r.push(VStack);
        let nested1 = r.append_child(container, TextBlock);
        let nested2 = r.append_child(container, TextBlock);
        r.state_mut::<TextBlock>(nested1).push("nest1".to_string());
        r.state_mut::<TextBlock>(nested2).push("nest2".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 3);

        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), "f"); // "flat"
        assert_eq!(buf[(0, 1)].symbol(), "n"); // "nest1"
        assert_eq!(buf[(0, 2)].symbol(), "n"); // "nest2"
    }

    #[test]
    fn remove_node() {
        let mut r = Renderer::new(10);
        let id1 = r.push(TextBlock);
        let id2 = r.push(TextBlock);

        r.state_mut::<TextBlock>(id1).push("keep".to_string());
        r.state_mut::<TextBlock>(id2).push("remove".to_string());

        // Render with both
        let frame1 = r.render();
        assert_eq!(frame1.area().height, 2);

        // Remove second
        r.remove(id2);

        let frame2 = r.render();
        assert_eq!(frame2.area().height, 1);
        assert_eq!(frame2.buffer()[(0, 0)].symbol(), "k"); // "keep"
    }

    #[test]
    fn remove_container_removes_children() {
        let mut r = Renderer::new(10);

        let container = r.push(VStack);
        let child = r.append_child(container, TextBlock);
        r.state_mut::<TextBlock>(child).push("gone".to_string());

        let frame1 = r.render();
        assert_eq!(frame1.area().height, 1);

        r.remove(container);

        let frame2 = r.render();
        assert_eq!(frame2.area().height, 0);
    }

    // --- Event handling tests ---

    /// A component that appends characters from key events to its state.
    struct InputCapture;

    impl Component for InputCapture {
        type State = String;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let line = ratatui_core::text::Line::raw(state.as_str());
            ratatui_core::widgets::Widget::render(Paragraph::new(line), area, buf);
        }

        fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
            if state.is_empty() { 0 } else { 1 }
        }

        fn handle_event(
            &self,
            event: &crossterm::event::Event,
            state: &mut Self::State,
        ) -> EventResult {
            use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
            if let Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                kind: KeyEventKind::Press,
                ..
            }) = event
            {
                state.push(*c);
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }

        fn initial_state(&self) -> String {
            String::new()
        }
    }

    fn key_event(c: char) -> crossterm::event::Event {
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::empty(),
        ))
    }

    #[test]
    fn event_delivered_to_focused_component() {
        let mut r = Renderer::new(10);
        let id = r.push(InputCapture);
        r.set_focus(id);

        let result = r.handle_event(&key_event('a'));
        assert_eq!(result, EventResult::Consumed);

        // State should have been mutated
        let state = r.state_mut::<InputCapture>(id);
        assert_eq!(&**state, "a");
    }

    #[test]
    fn no_focus_returns_ignored() {
        let mut r = Renderer::new(10);
        let _id = r.push(InputCapture);
        // No focus set

        let result = r.handle_event(&key_event('a'));
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn event_bubbles_to_parent() {
        let mut r = Renderer::new(10);

        // Parent handles events, child (TextBlock) does not
        let parent = r.push(InputCapture);
        let child = r.append_child(parent, TextBlock);
        r.state_mut::<TextBlock>(child).push("child".to_string());

        // Focus the child
        r.set_focus(child);

        // Child ignores the event → bubbles to parent
        let result = r.handle_event(&key_event('x'));
        assert_eq!(result, EventResult::Consumed);

        // Parent state should have the character
        let state = r.state_mut::<InputCapture>(parent);
        assert_eq!(&**state, "x");
    }

    #[test]
    fn frozen_component_skipped_in_bubble() {
        let mut r = Renderer::new(10);

        let parent = r.push(InputCapture);
        let child = r.append_child(parent, TextBlock);
        r.state_mut::<TextBlock>(child).push("child".to_string());

        // Freeze the parent
        let _ = r.render(); // populate cache
        r.freeze(parent);

        // Focus the child
        r.set_focus(child);

        // Event bubbles to parent, but parent is frozen → skipped
        let result = r.handle_event(&key_event('x'));
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn event_marks_state_dirty() {
        let mut r = Renderer::new(10);
        let id = r.push(InputCapture);
        r.set_focus(id);

        // Give it content so it renders (height > 0)
        r.state_mut::<InputCapture>(id).push('x');

        // Render to clear dirty flag
        let _ = r.render();
        assert!(!r.nodes[id.0].state.is_dirty());

        // Deliver event
        r.handle_event(&key_event('a'));

        // State should be dirty now (DerefMut in handle_event_erased)
        assert!(r.nodes[id.0].state.is_dirty());
    }

    #[test]
    fn focus_can_be_changed() {
        let mut r = Renderer::new(10);
        let id1 = r.push(InputCapture);
        let id2 = r.push(InputCapture);

        r.set_focus(id1);
        r.handle_event(&key_event('a'));
        assert_eq!(&**r.state_mut::<InputCapture>(id1), "a");
        assert_eq!(&**r.state_mut::<InputCapture>(id2), "");

        r.set_focus(id2);
        r.handle_event(&key_event('b'));
        assert_eq!(&**r.state_mut::<InputCapture>(id1), "a");
        assert_eq!(&**r.state_mut::<InputCapture>(id2), "b");
    }

    // --- Focus cycling tests ---

    /// A focusable component for tab cycling tests.
    struct FocusableItem;

    impl Component for FocusableItem {
        type State = String;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let line = ratatui_core::text::Line::raw(state.as_str());
            ratatui_core::widgets::Widget::render(Paragraph::new(line), area, buf);
        }

        fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
            if state.is_empty() { 0 } else { 1 }
        }

        fn is_focusable(&self, _state: &Self::State) -> bool {
            true
        }

        fn initial_state(&self) -> String {
            "item".to_string()
        }
    }

    fn tab_event() -> crossterm::event::Event {
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::empty(),
        ))
    }

    fn backtab_event() -> crossterm::event::Event {
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::BackTab,
            crossterm::event::KeyModifiers::SHIFT,
        ))
    }

    #[test]
    fn tab_cycles_through_focusable_nodes() {
        let mut r = Renderer::new(10);
        let _non_focusable = r.push(TextBlock); // not focusable
        let f1 = r.push(FocusableItem);
        let f2 = r.push(FocusableItem);
        let f3 = r.push(FocusableItem);

        r.state_mut::<TextBlock>(_non_focusable).push("header".to_string());

        // No initial focus → Tab focuses first focusable
        r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f1));

        // Tab again → second
        r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f2));

        // Tab again → third
        r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f3));

        // Tab wraps → back to first
        r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f1));
    }

    #[test]
    fn backtab_cycles_reverse() {
        let mut r = Renderer::new(10);
        let f1 = r.push(FocusableItem);
        let f2 = r.push(FocusableItem);
        let f3 = r.push(FocusableItem);

        r.set_focus(f1);

        // BackTab from first → wraps to last
        r.handle_event(&backtab_event());
        assert_eq!(r.focus(), Some(f3));

        // BackTab → second
        r.handle_event(&backtab_event());
        assert_eq!(r.focus(), Some(f2));
    }

    #[test]
    fn tab_skips_frozen_nodes() {
        let mut r = Renderer::new(10);
        let f1 = r.push(FocusableItem);
        let f2 = r.push(FocusableItem);
        let f3 = r.push(FocusableItem);

        let _ = r.render(); // populate caches
        r.freeze(f2); // freeze the middle one

        r.set_focus(f1);
        r.handle_event(&tab_event());
        // Should skip f2 (frozen) → go to f3
        assert_eq!(r.focus(), Some(f3));
    }

    #[test]
    fn tab_with_no_focusable_nodes_does_nothing() {
        let mut r = Renderer::new(10);
        let _id = r.push(TextBlock); // not focusable
        r.state_mut::<TextBlock>(_id).push("text".to_string());

        r.handle_event(&tab_event());
        assert_eq!(r.focus(), None);
    }

    // --- Declarative rebuild tests ---

    use crate::element::{Element, Elements};

    /// A simple element for testing that creates a TextBlock with given lines.
    struct TestTextEl {
        lines: Vec<String>,
    }

    impl TestTextEl {
        fn new(text: &str) -> Self {
            Self { lines: vec![text.to_string()] }
        }
    }

    impl Element for TestTextEl {
        fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
            let id = renderer.append_child(parent, TextBlock);
            for line in self.lines {
                renderer.state_mut::<TextBlock>(id).push(line);
            }
            id
        }

        fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
            let state = renderer.state_mut::<TextBlock>(node_id);
            state.clear();
            for line in self.lines {
                state.push(line);
            }
        }
    }

    #[test]
    fn rebuild_replaces_children() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Initially add two children imperatively
        let c1 = r.append_child(container, TextBlock);
        r.state_mut::<TextBlock>(c1).push("old1".to_string());
        let c2 = r.append_child(container, TextBlock);
        r.state_mut::<TextBlock>(c2).push("old2".to_string());

        let frame1 = r.render();
        assert_eq!(frame1.area().height, 2);

        // Rebuild with a single new child
        let mut els = Elements::new();
        els.add(TestTextEl::new("new1"));
        r.rebuild(container, els);

        let frame2 = r.render();
        assert_eq!(frame2.area().height, 1);
        assert_eq!(frame2.buffer()[(0, 0)].symbol(), "n"); // "new1"
    }

    #[test]
    fn rebuild_with_nested_children() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build a nested structure: VStack with two text blocks
        let mut inner = Elements::new();
        inner.add(TestTextEl::new("child1"));
        inner.add(TestTextEl::new("child2"));

        let mut els = Elements::new();
        els.add_with_children(crate::elements::VStackEl, inner);
        r.rebuild(container, els);

        let frame = r.render();
        assert_eq!(frame.area().height, 2);
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "c"); // "child1"
        assert_eq!(frame.buffer()[(0, 1)].symbol(), "c"); // "child2"
    }

    #[test]
    fn rebuild_with_empty_elements_clears_children() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let c = r.append_child(container, TextBlock);
        r.state_mut::<TextBlock>(c).push("exists".to_string());

        let frame1 = r.render();
        assert_eq!(frame1.area().height, 1);

        // Rebuild with empty elements
        r.rebuild(container, Elements::new());

        let frame2 = r.render();
        assert_eq!(frame2.area().height, 0);
    }

    #[test]
    fn rebuild_view_function_pattern() {
        // Simulate a view function that produces different trees based on state
        fn view(thinking: bool) -> Elements {
            let mut els = Elements::new();
            if thinking {
                els.add(TestTextEl::new("thinking..."));
            }
            els.add(TestTextEl::new("message"));
            els
        }

        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Thinking state
        r.rebuild(container, view(true));
        let frame1 = r.render();
        assert_eq!(frame1.area().height, 2);
        assert_eq!(frame1.buffer()[(0, 0)].symbol(), "t"); // "thinking..."

        // Not thinking
        r.rebuild(container, view(false));
        let frame2 = r.render();
        assert_eq!(frame2.area().height, 1);
        assert_eq!(frame2.buffer()[(0, 0)].symbol(), "m"); // "message"
    }

    #[test]
    fn rebuild_with_group() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut children = Elements::new();
        children.add(TestTextEl::new("grouped1"));
        children.add(TestTextEl::new("grouped2"));

        let mut els = Elements::new();
        els.add(TestTextEl::new("before"));
        els.group(children);
        els.add(TestTextEl::new("after"));
        r.rebuild(container, els);

        let frame = r.render();
        assert_eq!(frame.area().height, 4);
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "b"); // "before"
        assert_eq!(frame.buffer()[(0, 1)].symbol(), "g"); // "grouped1"
        assert_eq!(frame.buffer()[(0, 2)].symbol(), "g"); // "grouped2"
        assert_eq!(frame.buffer()[(0, 3)].symbol(), "a"); // "after"
    }

    #[test]
    fn custom_element_impl_works() {
        struct CustomWidget;

        impl Component for CustomWidget {
            type State = String;
            fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
                let line = ratatui_core::text::Line::raw(state.as_str());
                ratatui_core::widgets::Widget::render(Paragraph::new(line), area, buf);
            }
            fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
                if state.is_empty() { 0 } else { 1 }
            }
            fn initial_state(&self) -> String {
                String::new()
            }
        }

        struct CustomWidgetEl { config: String }

        impl Element for CustomWidgetEl {
            fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
                let id = renderer.append_child(parent, CustomWidget);
                let state = renderer.state_mut::<CustomWidget>(id);
                state.push_str(&self.config);
                id
            }
        }

        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(CustomWidgetEl { config: "custom!".to_string() });
        r.rebuild(container, els);

        let frame = r.render();
        assert_eq!(frame.area().height, 1);
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "c"); // "custom!"
    }

    // --- Reconciliation tests ---

    /// A counter element whose state tracks how many times it was built.
    /// Used to verify reconciliation reuses nodes vs creating new ones.
    struct CounterWidget;

    impl Component for CounterWidget {
        type State = (String, usize); // (label, build_count)

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let line = ratatui_core::text::Line::raw(state.0.as_str());
            ratatui_core::widgets::Widget::render(Paragraph::new(line), area, buf);
        }

        fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
            if state.0.is_empty() { 0 } else { 1 }
        }

        fn initial_state(&self) -> (String, usize) {
            (String::new(), 0)
        }
    }

    struct CounterEl {
        label: String,
    }

    impl CounterEl {
        fn new(label: &str) -> Self {
            Self { label: label.to_string() }
        }
    }

    impl Element for CounterEl {
        fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
            let id = renderer.append_child(parent, CounterWidget);
            let state = renderer.state_mut::<CounterWidget>(id);
            state.0 = self.label;
            state.1 = 1; // build_count = 1
            id
        }

        fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
            let state = renderer.state_mut::<CounterWidget>(node_id);
            state.0 = self.label;
            // Don't reset build_count — it proves the node was reused
        }
    }

    #[test]
    fn reconciliation_reuses_same_type_at_same_position() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // First build
        let mut els = Elements::new();
        els.add(CounterEl::new("first"));
        r.rebuild(container, els);

        let children_1 = r.children(container).to_vec();
        assert_eq!(children_1.len(), 1);
        let first_id = children_1[0];
        assert_eq!(r.state_mut::<CounterWidget>(first_id).1, 1); // build_count = 1

        // Second rebuild with same type at same position
        let mut els = Elements::new();
        els.add(CounterEl::new("updated"));
        r.rebuild(container, els);

        let children_2 = r.children(container).to_vec();
        assert_eq!(children_2.len(), 1);
        // Same NodeId — node was reused, not recreated
        assert_eq!(children_2[0], first_id);
        // Label updated by update()
        assert_eq!(&r.state_mut::<CounterWidget>(first_id).0, "updated");
        // build_count still 1 — proves update was called, not build
        assert_eq!(r.state_mut::<CounterWidget>(first_id).1, 1);
    }

    #[test]
    fn reconciliation_type_mismatch_creates_new_node() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build with CounterEl
        let mut els = Elements::new();
        els.add(CounterEl::new("counter"));
        r.rebuild(container, els);

        let old_id = r.children(container)[0];

        // Rebuild with TestTextEl (different type)
        let mut els = Elements::new();
        els.add(TestTextEl::new("text"));
        r.rebuild(container, els);

        let new_id = r.children(container)[0];
        // Different NodeId — old node tombstoned, new created
        assert_ne!(new_id, old_id);
    }

    #[test]
    fn keyed_elements_survive_position_change() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build: [A, B]
        let mut els = Elements::new();
        els.add(CounterEl::new("A")).key("a");
        els.add(CounterEl::new("B")).key("b");
        r.rebuild(container, els);

        let children_1 = r.children(container).to_vec();
        let id_a = children_1[0];
        let id_b = children_1[1];

        // Rebuild: [B, A] — reversed order
        let mut els = Elements::new();
        els.add(CounterEl::new("B")).key("b");
        els.add(CounterEl::new("A")).key("a");
        r.rebuild(container, els);

        let children_2 = r.children(container).to_vec();
        // Nodes reused but in new order
        assert_eq!(children_2[0], id_b);
        assert_eq!(children_2[1], id_a);
        // build_count still 1 for both — reused, not recreated
        assert_eq!(r.state_mut::<CounterWidget>(id_a).1, 1);
        assert_eq!(r.state_mut::<CounterWidget>(id_b).1, 1);
    }

    #[test]
    fn keyed_element_removed_and_added() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build: [A, B, C]
        let mut els = Elements::new();
        els.add(CounterEl::new("A")).key("a");
        els.add(CounterEl::new("B")).key("b");
        els.add(CounterEl::new("C")).key("c");
        r.rebuild(container, els);

        let id_a = r.children(container)[0];
        let id_c = r.children(container)[2];

        // Rebuild: [A, C] — B removed
        let mut els = Elements::new();
        els.add(CounterEl::new("A")).key("a");
        els.add(CounterEl::new("C")).key("c");
        r.rebuild(container, els);

        let children = r.children(container).to_vec();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0], id_a); // A reused
        assert_eq!(children[1], id_c); // C reused
    }

    #[test]
    fn find_by_key_works() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(CounterEl::new("alpha")).key("first");
        els.add(CounterEl::new("beta")).key("second");
        els.add(CounterEl::new("gamma")); // no key
        r.rebuild(container, els);

        assert_eq!(
            r.find_by_key(container, "first"),
            Some(r.children(container)[0])
        );
        assert_eq!(
            r.find_by_key(container, "second"),
            Some(r.children(container)[1])
        );
        assert_eq!(r.find_by_key(container, "third"), None);
        assert_eq!(r.find_by_key(container, "gamma"), None); // not a key
    }

    #[test]
    fn reconciliation_preserves_local_state() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build with a counter
        let mut els = Elements::new();
        els.add(CounterEl::new("item")).key("c");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "c").unwrap();
        // Simulate local state mutation (increment build_count as proxy)
        r.state_mut::<CounterWidget>(id).1 = 42;

        // Rebuild — node should be reused, local state preserved
        let mut els = Elements::new();
        els.add(CounterEl::new("updated-item")).key("c");
        r.rebuild(container, els);

        let id_after = r.find_by_key(container, "c").unwrap();
        assert_eq!(id, id_after); // same node
        assert_eq!(&r.state_mut::<CounterWidget>(id).0, "updated-item"); // label updated
        assert_eq!(r.state_mut::<CounterWidget>(id).1, 42); // local state preserved
    }

    #[test]
    fn reconciliation_nested_children() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build nested: VStack > [A, B]
        let mut inner = Elements::new();
        inner.add(CounterEl::new("A")).key("a");
        inner.add(CounterEl::new("B")).key("b");
        let mut els = Elements::new();
        els.add_with_children(crate::elements::VStackEl, inner).key("group");
        r.rebuild(container, els);

        let group_id = r.find_by_key(container, "group").unwrap();
        let id_a = r.find_by_key(group_id, "a").unwrap();
        let id_b = r.find_by_key(group_id, "b").unwrap();

        // Rebuild nested: VStack > [A, C] — B removed, C added
        let mut inner = Elements::new();
        inner.add(CounterEl::new("A-updated")).key("a");
        inner.add(CounterEl::new("C")).key("c");
        let mut els = Elements::new();
        els.add_with_children(crate::elements::VStackEl, inner).key("group");
        r.rebuild(container, els);

        // Group reused
        assert_eq!(r.find_by_key(container, "group").unwrap(), group_id);
        // A reused with updated label
        let id_a_after = r.find_by_key(group_id, "a").unwrap();
        assert_eq!(id_a_after, id_a);
        assert_eq!(&r.state_mut::<CounterWidget>(id_a).0, "A-updated");
        // B gone, C new
        assert_eq!(r.find_by_key(group_id, "b"), None);
        let id_c = r.find_by_key(group_id, "c").unwrap();
        assert_ne!(id_c, id_b); // new node
    }

    #[test]
    fn empty_rebuild_clears_with_reconciliation() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(CounterEl::new("something")).key("x");
        r.rebuild(container, els);
        assert_eq!(r.children(container).len(), 1);

        // Rebuild with empty
        r.rebuild(container, Elements::new());
        assert_eq!(r.children(container).len(), 0);
    }

    // --- Tick registration tests ---

    use std::time::Duration;

    #[test]
    fn register_tick_fires_handler() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        // Register a tick that appends to the text
        r.register_tick::<TextBlock>(id, Duration::from_millis(1), |state| {
            state.push("ticked".to_string());
        });

        assert!(r.has_active());

        // Sleep to ensure interval elapses
        std::thread::sleep(Duration::from_millis(5));

        let fired = r.tick();
        assert!(fired);

        // State should have been mutated by the handler
        assert_eq!(r.state_mut::<TextBlock>(id).len(), 2);
    }

    #[test]
    fn tick_respects_interval() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        // Register with a long interval
        r.register_tick::<TextBlock>(id, Duration::from_secs(60), |state| {
            state.push("ticked".to_string());
        });

        // Immediately tick — should not fire
        let fired = r.tick();
        assert!(!fired);
        assert_eq!(r.state_mut::<TextBlock>(id).len(), 1);
    }

    #[test]
    fn unregister_tick_prevents_firing() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        r.register_tick::<TextBlock>(id, Duration::from_millis(1), |state| {
            state.push("ticked".to_string());
        });
        assert!(r.has_active());

        r.unregister_tick(id);
        assert!(!r.has_active());

        std::thread::sleep(Duration::from_millis(5));
        let fired = r.tick();
        assert!(!fired);
    }

    #[test]
    fn tombstone_cleans_up_ticks() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        r.register_tick::<TextBlock>(id, Duration::from_millis(1), |_| {});
        assert!(r.has_active());

        r.remove(id);
        assert!(!r.has_active());
    }

    #[test]
    fn rebuild_cleans_up_removed_ticks() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build with a ticked element
        let mut els = Elements::new();
        els.add(CounterEl::new("item")).key("x");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "x").unwrap();
        r.register_tick::<CounterWidget>(id, Duration::from_millis(1), |state| {
            state.1 += 1;
        });
        assert!(r.has_active());

        // Rebuild without the keyed element
        r.rebuild(container, Elements::new());
        assert!(!r.has_active());
    }

    #[test]
    fn has_active_reflects_registrations() {
        let mut r = Renderer::new(10);
        assert!(!r.has_active());

        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());
        assert!(!r.has_active());

        r.register_tick::<TextBlock>(id, Duration::from_millis(80), |_| {});
        assert!(r.has_active());

        r.unregister_tick(id);
        assert!(!r.has_active());
    }

    #[test]
    fn spinner_el_build_auto_registers_tick() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(crate::elements::SpinnerEl::new("Loading..."));
        r.rebuild(container, els);

        assert!(r.has_active());
    }

    #[test]
    fn spinner_el_done_build_no_tick() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(crate::elements::SpinnerEl::new("Done").done("Completed"));
        r.rebuild(container, els);

        assert!(!r.has_active());
    }

    #[test]
    fn spinner_el_update_to_done_unregisters() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build active spinner
        let mut els = Elements::new();
        els.add(crate::elements::SpinnerEl::new("Loading...")).key("s");
        r.rebuild(container, els);
        assert!(r.has_active());

        // Rebuild as done
        let mut els = Elements::new();
        els.add(crate::elements::SpinnerEl::new("Done").done("Completed")).key("s");
        r.rebuild(container, els);
        assert!(!r.has_active());
    }

    // --- Mount/Unmount lifecycle tests ---

    /// Element that registers mount and unmount handlers.
    struct LifecycleEl {
        label: String,
        mount_marker: String,
        unmount_marker: String,
    }

    impl LifecycleEl {
        fn new(label: &str) -> Self {
            Self {
                label: label.to_string(),
                mount_marker: format!("mounted:{}", label),
                unmount_marker: format!("unmounted:{}", label),
            }
        }
    }

    /// Component with a log of lifecycle events.
    struct LifecycleWidget;

    struct LifecycleState {
        log: Vec<String>,
        mount_marker: String,
        unmount_marker: String,
    }

    impl Component for LifecycleWidget {
        type State = LifecycleState;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let text = state.log.join(", ");
            let line = ratatui_core::text::Line::raw(text);
            ratatui_core::widgets::Widget::render(Paragraph::new(line), area, buf);
        }

        fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
            if state.log.is_empty() { 0 } else { 1 }
        }

        fn initial_state(&self) -> LifecycleState {
            LifecycleState {
                log: Vec::new(),
                mount_marker: String::new(),
                unmount_marker: String::new(),
            }
        }

        fn lifecycle(&self, hooks: &mut Hooks<LifecycleState>, state: &LifecycleState) {
            let mount_marker = state.mount_marker.clone();
            if !mount_marker.is_empty() {
                hooks.use_mount(move |s| {
                    s.log.push(mount_marker.clone());
                });
            }
            let unmount_marker = state.unmount_marker.clone();
            if !unmount_marker.is_empty() {
                hooks.use_unmount(move |s| {
                    s.log.push(unmount_marker.clone());
                });
            }
        }
    }

    impl Element for LifecycleEl {
        fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
            let id = renderer.append_child(parent, LifecycleWidget);
            let state = renderer.state_mut::<LifecycleWidget>(id);
            state.log.push(self.label.clone());
            state.mount_marker = self.mount_marker;
            state.unmount_marker = self.unmount_marker;
            id
        }

        fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
            let state = renderer.state_mut::<LifecycleWidget>(node_id);
            state.log.retain(|s| s.starts_with("mounted:") || s.starts_with("unmounted:"));
            state.log.push(self.label.clone());
            state.mount_marker = self.mount_marker;
            state.unmount_marker = self.unmount_marker;
        }
    }

    #[test]
    fn mount_fires_after_build() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(LifecycleEl::new("hello")).key("a");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "a").unwrap();
        let state = r.state_mut::<LifecycleWidget>(id);
        // Should have: label from build, then mount marker
        assert!(state.log.contains(&"hello".to_string()));
        assert!(state.log.contains(&"mounted:hello".to_string()));
    }

    #[test]
    fn mount_fires_only_once_not_on_reuse() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // First build — mount fires
        let mut els = Elements::new();
        els.add(LifecycleEl::new("v1")).key("a");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "a").unwrap();
        let mount_count = r.state_mut::<LifecycleWidget>(id)
            .log.iter()
            .filter(|s| s.starts_with("mounted:"))
            .count();
        assert_eq!(mount_count, 1);

        // Second rebuild — reuse, mount should NOT fire again
        let mut els = Elements::new();
        els.add(LifecycleEl::new("v2")).key("a");
        r.rebuild(container, els);

        let state = r.state_mut::<LifecycleWidget>(id);
        let mount_count = state.log.iter().filter(|s| s.starts_with("mounted:")).count();
        assert_eq!(mount_count, 1); // still just 1
    }

    #[test]
    fn unmount_fires_on_tombstone() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(LifecycleEl::new("bye")).key("a");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "a").unwrap();

        // Rebuild with empty — triggers tombstone
        r.rebuild(container, Elements::new());

        // The node is tombstoned, but we can still read its state
        // (tombstoned nodes stay in the arena)
        let state = r.state_mut::<LifecycleWidget>(id);
        assert!(state.log.contains(&"unmounted:bye".to_string()));
    }

    #[test]
    fn unmount_parent_fires_before_children() {
        use std::sync::{Arc, Mutex};

        let order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build parent with child
        let parent_id = r.append_child(container, LifecycleWidget);
        r.state_mut::<LifecycleWidget>(parent_id).log.push("parent".to_string());
        let child_id = r.append_child(parent_id, LifecycleWidget);
        r.state_mut::<LifecycleWidget>(child_id).log.push("child".to_string());

        let order_clone = order.clone();
        r.on_unmount::<LifecycleWidget>(parent_id, move |_state| {
            order_clone.lock().unwrap().push("parent".to_string());
        });
        let order_clone = order.clone();
        r.on_unmount::<LifecycleWidget>(child_id, move |_state| {
            order_clone.lock().unwrap().push("child".to_string());
        });

        // Remove parent (which also removes child)
        r.remove(parent_id);

        let fired = order.lock().unwrap();
        assert_eq!(&*fired, &["child", "parent"]);
    }

    #[test]
    fn tombstone_cleans_up_all_effects() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        // Register tick + unmount
        r.register_tick::<TextBlock>(id, Duration::from_millis(1), |_| {});
        r.on_unmount::<TextBlock>(id, |_| {});
        assert!(r.has_active());

        r.remove(id);
        assert!(!r.has_active());
        // All effects for this node are gone
    }

    #[test]
    fn multiple_effects_per_node() {
        // Use the HookedCounter component which has interval via lifecycle
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(HookedCounterEl { label: "multi".into() }).key("m");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "m").unwrap();
        assert!(r.has_active()); // interval active

        // Tick should work
        std::thread::sleep(Duration::from_millis(5));
        r.tick();
        assert!(r.state_mut::<HookedCounter>(id).1 > 0); // count incremented

        // Tombstone cleans everything
        r.rebuild(container, Elements::new());
        assert!(!r.has_active());
    }

    // --- HStack / horizontal layout tests ---

    use crate::node::WidthConstraint;

    #[test]
    fn allocate_widths_all_fill() {
        let constraints = vec![WidthConstraint::Fill, WidthConstraint::Fill];
        let widths = super::allocate_widths(&constraints, 80);
        assert_eq!(widths, vec![40, 40]);
    }

    #[test]
    fn allocate_widths_fill_with_remainder() {
        let constraints = vec![WidthConstraint::Fill, WidthConstraint::Fill, WidthConstraint::Fill];
        let widths = super::allocate_widths(&constraints, 80);
        // 80 / 3 = 26 remainder 2 → first two get 27, last gets 26
        assert_eq!(widths, vec![27, 27, 26]);
        assert_eq!(widths.iter().sum::<u16>(), 80);
    }

    #[test]
    fn allocate_widths_fixed_plus_fill() {
        let constraints = vec![
            WidthConstraint::Fixed(2),
            WidthConstraint::Fill,
        ];
        let widths = super::allocate_widths(&constraints, 80);
        assert_eq!(widths, vec![2, 78]);
    }

    #[test]
    fn allocate_widths_fixed_exceeds_total() {
        let constraints = vec![
            WidthConstraint::Fixed(50),
            WidthConstraint::Fixed(50),
            WidthConstraint::Fill,
        ];
        let widths = super::allocate_widths(&constraints, 80);
        // Fixed: 50 + 50 = 100 > 80. Each fixed clamped to 80.
        // Fill gets 0 (saturating_sub).
        assert_eq!(widths[2], 0);
    }

    #[test]
    fn allocate_widths_single_fill() {
        let constraints = vec![WidthConstraint::Fill];
        let widths = super::allocate_widths(&constraints, 80);
        assert_eq!(widths, vec![80]);
    }

    #[test]
    fn hstack_measure_height_uses_max() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // HStack with two children of different heights
        let hstack = r.append_child(container, crate::component::HStack);
        r.set_layout(hstack, crate::node::Layout::Horizontal);

        let child1 = r.append_child(hstack, TextBlock);
        r.state_mut::<TextBlock>(child1).push("line1".to_string());
        r.state_mut::<TextBlock>(child1).push("line2".to_string());
        // child1: 2 lines tall

        let child2 = r.append_child(hstack, TextBlock);
        r.state_mut::<TextBlock>(child2).push("one".to_string());
        // child2: 1 line tall

        let frame = r.render();
        // HStack height = max(2, 1) = 2
        assert_eq!(frame.area().height, 2);
    }

    #[test]
    fn hstack_renders_side_by_side() {
        let mut r = Renderer::new(20);

        // Use declarative API for HStack
        let container = r.push(VStack);

        let mut row = Elements::new();
        row.add(TestTextEl::new(">")).width(WidthConstraint::Fixed(2));
        row.add(TestTextEl::new("hello"));

        let mut els = Elements::new();
        els.hstack(row);
        r.rebuild(container, els);

        let frame = r.render();
        let buf = frame.buffer();

        // ">" at x=0
        assert_eq!(buf[(0, 0)].symbol(), ">");
        // "hello" starts at x=2
        assert_eq!(buf[(2, 0)].symbol(), "h");
        assert_eq!(buf[(3, 0)].symbol(), "e");
    }

    #[test]
    fn hstack_two_fill_columns() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        let mut row = Elements::new();
        row.add(TestTextEl::new("left"));
        row.add(TestTextEl::new("right"));

        let mut els = Elements::new();
        els.hstack(row);
        r.rebuild(container, els);

        let frame = r.render();
        let buf = frame.buffer();

        // "left" at x=0 (first 10 cols)
        assert_eq!(buf[(0, 0)].symbol(), "l");
        // "right" at x=10 (second 10 cols)
        assert_eq!(buf[(10, 0)].symbol(), "r");
    }

    #[test]
    fn hstack_nested_in_vstack() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(TestTextEl::new("above"));

        let mut row = Elements::new();
        row.add(TestTextEl::new("$")).width(WidthConstraint::Fixed(2));
        row.add(TestTextEl::new("cmd"));
        els.hstack(row);

        els.add(TestTextEl::new("below"));
        r.rebuild(container, els);

        let frame = r.render();
        assert_eq!(frame.area().height, 3);

        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), "a"); // "above"
        assert_eq!(buf[(0, 1)].symbol(), "$"); // symbol column
        assert_eq!(buf[(2, 1)].symbol(), "c"); // "cmd" at x=2
        assert_eq!(buf[(0, 2)].symbol(), "b"); // "below"
    }

    #[test]
    fn hstack_reconciliation_preserves_width() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // First build
        let mut row = Elements::new();
        row.add(TestTextEl::new(">")).width(WidthConstraint::Fixed(2));
        row.add(TestTextEl::new("v1"));
        let mut els = Elements::new();
        els.hstack(row).key("row");
        r.rebuild(container, els);

        let frame1 = r.render();
        assert_eq!(frame1.buffer()[(2, 0)].symbol(), "v"); // "v1" at x=2

        // Rebuild — content changes but layout preserved
        let mut row = Elements::new();
        row.add(TestTextEl::new("$")).width(WidthConstraint::Fixed(2));
        row.add(TestTextEl::new("v2"));
        let mut els = Elements::new();
        els.hstack(row).key("row");
        r.rebuild(container, els);

        let frame2 = r.render();
        assert_eq!(frame2.buffer()[(0, 0)].symbol(), "$");
        assert_eq!(frame2.buffer()[(2, 0)].symbol(), "v"); // "v2" at x=2
    }

    // --- Content inset tests ---

    use crate::insets::Insets;

    /// A container component with configurable insets.
    struct PaddedBox;

    impl Component for PaddedBox {
        type State = Insets;

        fn render(&self, area: Rect, buf: &mut Buffer, _state: &Self::State) {
            // Draw a border character at corners to verify chrome rendering
            if area.width > 0 && area.height > 0 {
                buf[(area.x, area.y)].set_symbol("+");
                if area.width > 1 {
                    buf[(area.x + area.width - 1, area.y)].set_symbol("+");
                }
                if area.height > 1 {
                    buf[(area.x, area.y + area.height - 1)].set_symbol("+");
                    if area.width > 1 {
                        buf[(area.x + area.width - 1, area.y + area.height - 1)]
                            .set_symbol("+");
                    }
                }
            }
        }

        fn desired_height(&self, _width: u16, _state: &Self::State) -> u16 {
            0
        }

        fn content_inset(&self, state: &Self::State) -> Insets {
            *state
        }

        fn initial_state(&self) -> Insets {
            Insets::ZERO
        }
    }

    #[test]
    fn zero_insets_children_get_full_area() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // PaddedBox with zero insets — children should get full width
        let padded = r.append_child(container, PaddedBox);
        let child = r.append_child(padded, TextBlock);
        r.state_mut::<TextBlock>(child).push("hello".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 1);
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "h"); // child at x=0
    }

    #[test]
    fn uniform_insets_shrink_child_area() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // PaddedBox with 1-cell insets on all sides
        let padded = r.append_child(container, PaddedBox);
        **r.state_mut::<PaddedBox>(padded) = Insets::all(1);

        let child = r.append_child(padded, TextBlock);
        r.state_mut::<TextBlock>(child).push("hello".to_string());

        let frame = r.render();
        // Height: 1 (top inset) + 1 (child) + 1 (bottom inset) = 3
        assert_eq!(frame.area().height, 3);

        let buf = frame.buffer();
        // Chrome: corner at (0,0)
        assert_eq!(buf[(0, 0)].symbol(), "+");
        // Child "hello" at (1, 1) — offset by left and top insets
        assert_eq!(buf[(1, 1)].symbol(), "h");
        assert_eq!(buf[(2, 1)].symbol(), "e");
    }

    #[test]
    fn insets_reduce_inner_width() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // PaddedBox with 3-cell left, 2-cell right insets
        let padded = r.append_child(container, PaddedBox);
        **r.state_mut::<PaddedBox>(padded) = Insets::new().left(3).right(2);

        let child = r.append_child(padded, TextBlock);
        r.state_mut::<TextBlock>(child).push("hello".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 1);

        let buf = frame.buffer();
        // "hello" at x=3 (left inset), not x=0
        assert_eq!(buf[(3, 0)].symbol(), "h");
        assert_eq!(buf[(4, 0)].symbol(), "e");
    }

    #[test]
    fn insets_with_hstack_children() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // PaddedBox with 1-cell insets, horizontal layout inside
        let padded = r.append_child(container, PaddedBox);
        **r.state_mut::<PaddedBox>(padded) = Insets::all(1);
        r.set_layout(padded, Layout::Horizontal);

        let child1 = r.append_child(padded, TextBlock);
        r.state_mut::<TextBlock>(child1).push("L".to_string());
        // child1 gets Fill (default)

        let child2 = r.append_child(padded, TextBlock);
        r.state_mut::<TextBlock>(child2).push("R".to_string());
        // child2 gets Fill (default)

        let frame = r.render();
        // Height: 1 + max(1,1) + 1 = 3
        assert_eq!(frame.area().height, 3);

        let buf = frame.buffer();
        // Inner width = 20 - 2 = 18, split evenly = 9 each
        // "L" at x=1 (left inset), "R" at x=10 (1 + 9)
        assert_eq!(buf[(1, 1)].symbol(), "L");
        assert_eq!(buf[(10, 1)].symbol(), "R");
    }

    #[test]
    fn nested_insets() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // Outer box with 1-cell insets
        let outer = r.append_child(container, PaddedBox);
        **r.state_mut::<PaddedBox>(outer) = Insets::all(1);

        // Inner box with 1-cell insets (nested)
        let inner = r.append_child(outer, PaddedBox);
        **r.state_mut::<PaddedBox>(inner) = Insets::all(1);

        let child = r.append_child(inner, TextBlock);
        r.state_mut::<TextBlock>(child).push("deep".to_string());

        let frame = r.render();
        // Height: 1 + (1 + 1 + 1) + 1 = 5
        assert_eq!(frame.area().height, 5);

        let buf = frame.buffer();
        // Outer chrome at (0,0)
        assert_eq!(buf[(0, 0)].symbol(), "+");
        // Inner chrome at (1,1)
        assert_eq!(buf[(1, 1)].symbol(), "+");
        // "deep" at (2,2) — offset by both insets
        assert_eq!(buf[(2, 2)].symbol(), "d");
    }

    // --- Composite children tests ---

    /// A composite component that generates its own children:
    /// an HStack with a prefix and a text label.
    struct LabeledRow;

    struct LabeledRowState {
        prefix: String,
        label: String,
    }

    impl Component for LabeledRow {
        type State = LabeledRowState;

        fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &Self::State) {}

        fn desired_height(&self, _width: u16, _state: &Self::State) -> u16 {
            0 // children determine height
        }

        fn initial_state(&self) -> LabeledRowState {
            LabeledRowState {
                prefix: String::new(),
                label: String::new(),
            }
        }

        fn children(
            &self,
            state: &Self::State,
            _slot: Option<Elements>,
        ) -> Option<Elements> {
            // Ignore slot — generate own children
            let mut row = Elements::new();
            row.add(TestTextEl::new(&state.prefix))
                .width(WidthConstraint::Fixed(2));
            row.add(TestTextEl::new(&state.label));

            let mut els = Elements::new();
            els.hstack(row);
            Some(els)
        }
    }

    /// Element for LabeledRow.
    struct LabeledRowEl {
        prefix: String,
        label: String,
    }

    impl LabeledRowEl {
        fn new(prefix: &str, label: &str) -> Self {
            Self {
                prefix: prefix.to_string(),
                label: label.to_string(),
            }
        }
    }

    impl Element for LabeledRowEl {
        fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
            let id = renderer.append_child(parent, LabeledRow);
            renderer.set_layout(id, crate::node::Layout::Horizontal);
            let state = renderer.state_mut::<LabeledRow>(id);
            state.prefix = self.prefix;
            state.label = self.label;
            id
        }

        fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
            let state = renderer.state_mut::<LabeledRow>(node_id);
            state.prefix = self.prefix;
            state.label = self.label;
        }
    }

    #[test]
    fn composite_generates_own_children() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(LabeledRowEl::new(">", "hello"));
        r.rebuild(container, els);

        let frame = r.render();
        assert_eq!(frame.area().height, 1);

        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), ">");
        assert_eq!(buf[(2, 0)].symbol(), "h"); // "hello" at x=2
    }

    #[test]
    fn composite_children_reconciled_across_rebuilds() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // First build
        let mut els = Elements::new();
        els.add(LabeledRowEl::new(">", "v1")).key("row");
        r.rebuild(container, els);

        let row_id = r.find_by_key(container, "row").unwrap();

        // Rebuild — composite is reused, children regenerated
        let mut els = Elements::new();
        els.add(LabeledRowEl::new("$", "v2")).key("row");
        r.rebuild(container, els);

        // Same row node reused
        assert_eq!(r.find_by_key(container, "row").unwrap(), row_id);

        let frame = r.render();
        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), "$"); // prefix updated
        assert_eq!(buf[(2, 0)].symbol(), "v"); // label updated
    }

    #[test]
    fn default_passthrough_still_works() {
        // VStack uses default children() which returns slot
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut inner = Elements::new();
        inner.add(TestTextEl::new("child1"));
        inner.add(TestTextEl::new("child2"));

        let mut els = Elements::new();
        els.add_with_children(crate::elements::VStackEl, inner);
        r.rebuild(container, els);

        let frame = r.render();
        assert_eq!(frame.area().height, 2);
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "c"); // "child1"
        assert_eq!(frame.buffer()[(0, 1)].symbol(), "c"); // "child2"
    }

    /// A wrapper component that accepts slot children and adds a header.
    struct BannerComponent;

    impl Component for BannerComponent {
        type State = String; // banner title

        fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &Self::State) {}
        fn desired_height(&self, _width: u16, _state: &Self::State) -> u16 { 0 }
        fn initial_state(&self) -> String { String::new() }

        fn children(
            &self,
            state: &Self::State,
            slot: Option<Elements>,
        ) -> Option<Elements> {
            let mut els = Elements::new();
            // Add banner title
            els.add(TestTextEl::new(state));
            // Include slot children
            if let Some(slot) = slot {
                for _entry in slot.into_items() {
                    // Re-wrap each entry
                    els.add(TestTextEl::new("slot"));
                }
            }
            Some(els)
        }
    }

    struct BannerEl { title: String }

    impl Element for BannerEl {
        fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
            let id = renderer.append_child(parent, BannerComponent);
            let state = renderer.state_mut::<BannerComponent>(id);
            state.push_str(&self.title);
            id
        }

        fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
            let state = renderer.state_mut::<BannerComponent>(node_id);
            state.clear();
            state.push_str(&self.title);
        }
    }

    #[test]
    fn composite_wraps_slot_children() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        let mut slot = Elements::new();
        slot.add(TestTextEl::new("content"));

        let mut els = Elements::new();
        els.add_with_children(BannerEl { title: "TITLE".into() }, slot);
        r.rebuild(container, els);

        let frame = r.render();
        // Should have: TITLE + slot placeholder
        assert_eq!(frame.area().height, 2);
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "T"); // "TITLE"
        assert_eq!(frame.buffer()[(0, 1)].symbol(), "s"); // "slot" (wrapper)
    }

    #[test]
    fn nested_composites() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // A LabeledRow (composite) inside a VStack
        let mut els = Elements::new();
        els.add(TestTextEl::new("above"));
        els.add(LabeledRowEl::new(">", "nested"));
        els.add(TestTextEl::new("below"));
        r.rebuild(container, els);

        let frame = r.render();
        assert_eq!(frame.area().height, 3);

        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), "a"); // "above"
        assert_eq!(buf[(0, 1)].symbol(), ">"); // prefix
        assert_eq!(buf[(2, 1)].symbol(), "n"); // "nested" at x=2
        assert_eq!(buf[(0, 2)].symbol(), "b"); // "below"
    }

    // --- Hooks / lifecycle tests ---

    use crate::hooks::Hooks;

    /// A component that uses lifecycle to manage its own interval.
    struct HookedCounter;

    impl Component for HookedCounter {
        type State = (String, usize); // (label, count)

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let line = ratatui_core::text::Line::raw(state.0.as_str());
            ratatui_core::widgets::Widget::render(Paragraph::new(line), area, buf);
        }

        fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
            if state.0.is_empty() { 0 } else { 1 }
        }

        fn initial_state(&self) -> (String, usize) {
            (String::new(), 0)
        }

        fn lifecycle(&self, hooks: &mut Hooks<(String, usize)>, state: &(String, usize)) {
            if state.0 != "stop" {
                hooks.use_interval(Duration::from_millis(1), |s| {
                    s.1 += 1;
                });
            }
            // When label is "stop", no interval → old interval cleared
        }
    }

    struct HookedCounterEl { label: String }

    impl Element for HookedCounterEl {
        fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
            let id = renderer.append_child(parent, HookedCounter);
            let state = renderer.state_mut::<HookedCounter>(id);
            state.0 = self.label;
            id
        }

        fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
            let state = renderer.state_mut::<HookedCounter>(node_id);
            state.0 = self.label;
            // No effect management — lifecycle handles it
        }
    }

    #[test]
    fn lifecycle_registers_effects_on_build() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(HookedCounterEl { label: "active".into() });
        r.rebuild(container, els);

        // Lifecycle should have registered an interval
        assert!(r.has_active());
    }

    #[test]
    fn lifecycle_clears_effects_on_update() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build with active interval
        let mut els = Elements::new();
        els.add(HookedCounterEl { label: "active".into() }).key("c");
        r.rebuild(container, els);
        assert!(r.has_active());

        // Update to "stop" — lifecycle re-runs, no interval registered → cleared
        let mut els = Elements::new();
        els.add(HookedCounterEl { label: "stop".into() }).key("c");
        r.rebuild(container, els);
        assert!(!r.has_active());
    }

    #[test]
    fn lifecycle_interval_fires() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(HookedCounterEl { label: "go".into() }).key("c");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "c").unwrap();
        assert_eq!(r.state_mut::<HookedCounter>(id).1, 0); // count starts at 0

        std::thread::sleep(Duration::from_millis(5));
        r.tick();

        assert!(r.state_mut::<HookedCounter>(id).1 > 0); // count incremented
    }

    #[test]
    fn lifecycle_with_mount_and_unmount() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Component that uses lifecycle for mount/unmount
        struct MountTracker;
        impl Component for MountTracker {
            type State = Vec<String>;
            fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
                let text = state.join(",");
                let line = ratatui_core::text::Line::raw(text);
                ratatui_core::widgets::Widget::render(Paragraph::new(line), area, buf);
            }
            fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
                if state.is_empty() { 0 } else { 1 }
            }
            fn initial_state(&self) -> Vec<String> { Vec::new() }
            fn lifecycle(&self, hooks: &mut Hooks<Vec<String>>, _state: &Vec<String>) {
                hooks.use_mount(|s| s.push("mounted".into()));
                hooks.use_unmount(|s| s.push("unmounted".into()));
            }
        }

        struct MountTrackerEl;
        impl Element for MountTrackerEl {
            fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
                let id = renderer.append_child(parent, MountTracker);
                renderer.state_mut::<MountTracker>(id).push("built".into());
                id
            }
            fn update(self: Box<Self>, _renderer: &mut Renderer, _node_id: NodeId) {}
        }

        let mut els = Elements::new();
        els.add(MountTrackerEl).key("mt");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "mt").unwrap();
        let state = r.state_mut::<MountTracker>(id);
        assert!(state.contains(&"built".to_string()));
        assert!(state.contains(&"mounted".to_string()));

        // Tombstone — unmount fires
        r.rebuild(container, Elements::new());
        let state = r.state_mut::<MountTracker>(id);
        assert!(state.contains(&"unmounted".to_string()));
    }
}
