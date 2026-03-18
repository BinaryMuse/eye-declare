use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
};

use crate::component::{Component, EventResult, Tracked, VStack};
use crate::frame::Frame;
use crate::node::{Node, NodeId};

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
        Self { nodes, root, width, focused: None, cursor_hint: None }
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

        // Collect all descendants to mark as removed
        let mut to_remove = vec![id];
        let mut i = 0;
        while i < to_remove.len() {
            let node_id = to_remove[i];
            let children = self.nodes[node_id.0].children.clone();
            to_remove.extend(children);
            i += 1;
        }

        // Mark removed nodes: clear their children and parent, set frozen
        // We can't remove from the Vec without invalidating NodeIds,
        // so we "tombstone" them by clearing children and setting height to 0.
        for node_id in to_remove {
            let node = &mut self.nodes[node_id.0];
            node.children.clear();
            node.parent = None;
            node.frozen = true;
            node.last_height = Some(0);
            node.cached_buffer = None;
        }
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
            // Container: height = sum of children
            node.children
                .iter()
                .map(|&child| self.measure_height(child, width))
                .sum()
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
            // Render the container's own component first (background/border)
            let state = self.nodes[id.0].state.inner_as_any();
            self.nodes[id.0].component.render_erased(area, buffer, state);

            // Layout and render children vertically
            let children: Vec<NodeId> = self.nodes[id.0].children.clone();
            let mut y_offset = area.y;
            for child_id in children {
                let child_height = self.measure_height(child_id, area.width);
                if child_height == 0 {
                    continue;
                }
                let child_area = Rect::new(area.x, y_offset, area.width, child_height);
                self.render_node(child_id, child_area, buffer);
                y_offset = y_offset.saturating_add(child_height);
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
}
