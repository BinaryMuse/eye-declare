use std::any::{Any, TypeId};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use ratatui_core::{buffer::Buffer, layout::Rect};

use crate::component::{Component, EventResult, Tracked, VStack};
use crate::context::{ContextMap, ProvidedContexts, SavedContext};
use crate::element::{ElementEntry, Elements};
use crate::frame::Frame;
use crate::node::{
    Effect, EffectKind, Layout, Node, NodeArena, NodeId, TypedEffectHandler, WidthConstraint,
};

/// Manages a tree of components and renders them into a Frame.
///
/// The tree has an implicit root node (a VStack) created automatically.
/// Components are added as children of the root or of other nodes.
/// Children are laid out vertically within their parent's area.
pub struct Renderer {
    nodes: NodeArena,
    root: NodeId,
    width: u16,
    focused: Option<NodeId>,
    /// After rendering, the absolute cursor position for the focused
    /// component (if it returns one from cursor_position).
    cursor_hint: Option<(u16, u16)>,
    /// Registered effects per node. Multiple effects per node supported.
    effects: HashMap<NodeId, Vec<Effect>>,
    /// Context values available during reconciliation. Providers push
    /// values before their subtree is processed and pop them after.
    context: ContextMap,
    /// Saved focus per focus scope node. When autofocus moves focus into
    /// a scope, the previous focus is saved here so it can be restored
    /// when the scope is removed.
    saved_focus: HashMap<NodeId, Option<NodeId>>,
}

impl Renderer {
    /// Create a new renderer with the given terminal width.
    /// An implicit VStack root node is created automatically.
    pub fn new(width: u16) -> Self {
        let mut nodes = NodeArena::new();
        let root = nodes.alloc(Node::new(VStack));
        // Root starts clean since VStack has no visible content
        nodes[root].state.clear_dirty();
        Self {
            nodes,
            root,
            width,
            focused: None,
            cursor_hint: None,
            effects: HashMap::new(),
            context: ContextMap::new(),
            saved_focus: HashMap::new(),
        }
    }

    /// The root node's ID.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Add a component as a child of the given parent. Returns its NodeId.
    pub fn append_child<C: Component>(&mut self, parent: NodeId, component: C) -> NodeId {
        let layout = component.layout();
        let width_constraint = component.width_constraint();
        let mut node = Node::new(component);
        node.parent = Some(parent);
        node.layout = layout;
        node.width_constraint = width_constraint;
        let id = self.nodes.alloc(node);
        self.nodes[parent].children.push(id);
        id
    }

    /// Swap the component on an existing node, preserving state.
    ///
    /// Replaces the component (props) while keeping state intact.
    /// Used by reconciliation and for imperative prop updates.
    ///
    /// Note: this resets `layout` and `width_constraint` from the component's
    /// trait methods. When hooks override these (via `use_layout`/`use_width_constraint`),
    /// `update_node` must be called after this to restore the hook values.
    /// Every current reconciliation path satisfies this contract.
    pub fn swap_component<C: Component>(&mut self, id: NodeId, component: C) {
        let layout = component.layout();
        let width_constraint = component.width_constraint();
        self.nodes[id].component = Box::new(component);
        self.nodes[id].layout = layout;
        self.nodes[id].width_constraint = width_constraint;
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
        let node = &mut self.nodes[id];
        node.state
            .as_any_mut()
            .downcast_mut::<Tracked<C::State>>()
            .expect("state type mismatch in state_mut")
    }

    /// Freeze a component. Frozen components use their cached buffer
    /// and are not re-rendered on subsequent frames.
    pub fn freeze(&mut self, id: NodeId) {
        self.nodes[id].frozen = true;
    }

    /// Set the layout direction for a container node.
    #[cfg(test)]
    pub fn set_layout(&mut self, id: NodeId, layout: Layout) {
        self.nodes[id].layout = layout;
    }

