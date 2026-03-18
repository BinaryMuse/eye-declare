use crate::component::{Component, Tracked};
use crate::escape::CursorState;
use crate::frame::Frame;
use crate::node::NodeId;
use crate::renderer::Renderer;

/// Manages a growing inline region in the terminal.
///
/// Content grows downward as components are added or their content expands.
/// Old content scrolls into the terminal's native scrollback naturally.
/// Output is produced as `Vec<u8>` escape sequences ready to write.
pub struct InlineRenderer {
    renderer: Renderer,
    cursor: CursorState,
    prev_frame: Option<Frame>,
    /// Total rows we've "claimed" in the terminal so far.
    emitted_rows: u16,
}

impl InlineRenderer {
    /// Create a new inline renderer at the given terminal width.
    pub fn new(width: u16) -> Self {
        Self {
            renderer: Renderer::new(width),
            cursor: CursorState::new(),
            prev_frame: None,
            emitted_rows: 0,
        }
    }

    /// The root node's ID.
    pub fn root(&self) -> NodeId {
        self.renderer.root()
    }

    /// Add a component as a child of the given parent. Returns its NodeId.
    pub fn append_child<C: Component>(&mut self, parent: NodeId, component: C) -> NodeId {
        self.renderer.append_child(parent, component)
    }

    /// Shorthand: add a component as a child of the root. Returns its NodeId.
    pub fn push<C: Component>(&mut self, component: C) -> NodeId {
        self.renderer.push(component)
    }

    /// Access a component's tracked state for mutation.
    pub fn state_mut<C: Component>(&mut self, id: NodeId) -> &mut Tracked<C::State> {
        self.renderer.state_mut::<C>(id)
    }

    /// Freeze a component (skip future re-renders).
    pub fn freeze(&mut self, id: NodeId) {
        self.renderer.freeze(id)
    }

    /// Remove a node and all its descendants.
    pub fn remove(&mut self, id: NodeId) {
        self.renderer.remove(id)
    }

    /// List the children of a node.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.renderer.children(id)
    }

    /// Set the rendering width.
    pub fn set_width(&mut self, width: u16) {
        self.renderer.set_width(width);
    }

    /// Render the current state and return bytes to write to the terminal.
    ///
    /// Handles height growth: if the frame is taller than before, emits
    /// newlines to claim new terminal rows before writing the diff.
    /// Returns an empty Vec if nothing changed.
    pub fn render(&mut self) -> Vec<u8> {
        let new_frame = self.renderer.render();
        let new_height = new_frame.area().height;

        // First render
        if self.prev_frame.is_none() {
            if new_height == 0 {
                self.prev_frame = Some(new_frame);
                return Vec::new();
            }

            // For the first render, we need to claim space and write everything.
            // Create an empty "previous" frame so diff produces all cells.
            let empty = Frame::new(ratatui_core::buffer::Buffer::empty(
                ratatui_core::layout::Rect::new(0, 0, self.renderer.width(), 0),
            ));
            let diff = new_frame.diff(&empty);

            let mut output = Vec::new();

            // Emit newlines to claim rows (minus 1 because the cursor
            // is already on the first row)
            if new_height > 0 {
                for _ in 0..new_height.saturating_sub(1) {
                    output.push(b'\n');
                }
                self.emitted_rows = new_height;
            }

            // The cursor is now at the last row of our claimed space.
            // Our content starts at (cursor_row - new_height + 1).
            // Set cursor position so escape generation knows where we are.
            self.cursor.row = new_height.saturating_sub(1);
            self.cursor.col = 0;

            let escape_bytes = diff.to_escape_sequences(&mut self.cursor);
            output.extend_from_slice(&escape_bytes);

            self.prev_frame = Some(new_frame);
            return output;
        }

        // Subsequent renders
        let prev = self.prev_frame.as_ref().unwrap();
        let diff = new_frame.diff(prev);

        if diff.is_empty() && !diff.grew() {
            // Nothing changed
            self.prev_frame = Some(new_frame);
            return Vec::new();
        }

        let mut output = Vec::new();

        // If the frame grew, we need to claim more terminal rows
        let growth = diff.growth();
        if growth > 0 {
            // Move cursor to the bottom of our current region first
            // (it might be somewhere in the middle from the last write)
            let current_bottom = self.emitted_rows.saturating_sub(1);
            if self.cursor.row < current_bottom {
                let down = current_bottom - self.cursor.row;
                output.extend_from_slice(format!("\x1b[{}B", down).as_bytes());
            }
            self.cursor.row = current_bottom;
            self.cursor.col = 0;

            // Emit newlines to claim new rows
            for _ in 0..growth {
                output.push(b'\n');
            }
            self.emitted_rows += growth;
            self.cursor.row += growth;
        }

        let escape_bytes = diff.to_escape_sequences(&mut self.cursor);
        output.extend_from_slice(&escape_bytes);

        self.prev_frame = Some(new_frame);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use ratatui_core::{buffer::Buffer, layout::Rect};
    use ratatui_widgets::paragraph::Paragraph;

    struct TextBlock;

    impl Component for TextBlock {
        type State = Vec<String>;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let lines: Vec<ratatui_core::text::Line> =
                state.iter().map(|s| ratatui_core::text::Line::raw(s.as_str())).collect();
            let para = Paragraph::new(lines);
            ratatui_core::widgets::Widget::render(para, area, buf);
        }

        fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
            state.len() as u16
        }

        fn initial_state(&self) -> Vec<String> {
            vec![]
        }
    }

    #[test]
    fn first_render_empty_produces_nothing() {
        let mut ir = InlineRenderer::new(10);
        let _id = ir.push(TextBlock);
        let output = ir.render();
        assert!(output.is_empty());
    }

    #[test]
    fn first_render_with_content_produces_output() {
        let mut ir = InlineRenderer::new(10);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id).push("hello".to_string());

        let output = ir.render();
        assert!(!output.is_empty());

        let s = String::from_utf8_lossy(&output);
        // Should contain DEC 2026 sync sequences
        assert!(s.contains("\x1b[?2026h"));
        assert!(s.contains("\x1b[?2026l"));
        // Should contain the text
        assert!(s.contains("hello"));
    }

    #[test]
    fn no_change_produces_empty_output() {
        let mut ir = InlineRenderer::new(10);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id).push("hello".to_string());

        let _first = ir.render();
        let second = ir.render();
        assert!(second.is_empty());
    }

    #[test]
    fn growing_content_emits_newlines() {
        let mut ir = InlineRenderer::new(10);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id).push("line1".to_string());

        let _first = ir.render();

        // Add a second line
        ir.state_mut::<TextBlock>(id).push("line2".to_string());
        let second = ir.render();
        let s = String::from_utf8_lossy(&second);

        // Should contain a newline for growth
        assert!(s.contains('\n'));
        // Should contain the new line text
        assert!(s.contains("line2"));
    }
}