    /// List the children of a node.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id].children
    }

    /// Get the key assigned to a node (if any).
    pub fn node_key(&self, id: NodeId) -> Option<&str> {
        self.nodes[id].key.as_deref()
    }

    /// Get the last rendered height of a node.
    pub fn node_last_height(&self, id: NodeId) -> u16 {
        self.nodes[id].last_height.unwrap_or(0)
    }

    /// Set which component has focus for event routing.
    ///
    /// If the target node is inside a focus scope and focus is currently
    /// outside that scope, the current focus is saved so it can be
    /// restored when the scope is removed.
    pub fn set_focus(&mut self, id: NodeId) {
        if let Some(scope_id) = self.find_scope_for(id) {
            // Only save if the current focus is outside this scope
            let current_outside = self
                .focused
                .is_none_or(|f| !self.is_in_subtree(f, scope_id));
            if current_outside {
                self.saved_focus.entry(scope_id).or_insert(self.focused);
            }
        }
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

    /// Deliver an event to the component tree using capture + bubble phases.
    ///
    /// Tab and Shift-Tab are intercepted first for focus cycling among
    /// focusable components (depth-first tree order). All other events
    /// go through two-phase dispatch:
    ///
    /// 1. **Capture** (root → focused): each node's
    ///    [`handle_event_capture`](crate::Component::handle_event_capture) is called.
    ///    Returning [`EventResult::Consumed`] stops propagation immediately.
    /// 2. **Bubble** (focused → root): each node's
    ///    [`handle_event`](crate::Component::handle_event) is called.
    ///    Returning [`EventResult::Consumed`] stops propagation.
    ///
    /// Frozen nodes are skipped in both phases.
    ///
    /// **Note:** Tab/Shift-Tab focus cycling is intercepted *before* both
    /// phases. When cycling succeeds, neither phase runs. When it falls
    /// through (0 or 1 focusable nodes), normal two-phase dispatch applies.
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

            if (is_tab || is_backtab) && self.cycle_focus(is_backtab) {
                return EventResult::Consumed;
            }
            // If Tab/BackTab didn't cycle focus (0 or 1 focusable nodes),
            // fall through to normal event handling so components can use
            // the key for their own purposes.
        }

        let Some(focused) = self.focused else {
            return EventResult::Ignored;
        };

        // Build path from root to focused node
        let path = self.path_to_node(focused);

        // Capture phase: root → focused
        for &id in &path {
            let node = &mut self.nodes[id];
            if node.frozen {
                continue;
            }
            // Hook handler takes priority over trait method
            let result = if let Some(ref hook) = node.hook_capture {
                hook.call(event, node.state.as_any_mut())
            } else {
                let state_any = node.state.as_any_mut();
                node.component.handle_event_capture_erased(event, state_any)
            };
            if result == EventResult::Consumed {
                return EventResult::Consumed;
            }
        }

        // Bubble phase: focused → root
        for &id in path.iter().rev() {
            let node = &mut self.nodes[id];
            if node.frozen {
                continue;
            }
            // Hook handler takes priority over trait method
            let result = if let Some(ref hook) = node.hook_event {
                hook.call(event, node.state.as_any_mut())
            } else {
                let state_any = node.state.as_any_mut();
                node.component.handle_event_erased(event, state_any)
            };
            if result == EventResult::Consumed {
                return EventResult::Consumed;
            }
        }

        EventResult::Ignored
    }

    /// Build the path from root to a given node (inclusive).
    fn path_to_node(&self, target: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut current = Some(target);
        while let Some(id) = current {
            path.push(id);
            current = self.nodes[id].parent;
        }
        path.reverse();
        path
    }

    /// Check if a node is focusable (hook value overrides trait method).
    fn is_node_focusable(&self, id: NodeId) -> bool {
        let node = &self.nodes[id];
        if let Some(focusable) = node.hook_focusable {
            return focusable;
        }
        let state = node.state.inner_as_any();
        node.component.is_focusable_erased(state)
    }

    /// Collect focusable node IDs in depth-first tree order.
    fn focusable_nodes(&self) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.collect_focusable(self.root, &mut result);
        result
    }

    fn collect_focusable(&self, id: NodeId, result: &mut Vec<NodeId>) {
        let node = &self.nodes[id];
        if node.frozen {
            return;
        }
        if self.is_node_focusable(id) {
            result.push(id);
        }
        for &child in &node.children {
            self.collect_focusable(child, result);
        }
    }

    /// Find the deepest focus scope ancestor of a node (including self).
    fn find_scope_for(&self, node_id: NodeId) -> Option<NodeId> {
        let mut current = Some(node_id);
        while let Some(id) = current {
            if self.nodes[id].focus_scope {
                return Some(id);
            }
            current = self.nodes[id].parent;
        }
        None
    }

    /// Collect focusable nodes within a focus scope's subtree (DFS),
    /// stopping at nested scope boundaries.
    fn focusable_nodes_in_scope(&self, scope_id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.collect_focusable_scoped(scope_id, scope_id, &mut result);
        result
    }

    fn collect_focusable_scoped(&self, id: NodeId, scope_id: NodeId, result: &mut Vec<NodeId>) {
        let node = &self.nodes[id];
        if node.frozen {
            return;
        }
        // Check focusability before the scope boundary check so that
        // a node which is both focusable and a scope boundary can still
        // participate in its parent scope's Tab cycle.
        if self.is_node_focusable(id) {
            result.push(id);
        }
        // Stop descent at nested scope boundaries (but not the scope itself).
        // The node itself was already considered above.
        if id != scope_id && node.focus_scope {
            return;
        }
        for &child in &node.children {
            self.collect_focusable_scoped(child, scope_id, result);
        }
    }

    /// Check whether `node_id` is within the subtree rooted at `subtree_root`.
    fn is_in_subtree(&self, node_id: NodeId, subtree_root: NodeId) -> bool {
        let mut current = Some(node_id);
        while let Some(id) = current {
            if id == subtree_root {
                return true;
            }
            current = self.nodes[id].parent;
        }
        false
    }

    /// Cycle focus to the next (or previous) focusable component.
    ///
    /// When focus is inside a focus scope, cycling is confined to that
    /// scope's subtree (stopping at nested scope boundaries). When no
    /// scope encloses the focused node, cycling covers the entire tree.
    ///
    /// Returns `true` if focus actually moved to a different component.
    /// Returns `false` if there are no focusable components or only one
    /// (so Tab/BackTab should fall through to normal event handling).
    fn cycle_focus(&mut self, reverse: bool) -> bool {
        let scope = self.focused.and_then(|f| self.find_scope_for(f));

        let focusable = match scope {
            Some(scope_id) => self.focusable_nodes_in_scope(scope_id),
            None => self.focusable_nodes(),
        };

        if focusable.is_empty() {
            return false;
        }

        let current_idx = self
            .focused
            .and_then(|f| focusable.iter().position(|&id| id == f));

        let next_idx = match current_idx {
            Some(idx) => {
                if focusable.len() == 1 {
                    // Only one focusable component — nowhere to cycle
                    return false;
                }
                if reverse {
                    if idx == 0 {
                        focusable.len() - 1
                    } else {
                        idx - 1
                    }
                } else {
                    (idx + 1) % focusable.len()
                }
            }
            None => 0, // No current focus → focus first focusable
        };

        self.set_focus(focusable[next_idx]);
        true
    }

    /// Remove a node and all its descendants from the tree.
    ///
    /// # Panics
    /// Panics if trying to remove the root node.
    pub fn remove(&mut self, id: NodeId) {
        assert!(id != self.root, "cannot remove root node");

        // Remove from parent's children list and invalidate parent's cached height
        if let Some(parent) = self.nodes[id].parent {
            self.nodes[parent].children.retain(|&child| child != id);
            self.nodes[parent].force_dirty = true;
            self.nodes[parent].last_height = None;
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
    ///     els.add(TextBlock::new().unstyled("Hello"));
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
        self.nodes[parent]
            .children
            .iter()
            .find(|&&child_id| self.nodes[child_id].key.as_deref() == Some(key))
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
                if let EffectKind::Interval {
                    last_tick,
                    interval,
                } = &effect.kind
                    && now.duration_since(*last_tick) >= *interval
                {
                    due.push((id, idx));
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
            if let EffectKind::Interval {
                ref mut last_tick, ..
            } = effect.kind
            {
                *last_tick = now;
            }
            effects[idx].handler.call(self.nodes[id].state.as_any_mut());
            self.effects.insert(id, effects);
        }

        true
    }

    /// Whether there are any active tick registrations.
    pub fn has_active(&self) -> bool {
        self.effects.values().any(|effects| {
            effects
                .iter()
                .any(|e| matches!(e.kind, EffectKind::Interval { .. }))
        })
    }

    /// Fire and remove all OnMount effects for a node.
    fn fire_mount(&mut self, id: NodeId) {
        // Autofocus: set focus when this node mounts (if focusable)
        if self.nodes[id].autofocus && self.is_node_focusable(id) {
            self.set_focus(id);
        }

        if let Some(effects) = self.effects.remove(&id) {
            // Extract mount handlers, keep the rest
            let (mounts, remaining): (Vec<_>, Vec<_>) = effects
                .into_iter()
                .partition(|e| matches!(e.kind, EffectKind::OnMount));

            // Fire mount handlers
            for effect in mounts {
                effect.handler.call(self.nodes[id].state.as_any_mut());
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
                effect.handler.call(self.nodes[id].state.as_any_mut());
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
        let old_children: Vec<NodeId> = std::mem::take(&mut self.nodes[parent].children);

        // Separate old children into keyed and unkeyed
        let mut old_by_key: HashMap<String, NodeId> = HashMap::new();
        let mut old_unkeyed: VecDeque<NodeId> = VecDeque::new();

        for &child_id in &old_children {
            if let Some(ref key) = self.nodes[child_id].key {
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
                    Some(old_id) if self.nodes[old_id].element_type_id == Some(entry.type_id) => {
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
                    if self.nodes[front_id].element_type_id == Some(entry.type_id) {
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
                self.nodes[old_id].parent = Some(parent);
                self.nodes[old_id].width_constraint =
                    resolve_width_constraint(&self.nodes[old_id], entry.width_constraint);
                // Guarantee re-render after props update
                self.nodes[old_id].force_dirty = true;
                let (provided, resolved) = self.update_node(old_id, entry.children);
                let saved = self.push_context(provided);

                if let Some(els) = resolved {
                    self.reconcile_children(old_id, els.into_items());
                }

                self.pop_context(saved);
                old_id
            } else {
                // BUILD: create new node
                let id = entry.element.build(self, parent);
                self.nodes[id].element_type_id = Some(entry.type_id);
                self.nodes[id].key = entry.key;
                self.nodes[id].width_constraint =
                    resolve_width_constraint(&self.nodes[id], entry.width_constraint);
                let (provided, resolved) = self.update_node(id, entry.children);
                self.fire_mount(id);
                let saved = self.push_context(provided);

                if let Some(els) = resolved {
                    self.build_elements(id, els);
                }

                self.pop_context(saved);
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

        self.nodes[parent].children = new_children;
        // Children changed — force re-measure and re-render of the parent.
        self.nodes[parent].force_dirty = true;
        self.nodes[parent].last_height = None;
    }

    /// Build elements into the tree as children of `parent`.
    ///
    /// Sets element_type_id and key on newly created nodes so they
    /// can participate in future reconciliation.
    fn build_elements(&mut self, parent: NodeId, elements: Elements) {
        for entry in elements.into_items() {
            let node_id = entry.element.build(self, parent);
            self.nodes[node_id].element_type_id = Some(entry.type_id);
            self.nodes[node_id].key = entry.key;
            self.nodes[node_id].width_constraint =
                resolve_width_constraint(&self.nodes[node_id], entry.width_constraint);
            let (provided, resolved) = self.update_node(node_id, entry.children);
            self.fire_mount(node_id);
            let saved = self.push_context(provided);

            if let Some(els) = resolved {
                self.build_elements(node_id, els);
            }

            self.pop_context(saved);
        }
    }

    /// Run the component's combined update: collect hooks and produce
    /// the element tree in a single call.
    ///
    /// Returns provided context values (for descendants) and resolved
    /// children elements. Applies lifecycle output (effects, hook
    /// overrides) to the node.
    fn update_node(
        &mut self,
        id: NodeId,
        slot: Option<Elements>,
    ) -> (ProvidedContexts, Option<Elements>) {
        let children = slot.unwrap_or_default();

        let (output, result) = {
            let context = &self.context;
            let node = &mut self.nodes[id];
            node.component
                .update_erased(node.state.as_any_mut(), context, children)
        };

        // Apply lifecycle output
        if output.effects.is_empty() {
            self.effects.remove(&id);
        } else {
            self.effects.insert(id, output.effects);
        }
        self.nodes[id].autofocus = output.autofocus;
        self.nodes[id].focus_scope = output.focus_scope;
        self.nodes[id].hook_focusable = output.focusable;
        self.nodes[id].hook_cursor = output.cursor_hook;
        self.nodes[id].hook_event = output.event_hook;
        self.nodes[id].hook_capture = output.capture_hook;
        if let Some(layout) = output.layout {
            self.nodes[id].layout = layout;
        }
        if let Some(wc) = output.width_constraint {
            self.nodes[id].width_constraint = wc;
        }

        // Return Some even when empty if the node already has children,
        // so reconcile_children can tombstone them on transition to empty.
        let elements = if result.is_empty() && self.nodes[id].children.is_empty() {
            None
        } else {
            Some(result)
        };

        (output.provided, elements)
    }

    /// Test helper: run update_node for lifecycle side effects only.
    /// Used by focus scope tests that need hooks applied on imperatively
    /// built nodes. Returns provided context values.
    #[cfg(test)]
    fn apply_lifecycle(&mut self, id: NodeId) -> Vec<(TypeId, Box<dyn Any + Send + Sync>)> {
        let (provided, _) = self.update_node(id, None);
        provided
    }

    /// Tombstone a node and all its descendants without touching the
    /// parent's children list (caller is responsible for that).
    fn tombstone_subtree(&mut self, id: NodeId) {
        // Focus scope restoration: if this node is a scope boundary and
        // focus is currently inside it, restore the saved pre-scope focus.
        // This must happen before recursing, since children will be freed.
        if self.nodes[id].focus_scope {
            let focus_inside = self.focused.is_some_and(|f| self.is_in_subtree(f, id));
            if focus_inside {
                let restored = self.saved_focus.remove(&id).flatten();
                self.focused = restored.filter(|&r| {
                    // Validate: the saved node must still be live and
                    // must not be part of this subtree being removed.
                    //
                    // Note: is_live checks slot occupancy, not identity.
                    // If the saved NodeId was freed and its arena slot
                    // recycled for a different node, this will incorrectly
                    // match. A generational NodeId would close this gap
                    // but is a broader arena change. In practice the risk
                    // is low: scopes are typically removed in the same
                    // rebuild pass that frees their saved target, and
                    // arena recycling across rebuilds is rare.
                    self.nodes.is_live(r) && !self.is_in_subtree(r, id)
                });
                // If the saved target is gone, fall back to the first
                // focusable node in the parent scope (or the whole tree).
                if self.focused.is_none() {
                    let parent_scope = self.nodes[id].parent.and_then(|p| self.find_scope_for(p));
                    let candidates = match parent_scope {
                        Some(ps) => self.focusable_nodes_in_scope(ps),
                        None => self.focusable_nodes(),
                    };
                    if let Some(fallback) =
                        candidates.into_iter().find(|&n| !self.is_in_subtree(n, id))
                    {
                        self.set_focus(fallback);
                    }
                }
            } else {
                self.saved_focus.remove(&id);
            }
        }

        // Children unmount first (bottom-up), then parent
        let children = std::mem::take(&mut self.nodes[id].children);
        for child_id in children {
            self.tombstone_subtree(child_id);
        }
        self.fire_unmount(id);
        self.effects.remove(&id);
        if self.focused == Some(id) {
            self.focused = None;
        }
        self.saved_focus.remove(&id);
        self.nodes.free(id);
    }

    /// Insert a root-level context value, available to all components.
    ///
    /// Root contexts are never popped — they persist for the lifetime
    /// of the renderer. Used by [`ApplicationBuilder::with_context`](crate::ApplicationBuilder::with_context).
    pub fn set_root_context<T: Any + Send + Sync>(&mut self, value: T) {
        self.context.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Insert a root-level context value from a type-erased box.
    pub(crate) fn set_root_context_raw(
        &mut self,
        type_id: TypeId,
        value: Box<dyn Any + Send + Sync>,
    ) {
        self.context.insert(type_id, value);
    }

    /// Push provided context values onto the context map, saving
    /// previous values for later restoration.
    fn push_context(&mut self, provided: ProvidedContexts) -> SavedContext {
        let mut saved = Vec::with_capacity(provided.len());
        for (type_id, value) in provided {
            let old = self.context.insert(type_id, value);
            saved.push((type_id, old));
        }
        saved
    }

    /// Restore context values saved by a previous `push_context` call.
    fn pop_context(&mut self, saved: SavedContext) {
        for (type_id, old) in saved {
            match old {
                Some(v) => {
                    self.context.insert(type_id, v);
                }
                None => {
                    self.context.remove(&type_id);
                }
            }
        }
    }

    /// Set the rendering width (e.g., on terminal resize).
    /// Invalidates all cached buffers and marks all non-frozen nodes
    /// dirty so they re-render at the new width.
    pub fn set_width(&mut self, width: u16) {
        if self.width != width {
            self.width = width;
            for node in self.nodes.iter_mut() {
                node.cached_buffer = None;
                node.last_height = None;
                // Force dirty so non-frozen nodes re-render even if
                // state wasn't mutated via DerefMut
                if !node.frozen {
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
            let node = &self.nodes[focused];
            if let Some(layout_rect) = node.layout_rect {
                // Hook cursor takes priority over trait method
                let cursor_pos = if let Some(ref hook) = node.hook_cursor {
                    hook.call(layout_rect, node.state.inner_as_any())
                } else {
                    let state = node.state.inner_as_any();
                    node.component.cursor_position_erased(layout_rect, state)
                };
                if let Some((rel_col, rel_row)) = cursor_pos {
                    self.cursor_hint = Some((layout_rect.x + rel_col, layout_rect.y + rel_row));
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
    ///
    /// For containers, sums children heights + insets. For leaves,
    /// performs a probe render into a temporary buffer and scans for
    /// actual content height. The probe buffer is cached for reuse
    /// in `render_node`.
    fn measure_height(&mut self, id: NodeId, width: u16) -> u16 {
        let node = &self.nodes[id];

        if node.frozen {
            return node.last_height.unwrap_or(0);
        }

        let height = if node.is_container() {
            let insets = node
                .component
                .content_inset_erased(node.state.inner_as_any());
            let inner_width = width.saturating_sub(insets.horizontal());

            let children: Vec<NodeId> = node.children.clone();
            let layout = node.layout;

            let children_height = match layout {
                Layout::Vertical => children
                    .iter()
                    .map(|&child| self.measure_height(child, inner_width))
                    .sum(),
                Layout::Horizontal => {
                    let constraints: Vec<WidthConstraint> = children
                        .iter()
                        .map(|&cid| self.nodes[cid].width_constraint)
                        .collect();
                    let widths = allocate_widths(&constraints, inner_width);
                    children
                        .iter()
                        .zip(widths.iter())
                        .map(|(&child, &w)| self.measure_height(child, w))
                        .max()
                        .unwrap_or(0)
                }
            };

            children_height + insets.vertical()
        } else {
            // Leaf: use desired_height hint if provided, otherwise probe render.
            let needs_measure =
                node.force_dirty || node.state.is_dirty() || node.last_height.is_none();
            if !needs_measure {
                // probe_rendered may linger from a previous frame where the node
                // probed to height 0 (render_node never ran to clear it). Safe:
                // the next dirty frame re-probes and overwrites both cached_buffer
                // and probe_rendered before render_node reads them.
                return node.last_height.unwrap_or(0);
            }

            let state = self.nodes[id].state.inner_as_any();
            if let Some(h) = self.nodes[id].component.desired_height_erased(width, state) {
                // Component declared its height — no probe needed.
                h
            } else {
                // Probe render: auto-growing buffer to measure actual content.
                let mut probe_height = INITIAL_PROBE_HEIGHT;
                loop {
                    let probe_area = Rect::new(0, 0, width, probe_height);
                    let mut probe_buf = Buffer::empty(probe_area);
                    let state = self.nodes[id].state.inner_as_any();
                    self.nodes[id]
                        .component
                        .render_erased(probe_area, &mut probe_buf, state);
                    let h = scan_content_height(&probe_buf, width, probe_height);
                    if h < probe_height || probe_height >= MAX_PROBE_HEIGHT {
                        #[cfg(debug_assertions)]
                        if h == MAX_PROBE_HEIGHT {
                            eprintln!(
                                "eye_declare: component content may exceed MAX_PROBE_HEIGHT \
                                 ({MAX_PROBE_HEIGHT} rows); output will be truncated. \
                                 Override desired_height() to declare the height explicitly."
                            );
                        }
                        // Trim the probe buffer to actual content height to avoid
                        // holding an oversized buffer in the cache.
                        if h < probe_height && h > 0 {
                            let trimmed_area = Rect::new(0, 0, width, h);
                            let mut trimmed = Buffer::empty(trimmed_area);
                            copy_buffer(&probe_buf, &mut trimmed, trimmed_area);
                            self.nodes[id].cached_buffer = Some(trimmed);
                        } else {
                            self.nodes[id].cached_buffer = Some(probe_buf);
                        }
                        self.nodes[id].probe_rendered = true;
                        break h;
                    }
                    probe_height = (probe_height * 2).min(MAX_PROBE_HEIGHT);
                }
            }
        };

        self.nodes[id].last_height = Some(height);
        height
    }

    /// Recursively render a node and its children into the buffer.
    fn render_node(&mut self, id: NodeId, area: Rect, buffer: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Store layout rect for cursor positioning
        self.nodes[id].layout_rect = Some(area);

        let node = &self.nodes[id];
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
            // Render the container's own component (background/border/chrome).
            // For transparent containers (VStack, user view() components),
            // render() is a no-op.
            let state = self.nodes[id].state.inner_as_any();
            self.nodes[id].component.render_erased(area, buffer, state);

            let insets = self.nodes[id]
                .component
                .content_inset_erased(self.nodes[id].state.inner_as_any());
            let inner = Rect::new(
                area.x.saturating_add(insets.left),
                area.y.saturating_add(insets.top),
                area.width.saturating_sub(insets.horizontal()),
                area.height.saturating_sub(insets.vertical()),
            );

            let children: Vec<NodeId> = self.nodes[id].children.clone();
            let layout = self.nodes[id].layout;

            match layout {
                Layout::Vertical => {
                    let mut y_offset = inner.y;
                    for child_id in &children {
                        // Use cached height from measure pass
                        let child_height = self.nodes[*child_id].last_height.unwrap_or(0);
                        if child_height == 0 {
                            continue;
                        }
                        let child_area = Rect::new(inner.x, y_offset, inner.width, child_height);
                        self.render_node(*child_id, child_area, buffer);
                        y_offset = y_offset.saturating_add(child_height);
                    }
                }
                Layout::Horizontal => {
                    let constraints: Vec<WidthConstraint> = children
                        .iter()
                        .map(|&cid| self.nodes[cid].width_constraint)
                        .collect();
                    let widths = allocate_widths(&constraints, inner.width);
                    let mut x_offset = inner.x;
                    for (child_id, &child_width) in children.iter().zip(widths.iter()) {
                        if child_width == 0 {
                            continue;
                        }
                        let child_area = Rect::new(x_offset, inner.y, child_width, inner.height);
                        self.render_node(*child_id, child_area, buffer);
                        x_offset = x_offset.saturating_add(child_width);
                    }
                }
            }

            // Cache and clean
            let mut node_buf = Buffer::empty(area);
            copy_buffer_region(buffer, &mut node_buf, area);
            self.nodes[id].cached_buffer = Some(node_buf);
            self.nodes[id].last_height = Some(area.height);
            self.nodes[id].state.clear_dirty();
            self.nodes[id].force_dirty = false;
        } else {
            // Leaf: use probe-rendered buffer if available, otherwise render fresh.
            if self.nodes[id].probe_rendered {
                if let Some(ref cached) = self.nodes[id].cached_buffer {
                    copy_buffer(cached, buffer, area);
                }
                self.nodes[id].probe_rendered = false;
            } else {
                let state = self.nodes[id].state.inner_as_any();
                self.nodes[id].component.render_erased(area, buffer, state);
            }

            // Re-cache with correct area coordinates and clean up.
            let mut node_buf = Buffer::empty(area);
            copy_buffer_region(buffer, &mut node_buf, area);
            self.nodes[id].cached_buffer = Some(node_buf);
            self.nodes[id].last_height = Some(area.height);
            self.nodes[id].state.clear_dirty();
            self.nodes[id].force_dirty = false;
        }
    }
}

/// Resolve the effective width constraint for a node.
///
/// Component-declared constraint takes priority; falls back to the
/// entry's constraint (set via `ElementHandle::width()`).
fn resolve_width_constraint(node: &Node, entry_constraint: WidthConstraint) -> WidthConstraint {
    let comp = node.component.width_constraint_erased();
    if comp != WidthConstraint::default() {
        comp
    } else {
        entry_constraint
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
    let per_fill = if fill_count > 0 {
        remaining / fill_count
    } else {
        0
    };
    let mut remainder = if fill_count > 0 {
        remaining % fill_count
    } else {
        0
    };

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

/// Initial height for leaf probe buffers. Doubled on overflow up to MAX.
const INITIAL_PROBE_HEIGHT: u16 = 64;

/// Maximum probe buffer height. Content beyond this is truncated.
const MAX_PROBE_HEIGHT: u16 = 512;

/// Scan a buffer bottom-to-top for the first row with non-default content.
/// Returns the content height (last non-empty row + 1), or 0 if empty.
fn scan_content_height(buf: &Buffer, width: u16, max_height: u16) -> u16 {
    use ratatui_core::buffer::Cell;
    let empty = Cell::EMPTY;
    for y in (0..max_height).rev() {
        for x in 0..width {
            if let Some(cell) = buf.cell((x, y))
                && cell != &empty
            {
                return y + 1;
            }
        }
    }
    0
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

    /// Local test component for imperative tests. Uses Vec<String> state.
    /// Named differently from the real TextBlock in components.
    struct TextBlock;

    impl Component for TextBlock {
        type State = Vec<String>;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let text: Vec<Line> = state.iter().map(|s| Line::raw(s.as_str())).collect();
            let para = Paragraph::new(text);
            ratatui_core::widgets::Widget::render(para, area, buf);
        }

        fn initial_state(&self) -> Option<Vec<String>> {
            Some(vec![])
        }
    }

    // --- Probe render / scan_content_height tests ---

    #[test]
    fn scan_content_height_empty_buffer() {
        let buf = Buffer::empty(Rect::new(0, 0, 10, 64));
        assert_eq!(scan_content_height(&buf, 10, 64), 0);
    }

    #[test]
    fn scan_content_height_single_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 64));
        buf.set_string(0, 0, "hello", ratatui_core::style::Style::default());
        assert_eq!(scan_content_height(&buf, 10, 64), 1);
    }

    #[test]
    fn scan_content_height_content_in_middle() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 64));
        buf.set_string(0, 0, "line1", ratatui_core::style::Style::default());
        buf.set_string(0, 4, "line5", ratatui_core::style::Style::default());
        assert_eq!(scan_content_height(&buf, 10, 64), 5);
    }

    #[test]
    fn scan_content_height_styled_space() {
        use ratatui_core::style::{Color, Style};
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 64));
        // A cell with just a background color (space character but styled)
        buf.set_style(Rect::new(0, 2, 5, 1), Style::default().bg(Color::Red));
        assert_eq!(scan_content_height(&buf, 10, 64), 3);
    }

    #[test]
    fn probe_render_measures_tall_component() {
        // A component that renders more than INITIAL_PROBE_HEIGHT rows
        // should be measured correctly via auto-grow.
        #[derive(Default)]
        struct TallBlock {
            lines: usize,
        }
        impl Component for TallBlock {
            type State = ();
            fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
                for y in 0..(self.lines as u16).min(area.height) {
                    buf.set_string(
                        area.x,
                        area.y + y,
                        "x",
                        ratatui_core::style::Style::default(),
                    );
                }
            }
        }

        let mut r = Renderer::new(10);
        r.push(TallBlock { lines: 100 });
        let frame = r.render();
        assert_eq!(frame.area().height, 100);
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

        assert!(r.nodes[id].state.is_dirty());
        let _ = r.render();
        assert!(!r.nodes[id].state.is_dirty());
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

        fn handle_event(
            &self,
            event: &crossterm::event::Event,
            state: &mut Tracked<Self::State>,
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

        fn initial_state(&self) -> Option<String> {
            Some(String::new())
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
        assert!(!r.nodes[id].state.is_dirty());

        // Deliver event
        r.handle_event(&key_event('a'));

        // State should be dirty now (component accessed state via DerefMut)
        assert!(r.nodes[id].state.is_dirty());
    }

    #[test]
    fn noop_handler_does_not_mark_dirty() {
        let mut r = Renderer::new(10);

        // Parent has a no-op capture handler (default Ignored, never touches state).
        // Child handles char events via bubble.
        let parent = r.push(VStack);
        let child = r.append_child(parent, InputCapture);
        r.state_mut::<InputCapture>(child).push('x'); // give height
        r.set_focus(child);

        let _ = r.render(); // clear all dirty flags
        assert!(!r.nodes[parent].state.is_dirty());
        assert!(!r.nodes[child].state.is_dirty());

        // Deliver event — VStack's default capture handler runs but
        // never accesses state, so parent must stay clean.
        r.handle_event(&key_event('a'));

        assert!(
            !r.nodes[parent].state.is_dirty(),
            "no-op capture handler must not set dirty"
        );
        // Child's bubble handler did mutate state
        assert!(r.nodes[child].state.is_dirty());
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

        fn is_focusable(&self, _state: &Self::State) -> bool {
            true
        }

        fn initial_state(&self) -> Option<String> {
            Some("item".to_string())
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

        r.state_mut::<TextBlock>(_non_focusable)
            .push("header".to_string());

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
    fn tab_with_no_focusable_nodes_falls_through() {
        let mut r = Renderer::new(10);
        let _id = r.push(TextBlock); // not focusable
        r.state_mut::<TextBlock>(_id).push("text".to_string());

        // Tab should return Ignored so the event can be handled
        // by components for their own purposes (e.g., inserting text).
        let result = r.handle_event(&tab_event());
        assert_eq!(r.focus(), None);
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn tab_with_single_focusable_falls_through() {
        let mut r = Renderer::new(10);
        let f1 = r.push(FocusableItem);

        // First Tab focuses the only item
        let result = r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f1));
        assert_eq!(result, EventResult::Consumed);

        // Second Tab has nowhere to cycle — falls through
        let result = r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f1));
        assert_eq!(result, EventResult::Ignored);
    }

    // --- Focus scope tests ---

    /// A container that declares a focus scope via lifecycle hook.
    struct ScopeContainer;

    impl Component for ScopeContainer {
        type State = ();

        fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &()) {}

        fn initial_state(&self) -> Option<()> {
            Some(())
        }

        fn lifecycle(&self, hooks: &mut Hooks<()>, _state: &()) {
            hooks.use_focus_scope();
        }
    }

    /// A focusable component that requests autofocus on mount.
    struct AutofocusItem;

    impl Component for AutofocusItem {
        type State = String;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let line = ratatui_core::text::Line::raw(state.as_str());
            ratatui_core::widgets::Widget::render(Paragraph::new(line), area, buf);
        }

        fn is_focusable(&self, _state: &Self::State) -> bool {
            true
        }

        fn initial_state(&self) -> Option<String> {
            Some("autofocus-item".to_string())
        }

        fn lifecycle(&self, hooks: &mut Hooks<Self::State>, _state: &Self::State) {
            hooks.use_autofocus();
        }
    }

    #[test]
    fn tab_cycles_within_scope_only() {
        let mut r = Renderer::new(20);
        let f_outside = r.push(FocusableItem);

        // Create a scope container with two focusable children
        let scope = r.push(ScopeContainer);
        r.apply_lifecycle(scope); // sets focus_scope flag
        let f1 = r.append_child(scope, FocusableItem);
        let f2 = r.append_child(scope, FocusableItem);

        // Focus the first item inside the scope
        r.set_focus(f1);

        // Tab should cycle within scope: f1 → f2
        r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f2));

        // Tab again: f2 → f1 (wraps within scope)
        r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f1));

        // f_outside should never be reached
        let _ = f_outside;
    }

    #[test]
    fn tab_in_scope_with_single_focusable_falls_through() {
        let mut r = Renderer::new(20);

        let scope = r.push(ScopeContainer);
        r.apply_lifecycle(scope);
        let f1 = r.append_child(scope, FocusableItem);

        r.set_focus(f1);

        // Only one focusable in scope — Tab should fall through
        // to event handlers (not consumed by cycling)
        let result = r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f1));
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn backtab_cycles_within_scope() {
        let mut r = Renderer::new(20);
        let _f_outside = r.push(FocusableItem);

        let scope = r.push(ScopeContainer);
        r.apply_lifecycle(scope);
        let f1 = r.append_child(scope, FocusableItem);
        let f2 = r.append_child(scope, FocusableItem);
        let f3 = r.append_child(scope, FocusableItem);

        r.set_focus(f1);

        // BackTab from first → wraps to last in scope
        r.handle_event(&backtab_event());
        assert_eq!(r.focus(), Some(f3));

        // BackTab → f2
        r.handle_event(&backtab_event());
        assert_eq!(r.focus(), Some(f2));
    }

    #[test]
    fn removing_scope_restores_previous_focus() {
        let mut r = Renderer::new(20);
        let f_outside = r.push(FocusableItem);
        r.set_focus(f_outside);

        // Build scope with an autofocus child (this saves pre-scope focus)
        let scope = r.push(ScopeContainer);
        let _f_in = r.append_child(scope, AutofocusItem);

        // Simulate mount: apply lifecycle and fire mount effects
        let provided = r.apply_lifecycle(scope);
        let saved = r.push_context(provided);
        let provided_child = r.apply_lifecycle(_f_in);
        r.fire_mount(_f_in);
        let saved2 = r.push_context(provided_child);
        r.pop_context(saved2);
        r.pop_context(saved);

        // Autofocus should have moved focus into the scope
        assert_eq!(r.focus(), Some(_f_in));

        // Remove the scope
        r.remove(scope);

        // Focus should be restored to f_outside
        assert_eq!(r.focus(), Some(f_outside));
    }

    #[test]
    fn nested_scopes_inner_removed_restores_to_outer() {
        let mut r = Renderer::new(20);
        let f_outside = r.push(FocusableItem);
        r.set_focus(f_outside);

        // Outer scope with autofocus child
        let outer_scope = r.push(ScopeContainer);
        let f_outer = r.append_child(outer_scope, AutofocusItem);

        let provided = r.apply_lifecycle(outer_scope);
        let saved = r.push_context(provided);
        let provided_child = r.apply_lifecycle(f_outer);
        r.fire_mount(f_outer);
        let saved2 = r.push_context(provided_child);
        r.pop_context(saved2);

        assert_eq!(r.focus(), Some(f_outer));

        // Inner scope with autofocus child
        let inner_scope = r.append_child(outer_scope, ScopeContainer);
        let f_inner = r.append_child(inner_scope, AutofocusItem);

        let provided_inner_scope = r.apply_lifecycle(inner_scope);
        let saved3 = r.push_context(provided_inner_scope);
        let provided_inner_child = r.apply_lifecycle(f_inner);
        r.fire_mount(f_inner);
        let saved4 = r.push_context(provided_inner_child);
        r.pop_context(saved4);
        r.pop_context(saved3);
        r.pop_context(saved);

        assert_eq!(r.focus(), Some(f_inner));

        // Remove inner scope → focus should return to f_outer
        r.remove(inner_scope);
        assert_eq!(r.focus(), Some(f_outer));

        // Remove outer scope → focus should return to f_outside
        r.remove(outer_scope);
        assert_eq!(r.focus(), Some(f_outside));
    }

    #[test]
    fn scope_removed_with_invalid_saved_focus_falls_back() {
        let mut r = Renderer::new(20);
        let f0 = r.push(FocusableItem);
        let f1 = r.push(FocusableItem);
        r.set_focus(f1);

        // Build scope with autofocus child (saves f1 as pre-scope focus)
        let scope = r.push(ScopeContainer);
        let _f_in = r.append_child(scope, AutofocusItem);

        let provided = r.apply_lifecycle(scope);
        let saved = r.push_context(provided);
        let provided_child = r.apply_lifecycle(_f_in);
        r.fire_mount(_f_in);
        let saved2 = r.push_context(provided_child);
        r.pop_context(saved2);
        r.pop_context(saved);

        assert_eq!(r.focus(), Some(_f_in));

        // Remove the saved focus target (f1) while scope is open
        r.remove(f1);

        // Remove the scope — saved focus is invalid, should fall back
        // to the first focusable in the parent scope (f0)
        r.remove(scope);
        assert_eq!(r.focus(), Some(f0));
    }

    #[test]
    fn scope_removed_with_no_remaining_focusable_clears() {
        let mut r = Renderer::new(20);
        let f_outside = r.push(FocusableItem);
        r.set_focus(f_outside);

        // Build scope with autofocus child
        let scope = r.push(ScopeContainer);
        let _f_in = r.append_child(scope, AutofocusItem);

        let provided = r.apply_lifecycle(scope);
        let saved = r.push_context(provided);
        let provided_child = r.apply_lifecycle(_f_in);
        r.fire_mount(_f_in);
        let saved2 = r.push_context(provided_child);
        r.pop_context(saved2);
        r.pop_context(saved);

        // Remove all outside focusable nodes
        r.remove(f_outside);

        // Remove the scope — nothing to fall back to
        r.remove(scope);
        assert_eq!(r.focus(), None);
    }

    #[test]
    fn programmatic_set_focus_crosses_scope_boundaries() {
        let mut r = Renderer::new(20);
        let f_outside = r.push(FocusableItem);

        let scope = r.push(ScopeContainer);
        r.apply_lifecycle(scope);
        let f_in = r.append_child(scope, FocusableItem);

        // Focus inside scope
        r.set_focus(f_in);
        assert_eq!(r.focus(), Some(f_in));

        // Programmatic set_focus crosses scope boundary
        r.set_focus(f_outside);
        assert_eq!(r.focus(), Some(f_outside));

        // And back in
        r.set_focus(f_in);
        assert_eq!(r.focus(), Some(f_in));
    }

    #[test]
    fn set_focus_into_scope_saves_pre_scope_focus() {
        let mut r = Renderer::new(20);
        let f_outside = r.push(FocusableItem);
        r.set_focus(f_outside);

        let scope = r.push(ScopeContainer);
        r.apply_lifecycle(scope);
        let f_in = r.append_child(scope, FocusableItem);

        // Programmatic set_focus into scope should save pre-scope focus
        r.set_focus(f_in);
        assert_eq!(r.focus(), Some(f_in));

        // Remove scope — should restore to f_outside
        r.remove(scope);
        assert_eq!(r.focus(), Some(f_outside));
    }

    #[test]
    fn tab_into_scope_saves_pre_scope_focus() {
        let mut r = Renderer::new(20);
        let f_outside = r.push(FocusableItem);

        let scope = r.push(ScopeContainer);
        r.apply_lifecycle(scope);
        let f_in = r.append_child(scope, FocusableItem);

        // Start focused outside
        r.set_focus(f_outside);

        // Tab cycles globally (no scope on f_outside) and lands on f_in
        r.handle_event(&tab_event());
        assert_eq!(r.focus(), Some(f_in));

        // Remove scope — should restore to f_outside since set_focus
        // was called internally by cycle_focus
        r.remove(scope);
        assert_eq!(r.focus(), Some(f_outside));
    }

    // --- Capture phase tests ---

    /// A component that intercepts specific keys during the capture phase.
    /// Consumes Ctrl+N during capture; ignores everything else.
    struct CaptureShortcut;

    impl Component for CaptureShortcut {
        type State = Vec<String>;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let text: Vec<Line> = state.iter().map(|s| Line::raw(s.as_str())).collect();
            let para = Paragraph::new(text);
            ratatui_core::widgets::Widget::render(para, area, buf);
        }

        fn handle_event_capture(
            &self,
            event: &crossterm::event::Event,
            state: &mut Tracked<Self::State>,
        ) -> EventResult {
            use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
            if let Event::Key(KeyEvent {
                code: KeyCode::Char('n'),
                kind: KeyEventKind::Press,
                modifiers,
                ..
            }) = event
                && modifiers.contains(KeyModifiers::CONTROL)
            {
                state.push("capture:ctrl-n".to_string());
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }

        fn initial_state(&self) -> Option<Vec<String>> {
            Some(vec![])
        }
    }

    fn ctrl_n_event() -> crossterm::event::Event {
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('n'),
            crossterm::event::KeyModifiers::CONTROL,
        ))
    }

    #[test]
    fn capture_phase_intercepts_before_bubble() {
        let mut r = Renderer::new(10);

        // Parent captures Ctrl+N; child handles all chars via bubble
        let parent = r.push(CaptureShortcut);
        let child = r.append_child(parent, InputCapture);
        r.set_focus(child);

        // Regular key → capture ignores, bubble handles at child
        let result = r.handle_event(&key_event('a'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(&**r.state_mut::<InputCapture>(child), "a");
        assert!(r.state_mut::<CaptureShortcut>(parent).is_empty());

        // Ctrl+N → capture intercepts at parent, child never sees it
        let result = r.handle_event(&ctrl_n_event());
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(&**r.state_mut::<InputCapture>(child), "a"); // unchanged
        assert_eq!(
            &**r.state_mut::<CaptureShortcut>(parent),
            &["capture:ctrl-n".to_string()]
        );
    }

    #[test]
    fn capture_consumed_prevents_bubble() {
        let mut r = Renderer::new(10);

        // Grandparent captures Ctrl+N, parent also handles chars in bubble
        let grandparent = r.push(CaptureShortcut);
        let parent = r.append_child(grandparent, InputCapture);
        let child = r.append_child(parent, TextBlock);
        r.state_mut::<TextBlock>(child).push("child".to_string());
        r.set_focus(child);

        // Ctrl+N captured by grandparent — parent's bubble handler never runs
        let result = r.handle_event(&ctrl_n_event());
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(
            &**r.state_mut::<CaptureShortcut>(grandparent),
            &["capture:ctrl-n".to_string()]
        );
        assert_eq!(&**r.state_mut::<InputCapture>(parent), ""); // never touched
    }

    #[test]
    fn frozen_node_skipped_in_capture() {
        let mut r = Renderer::new(10);

        let parent = r.push(CaptureShortcut);
        let child = r.append_child(parent, TextBlock);
        r.state_mut::<TextBlock>(child).push("child".to_string());
        r.set_focus(child);

        // Freeze parent
        let _ = r.render();
        r.freeze(parent);

        // Ctrl+N → parent is frozen so capture skipped, TextBlock ignores in bubble
        let result = r.handle_event(&ctrl_n_event());
        assert_eq!(result, EventResult::Ignored);
        assert!(r.state_mut::<CaptureShortcut>(parent).is_empty());
    }

    #[test]
    fn focused_node_participates_in_capture() {
        // A component that handles events in capture phase, used as the focused node
        struct SelfCapture;

        impl Component for SelfCapture {
            type State = Vec<String>;

            fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &Self::State) {}

            fn is_focusable(&self, _state: &Self::State) -> bool {
                true
            }

            fn handle_event_capture(
                &self,
                event: &crossterm::event::Event,
                state: &mut Tracked<Self::State>,
            ) -> EventResult {
                use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
                if let Event::Key(KeyEvent {
                    code: KeyCode::Char(c),
                    kind: KeyEventKind::Press,
                    ..
                }) = event
                {
                    state.push(format!("capture:{c}"));
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            fn handle_event(
                &self,
                event: &crossterm::event::Event,
                state: &mut Tracked<Self::State>,
            ) -> EventResult {
                use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
                if let Event::Key(KeyEvent {
                    code: KeyCode::Char(c),
                    kind: KeyEventKind::Press,
                    ..
                }) = event
                {
                    state.push(format!("bubble:{c}"));
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            fn initial_state(&self) -> Option<Vec<String>> {
                Some(vec![])
            }
        }

        let mut r = Renderer::new(10);
        let id = r.push(SelfCapture);
        r.set_focus(id);

        r.handle_event(&key_event('x'));

        let state = r.state_mut::<SelfCapture>(id);
        // Capture fires first and consumes — bubble never runs
        assert_eq!(&**state, &["capture:x".to_string()]);
    }

    #[test]
    fn capture_ignores_then_bubble_handles() {
        let mut r = Renderer::new(10);

        // Parent has capture handler but only for Ctrl+N
        let parent = r.push(CaptureShortcut);
        let child = r.append_child(parent, InputCapture);
        r.set_focus(child);

        // Regular char → capture ignores at both, bubble handles at child
        let result = r.handle_event(&key_event('z'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(&**r.state_mut::<InputCapture>(child), "z");
        assert!(r.state_mut::<CaptureShortcut>(parent).is_empty());
    }

    // --- Declarative rebuild tests ---

    use crate::element::{Element, Elements};

    /// A simple element for testing that creates a TextBlock with given lines.
    struct TestTextEl {
        lines: Vec<String>,
    }

    impl TestTextEl {
        fn new(text: &str) -> Self {
            Self {
                lines: vec![text.to_string()],
            }
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
        els.add_element(TestTextEl::new("new1"));
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
        inner.add_element(TestTextEl::new("child1"));
        inner.add_element(TestTextEl::new("child2"));

        let mut els = Elements::new();
        els.add_with_children(VStack, inner);
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
                els.add_element(TestTextEl::new("thinking..."));
            }
            els.add_element(TestTextEl::new("message"));
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
        children.add_element(TestTextEl::new("grouped1"));
        children.add_element(TestTextEl::new("grouped2"));

        let mut els = Elements::new();
        els.add_element(TestTextEl::new("before"));
        els.group(children);
        els.add_element(TestTextEl::new("after"));
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
            fn initial_state(&self) -> Option<String> {
                Some(String::new())
            }
        }

        struct CustomWidgetEl {
            config: String,
        }

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
        els.add_element(CustomWidgetEl {
            config: "custom!".to_string(),
        });
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

        fn initial_state(&self) -> Option<(String, usize)> {
            Some((String::new(), 0))
        }
    }

    struct CounterEl {
        label: String,
    }

    impl CounterEl {
        fn new(label: &str) -> Self {
            Self {
                label: label.to_string(),
            }
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
        els.add_element(CounterEl::new("first"));
        r.rebuild(container, els);

        let children_1 = r.children(container).to_vec();
        assert_eq!(children_1.len(), 1);
        let first_id = children_1[0];
        assert_eq!(r.state_mut::<CounterWidget>(first_id).1, 1); // build_count = 1

        // Second rebuild with same type at same position
        let mut els = Elements::new();
        els.add_element(CounterEl::new("updated"));
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
        els.add_element(CounterEl::new("counter"));
        r.rebuild(container, els);

        // Verify it's a CounterWidget
        let old_id = r.children(container)[0];
        assert_eq!(r.state_mut::<CounterWidget>(old_id).1, 1); // build_count = 1

        // Rebuild with TestTextEl (different type)
        let mut els = Elements::new();
        els.add_element(TestTextEl::new("text"));
        r.rebuild(container, els);

        let new_id = r.children(container)[0];
        // Old node was freed and new one created (slot may be reused).
        // Verify the component is now a TextBlock, not a CounterWidget.
        let state = r.state_mut::<TextBlock>(new_id);
        assert_eq!(state.len(), 1);
        assert_eq!(state[0], "text");
    }

    #[test]
    fn keyed_elements_survive_position_change() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build: [A, B]
        let mut els = Elements::new();
        els.add_element(CounterEl::new("A")).key("a");
        els.add_element(CounterEl::new("B")).key("b");
        r.rebuild(container, els);

        let children_1 = r.children(container).to_vec();
        let id_a = children_1[0];
        let id_b = children_1[1];

        // Rebuild: [B, A] — reversed order
        let mut els = Elements::new();
        els.add_element(CounterEl::new("B")).key("b");
        els.add_element(CounterEl::new("A")).key("a");
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
        els.add_element(CounterEl::new("A")).key("a");
        els.add_element(CounterEl::new("B")).key("b");
        els.add_element(CounterEl::new("C")).key("c");
        r.rebuild(container, els);

        let id_a = r.children(container)[0];
        let id_c = r.children(container)[2];

        // Rebuild: [A, C] — B removed
        let mut els = Elements::new();
        els.add_element(CounterEl::new("A")).key("a");
        els.add_element(CounterEl::new("C")).key("c");
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
        els.add_element(CounterEl::new("alpha")).key("first");
        els.add_element(CounterEl::new("beta")).key("second");
        els.add_element(CounterEl::new("gamma")); // no key
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
        els.add_element(CounterEl::new("item")).key("c");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "c").unwrap();
        // Simulate local state mutation (increment build_count as proxy)
        r.state_mut::<CounterWidget>(id).1 = 42;

        // Rebuild — node should be reused, local state preserved
        let mut els = Elements::new();
        els.add_element(CounterEl::new("updated-item")).key("c");
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
        inner.add_element(CounterEl::new("A")).key("a");
        inner.add_element(CounterEl::new("B")).key("b");
        let mut els = Elements::new();
        els.add_with_children(VStack, inner).key("group");
        r.rebuild(container, els);

        let group_id = r.find_by_key(container, "group").unwrap();
        let id_a = r.find_by_key(group_id, "a").unwrap();
        let id_b = r.find_by_key(group_id, "b").unwrap();

        // Rebuild nested: VStack > [A, C] — B removed, C added
        let mut inner = Elements::new();
        inner.add_element(CounterEl::new("A-updated")).key("a");
        inner.add_element(CounterEl::new("C")).key("c");
        let mut els = Elements::new();
        els.add_with_children(VStack, inner).key("group");
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
        els.add_element(CounterEl::new("something")).key("x");
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
        els.add_element(CounterEl::new("item")).key("x");
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
    fn spinner_build_auto_registers_tick() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(crate::components::spinner::Spinner::new("Loading..."));
        r.rebuild(container, els);

        assert!(r.has_active());
    }

    #[test]
    fn spinner_done_build_no_tick() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(crate::components::spinner::Spinner::new("Done").done("Completed"));
        r.rebuild(container, els);

        assert!(!r.has_active());
    }

    #[test]
    fn spinner_update_to_done_unregisters() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build active spinner
        let mut els = Elements::new();
        els.add(crate::components::spinner::Spinner::new("Loading..."))
            .key("s");
        r.rebuild(container, els);
        assert!(r.has_active());

        // Rebuild as done
        let mut els = Elements::new();
        els.add(crate::components::spinner::Spinner::new("Done").done("Completed"))
            .key("s");
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

    #[derive(Default)]
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

        fn initial_state(&self) -> Option<LifecycleState> {
            Some(LifecycleState {
                log: Vec::new(),
                mount_marker: String::new(),
                unmount_marker: String::new(),
            })
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
            state
                .log
                .retain(|s| s.starts_with("mounted:") || s.starts_with("unmounted:"));
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
        els.add_element(LifecycleEl::new("hello")).key("a");
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
        els.add_element(LifecycleEl::new("v1")).key("a");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "a").unwrap();
        let mount_count = r
            .state_mut::<LifecycleWidget>(id)
            .log
            .iter()
            .filter(|s| s.starts_with("mounted:"))
            .count();
        assert_eq!(mount_count, 1);

        // Second rebuild — reuse, mount should NOT fire again
        let mut els = Elements::new();
        els.add_element(LifecycleEl::new("v2")).key("a");
        r.rebuild(container, els);

        let state = r.state_mut::<LifecycleWidget>(id);
        let mount_count = state
            .log
            .iter()
            .filter(|s| s.starts_with("mounted:"))
            .count();
        assert_eq!(mount_count, 1); // still just 1
    }

    #[test]
    fn unmount_fires_on_tombstone() {
        use std::sync::{Arc, Mutex};

        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add_element(LifecycleEl::new("bye")).key("a");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "a").unwrap();

        // Register an unmount handler that captures to external state
        let log_clone = log.clone();
        r.on_unmount::<LifecycleWidget>(id, move |_state| {
            log_clone.lock().unwrap().push("unmounted".to_string());
        });

        // Rebuild with empty — triggers tombstone and frees the node
        r.rebuild(container, Elements::new());

        // Verify unmount fired via external log
        let entries = log.lock().unwrap();
        assert!(entries.contains(&"unmounted".to_string()));
    }

    #[test]
    fn unmount_parent_fires_before_children() {
        use std::sync::{Arc, Mutex};

        let order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        // Build parent with child
        let parent_id = r.append_child(container, LifecycleWidget);
        r.state_mut::<LifecycleWidget>(parent_id)
            .log
            .push("parent".to_string());
        let child_id = r.append_child(parent_id, LifecycleWidget);
        r.state_mut::<LifecycleWidget>(child_id)
            .log
            .push("child".to_string());

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
        els.add_element(HookedCounterEl {
            label: "multi".into(),
        })
        .key("m");
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
        let constraints = vec![
            WidthConstraint::Fill,
            WidthConstraint::Fill,
            WidthConstraint::Fill,
        ];
        let widths = super::allocate_widths(&constraints, 80);
        // 80 / 3 = 26 remainder 2 → first two get 27, last gets 26
        assert_eq!(widths, vec![27, 27, 26]);
        assert_eq!(widths.iter().sum::<u16>(), 80);
    }

    #[test]
    fn allocate_widths_fixed_plus_fill() {
        let constraints = vec![WidthConstraint::Fixed(2), WidthConstraint::Fill];
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
        row.add_element(TestTextEl::new(">"))
            .width(WidthConstraint::Fixed(2));
        row.add_element(TestTextEl::new("hello"));

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
        row.add_element(TestTextEl::new("left"));
        row.add_element(TestTextEl::new("right"));

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
        els.add_element(TestTextEl::new("above"));

        let mut row = Elements::new();
        row.add_element(TestTextEl::new("$"))
            .width(WidthConstraint::Fixed(2));
        row.add_element(TestTextEl::new("cmd"));
        els.hstack(row);

        els.add_element(TestTextEl::new("below"));
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
        row.add_element(TestTextEl::new(">"))
            .width(WidthConstraint::Fixed(2));
        row.add_element(TestTextEl::new("v1"));
        let mut els = Elements::new();
        els.hstack(row).key("row");
        r.rebuild(container, els);

        let frame1 = r.render();
        assert_eq!(frame1.buffer()[(2, 0)].symbol(), "v"); // "v1" at x=2

        // Rebuild — content changes but layout preserved
        let mut row = Elements::new();
        row.add_element(TestTextEl::new("$"))
            .width(WidthConstraint::Fixed(2));
        row.add_element(TestTextEl::new("v2"));
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
                        buf[(area.x + area.width - 1, area.y + area.height - 1)].set_symbol("+");
                    }
                }
            }
        }

        fn content_inset(&self, state: &Self::State) -> Insets {
            *state
        }

        fn initial_state(&self) -> Option<Insets> {
            Some(Insets::ZERO)
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

    #[derive(Default)]
    struct LabeledRowState {
        prefix: String,
        label: String,
    }

    impl Component for LabeledRow {
        type State = LabeledRowState;

        fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &Self::State) {}

        fn initial_state(&self) -> Option<LabeledRowState> {
            Some(LabeledRowState {
                prefix: String::new(),
                label: String::new(),
            })
        }

        fn view(&self, state: &Self::State, _children: Elements) -> Elements {
            // Ignore children — generate own children
            let mut row = Elements::new();
            row.add_element(TestTextEl::new(&state.prefix))
                .width(WidthConstraint::Fixed(2));
            row.add_element(TestTextEl::new(&state.label));

            let mut els = Elements::new();
            els.hstack(row);
            els
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
        els.add_element(LabeledRowEl::new(">", "hello"));
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
        els.add_element(LabeledRowEl::new(">", "v1")).key("row");
        r.rebuild(container, els);

        let row_id = r.find_by_key(container, "row").unwrap();

        // Rebuild — composite is reused, children regenerated
        let mut els = Elements::new();
        els.add_element(LabeledRowEl::new("$", "v2")).key("row");
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
        inner.add_element(TestTextEl::new("child1"));
        inner.add_element(TestTextEl::new("child2"));

        let mut els = Elements::new();
        els.add_with_children(VStack, inner);
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
        fn initial_state(&self) -> Option<String> {
            Some(String::new())
        }

        fn view(&self, state: &Self::State, children: Elements) -> Elements {
            let mut els = Elements::new();
            // Add banner title
            els.add_element(TestTextEl::new(state));
            // Include slot children
            for _entry in children.into_items() {
                // Re-wrap each entry
                els.add_element(TestTextEl::new("slot"));
            }
            els
        }
    }

    struct BannerEl {
        title: String,
    }

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
        slot.add_element(TestTextEl::new("content"));

        let mut els = Elements::new();
        els.add_element_with_children(
            BannerEl {
                title: "TITLE".into(),
            },
            slot,
        );
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
        els.add_element(TestTextEl::new("above"));
        els.add_element(LabeledRowEl::new(">", "nested"));
        els.add_element(TestTextEl::new("below"));
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

        fn initial_state(&self) -> Option<(String, usize)> {
            Some((String::new(), 0))
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

    struct HookedCounterEl {
        label: String,
    }

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
        els.add_element(HookedCounterEl {
            label: "active".into(),
        });
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
        els.add_element(HookedCounterEl {
            label: "active".into(),
        })
        .key("c");
        r.rebuild(container, els);
        assert!(r.has_active());

        // Update to "stop" — lifecycle re-runs, no interval registered → cleared
        let mut els = Elements::new();
        els.add_element(HookedCounterEl {
            label: "stop".into(),
        })
        .key("c");
        r.rebuild(container, els);
        assert!(!r.has_active());
    }

    #[test]
    fn lifecycle_interval_fires() {
        let mut r = Renderer::new(10);
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add_element(HookedCounterEl { label: "go".into() })
            .key("c");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "c").unwrap();
        assert_eq!(r.state_mut::<HookedCounter>(id).1, 0); // count starts at 0

        std::thread::sleep(Duration::from_millis(5));
        r.tick();

        assert!(r.state_mut::<HookedCounter>(id).1 > 0); // count incremented
    }

    #[test]
    fn lifecycle_with_mount_and_unmount() {
        use std::sync::{Arc, Mutex};

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
            fn initial_state(&self) -> Option<Vec<String>> {
                Some(Vec::new())
            }
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
        els.add_element(MountTrackerEl).key("mt");
        r.rebuild(container, els);

        let id = r.find_by_key(container, "mt").unwrap();
        let state = r.state_mut::<MountTracker>(id);
        assert!(state.contains(&"built".to_string()));
        assert!(state.contains(&"mounted".to_string()));

        // Register an external unmount observer before tombstoning
        let unmount_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log_clone = unmount_log.clone();
        r.on_unmount::<MountTracker>(id, move |_state| {
            log_clone.lock().unwrap().push("unmounted".to_string());
        });

        // Tombstone — unmount fires, node is freed
        r.rebuild(container, Elements::new());
        let entries = unmount_log.lock().unwrap();
        assert!(entries.contains(&"unmounted".to_string()));
    }

    // --- Context tests ---

    /// A component that provides a context value to descendants.
    struct ContextProvider {
        value: String,
    }

    impl Component for ContextProvider {
        type State = ();
        fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &()) {}
        fn lifecycle(&self, hooks: &mut crate::hooks::Hooks<()>, _state: &()) {
            hooks.provide_context(self.value.clone());
        }
    }

    crate::impl_slot_children!(ContextProvider);

    /// A component that consumes a context value.
    struct ContextConsumer;

    #[derive(Default)]
    struct ContextConsumerState {
        received: Option<String>,
    }

    impl Component for ContextConsumer {
        type State = ContextConsumerState;
        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            if let Some(ref val) = state.received {
                let text: Vec<Line> = vec![Line::raw(val.as_str())];
                let para = Paragraph::new(text);
                ratatui_core::widgets::Widget::render(para, area, buf);
            }
        }
        fn lifecycle(&self, hooks: &mut crate::hooks::Hooks<Self::State>, _state: &Self::State) {
            hooks.use_context::<String>(|value, state| {
                state.received = value.cloned();
            });
        }
    }

    #[test]
    fn context_provider_to_consumer() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        let mut els = Elements::new();
        let mut children = Elements::new();
        children.add(ContextConsumer).key("consumer");
        els.add_with_children(
            ContextProvider {
                value: "hello from context".into(),
            },
            children,
        )
        .key("provider");
        r.rebuild(container, els);

        // The consumer should have received the context value
        let consumer_id = r.find_by_key(container, "provider").unwrap();
        let inner_consumer = r
            .children(consumer_id)
            .iter()
            .find(|&&id| r.node_key(id) == Some("consumer"))
            .copied()
            .unwrap();
        let state = r.state_mut::<ContextConsumer>(inner_consumer);
        assert_eq!(state.received.as_deref(), Some("hello from context"));
    }

    #[test]
    fn context_absent_passes_none() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // Consumer without any provider above it
        let mut els = Elements::new();
        els.add(ContextConsumer).key("consumer");
        r.rebuild(container, els);

        let consumer_id = r.find_by_key(container, "consumer").unwrap();
        let state = r.state_mut::<ContextConsumer>(consumer_id);
        assert_eq!(state.received, None);
    }

    #[test]
    fn context_shadowing() {
        // Inner provider shadows outer provider of the same type
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        let mut inner_children = Elements::new();
        inner_children.add(ContextConsumer).key("inner-consumer");

        let mut inner_provider_els = Elements::new();
        inner_provider_els
            .add_with_children(
                ContextProvider {
                    value: "inner".into(),
                },
                inner_children,
            )
            .key("inner-provider");

        let mut outer_children = Elements::new();
        outer_children.splice(inner_provider_els);
        outer_children.add(ContextConsumer).key("outer-consumer");

        let mut els = Elements::new();
        els.add_with_children(
            ContextProvider {
                value: "outer".into(),
            },
            outer_children,
        )
        .key("outer-provider");
        r.rebuild(container, els);

        // Inner consumer should see "inner" (shadowed)
        let outer_provider_id = r.find_by_key(container, "outer-provider").unwrap();
        let inner_provider_id = r.find_by_key(outer_provider_id, "inner-provider").unwrap();
        let inner_consumer_id = r.find_by_key(inner_provider_id, "inner-consumer").unwrap();
        let state = r.state_mut::<ContextConsumer>(inner_consumer_id);
        assert_eq!(state.received.as_deref(), Some("inner"));

        // Outer consumer (sibling of inner provider) should see "outer"
        let outer_consumer_id = r.find_by_key(outer_provider_id, "outer-consumer").unwrap();
        let state = r.state_mut::<ContextConsumer>(outer_consumer_id);
        assert_eq!(state.received.as_deref(), Some("outer"));
    }

    #[test]
    fn root_context_available_to_all() {
        let mut r = Renderer::new(20);
        r.set_root_context("root-value".to_string());
        let container = r.push(VStack);

        let mut els = Elements::new();
        els.add(ContextConsumer).key("consumer");
        r.rebuild(container, els);

        let consumer_id = r.find_by_key(container, "consumer").unwrap();
        let state = r.state_mut::<ContextConsumer>(consumer_id);
        assert_eq!(state.received.as_deref(), Some("root-value"));
    }

    #[test]
    fn context_updates_on_rebuild() {
        let mut r = Renderer::new(20);
        let container = r.push(VStack);

        // First build with "v1"
        let mut els = Elements::new();
        let mut children = Elements::new();
        children.add(ContextConsumer).key("consumer");
        els.add_with_children(ContextProvider { value: "v1".into() }, children)
            .key("provider");
        r.rebuild(container, els);

        let provider_id = r.find_by_key(container, "provider").unwrap();
        let consumer_id = r.find_by_key(provider_id, "consumer").unwrap();
        let state = r.state_mut::<ContextConsumer>(consumer_id);
        assert_eq!(state.received.as_deref(), Some("v1"));

        // Rebuild with "v2" — consumer should see updated value
        let mut els = Elements::new();
        let mut children = Elements::new();
        children.add(ContextConsumer).key("consumer");
        els.add_with_children(ContextProvider { value: "v2".into() }, children)
            .key("provider");
        r.rebuild(container, els);

        let provider_id = r.find_by_key(container, "provider").unwrap();
        let consumer_id = r.find_by_key(provider_id, "consumer").unwrap();
        let state = r.state_mut::<ContextConsumer>(consumer_id);
        assert_eq!(state.received.as_deref(), Some("v2"));
    }

    // --- view() method tests ---

    #[test]
    fn view_component_renders_via_canvas() {
        use crate::components::canvas::Canvas;

        #[derive(Default)]
        struct MyComp;

        impl Component for MyComp {
            type State = ();

            fn view(&self, _state: &(), _children: Elements) -> Elements {
                let mut els = Elements::new();
                els.add(Canvas::new(|area: Rect, buf: &mut Buffer| {
                    let para = Paragraph::new("hello view");
                    ratatui_core::widgets::Widget::render(para, area, buf);
                }));
                els
            }
        }

        let mut r = Renderer::new(20);
        let root = r.root();
        let mut els = Elements::new();
        els.add(MyComp);
        r.rebuild(root, els);

        let frame = r.render();
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "h"); // "hello view"
        assert_eq!(frame.buffer()[(5, 0)].symbol(), " ");
        assert_eq!(frame.buffer()[(6, 0)].symbol(), "v"); // "view"
    }

    #[test]
    fn view_component_with_slot_children() {
        use crate::components::view::View;

        #[derive(Default)]
        struct Card {
            title: String,
        }

        impl Component for Card {
            type State = ();

            fn view(&self, _state: &(), children: Elements) -> Elements {
                let mut els = Elements::new();
                els.add_with_children(
                    View {
                        border: Some(ratatui_widgets::borders::BorderType::Plain),
                        title: Some(self.title.clone()),
                        ..View::default()
                    },
                    children,
                );
                els
            }
        }

        crate::impl_slot_children!(Card);

        let mut r = Renderer::new(30);
        let root = r.root();

        let mut els = Elements::new();
        let mut card_children = Elements::new();
        card_children.add(crate::components::text::TextBlock::new().unstyled("inside"));
        els.add_with_children(
            Card {
                title: "My Card".into(),
            },
            card_children,
        );
        r.rebuild(root, els);

        let frame = r.render();
        // Top-left corner = border
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "┌");
        // Title in top border
        assert_eq!(frame.buffer()[(1, 0)].symbol(), "M"); // "My Card"
        // Child content inside border (row 1, col 1)
        assert_eq!(frame.buffer()[(1, 1)].symbol(), "i"); // "inside"
    }

    #[test]
    fn view_component_renders_view_children() {
        // After the uses_view() removal, view() components still have their
        // render() and content_inset() called normally. This test verifies
        // that a component using view() renders its view children correctly.
        #[derive(Default)]
        struct Transparent;

        impl Component for Transparent {
            type State = ();

            fn view(&self, _state: &(), _children: Elements) -> Elements {
                let mut els = Elements::new();
                els.add(crate::components::text::TextBlock::new().unstyled("from view"));
                els
            }
        }

        let mut r = Renderer::new(30);
        let root = r.root();
        let mut els = Elements::new();
        els.add(Transparent);
        r.rebuild(root, els);

        let frame = r.render();
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "f"); // "from view"
    }

    #[test]
    fn view_component_reconciles_on_rebuild() {
        use crate::components::canvas::Canvas;

        #[derive(Default)]
        struct Counter {
            label: String,
        }

        impl Component for Counter {
            type State = ();

            fn view(&self, _state: &(), _children: Elements) -> Elements {
                let label = self.label.clone();
                let mut els = Elements::new();
                els.add(Canvas::new(move |area: Rect, buf: &mut Buffer| {
                    let para = Paragraph::new(label.as_str());
                    ratatui_core::widgets::Widget::render(para, area, buf);
                }));
                els
            }
        }

        let mut r = Renderer::new(20);
        let root = r.root();

        let mut els = Elements::new();
        els.add(Counter {
            label: "count: 0".into(),
        })
        .key("counter");
        r.rebuild(root, els);

        let frame = r.render();
        assert_eq!(frame.buffer()[(7, 0)].symbol(), "0"); // "count: 0"

        // Rebuild with new props
        let mut els = Elements::new();
        els.add(Counter {
            label: "count: 1".into(),
        })
        .key("counter");
        r.rebuild(root, els);

        let frame = r.render();
        assert_eq!(frame.buffer()[(7, 0)].symbol(), "1"); // "count: 1"
    }

    #[test]
    fn view_component_empty_elements_produces_zero_height() {
        #[derive(Default)]
        struct Empty;

        impl Component for Empty {
            type State = ();

            fn view(&self, _state: &(), _children: Elements) -> Elements {
                Elements::new()
            }
        }

        let mut r = Renderer::new(20);
        let root = r.root();
        let mut els = Elements::new();
        els.add(Empty);
        r.rebuild(root, els);

        let frame = r.render();
        assert_eq!(frame.buffer().area.height, 0);
    }

    #[test]
    fn legacy_components_still_work_with_view_on_trait() {
        // Components that don't implement uses_view/view work exactly as before.
        // Uses append_child (imperative API) — the legacy path.
        #[derive(Default)]
        struct OldSchool;

        impl Component for OldSchool {
            type State = ();

            fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
                let para = Paragraph::new("old school");
                ratatui_core::widgets::Widget::render(para, area, buf);
            }
        }

        let mut r = Renderer::new(20);
        let root = r.root();
        r.append_child(root, OldSchool);

        let frame = r.render();
        assert_eq!(frame.buffer()[(0, 0)].symbol(), "o"); // "old school"
    }
}
