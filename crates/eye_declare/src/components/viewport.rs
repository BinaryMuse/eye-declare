//! Fixed-height viewport into a larger text buffer.
//!
//! [`Viewport`] renders a fixed-height window into a list of text lines,
//! automatically showing the most recent lines (tail behavior). This is
//! designed for displaying streaming command output, log tails, and
//! similar content where the latest output matters most.
//!
//! # Examples
//!
//! ```ignore
//! use eye_declare::{element, Viewport, BorderType};
//!
//! element! {
//!     Viewport(
//!         lines: output_lines.clone(),
//!         height: 10,
//!         border: BorderType::Plain,
//!         title: "Command output".into(),
//!     )
//! }
//! ```

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;
use ratatui_core::text::Line;
use ratatui_core::widgets::Widget;
use ratatui_widgets::block::{Block, Padding};
use ratatui_widgets::borders::{BorderType, Borders};

use crate::component::Component;
use crate::insets::Insets;

/// A fixed-height viewport into a list of text lines.
///
/// Shows the last `height` lines from the provided `lines` vector.
/// If fewer lines exist than the viewport height, renders from the top.
/// Optional border and title.
#[derive(typed_builder::TypedBuilder)]
pub struct Viewport {
    /// Lines of text to display.
    #[builder(setter(into))]
    pub lines: Vec<String>,

    /// Visible height in rows (excluding border).
    pub height: u16,

    /// Optional border type.
    #[builder(default, setter(into))]
    pub border: Option<BorderType>,

    /// Optional title shown in the top border.
    #[builder(default, setter(into))]
    pub title: Option<String>,

    /// Style applied to text content.
    #[builder(default, setter(into))]
    pub style: Style,

    /// Style applied to the border.
    #[builder(default, setter(into))]
    pub border_style: Style,
}

impl Viewport {
    /// Compute the border insets (chrome) for this viewport.
    fn border_insets(&self) -> Insets {
        let has_border = self.border.is_some();
        let b: u16 = if has_border { 1 } else { 0 };
        Insets {
            top: b,
            right: b,
            bottom: b,
            left: b,
        }
    }

    /// Build the ratatui Block for border rendering.
    fn build_block(&self) -> Block<'static> {
        let mut block = Block::default();

        if let Some(border_type) = self.border {
            let borders = Borders::ALL;
            block = block
                .border_type(border_type)
                .border_style(self.border_style)
                .borders(borders);
        }

        if let Some(ref title) = self.title {
            block = block.title_top(Line::from(format!(" {title} ")));
        }

        block.padding(Padding::ZERO)
    }
}

impl Component for Viewport {
    type State = ();

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        // Render the border/chrome
        let block = self.build_block();
        block.render(area, buf);

        // Compute inner area
        let insets = self.border_insets();
        let inner = Rect::new(
            area.x.saturating_add(insets.left),
            area.y.saturating_add(insets.top),
            area.width.saturating_sub(insets.horizontal()),
            area.height.saturating_sub(insets.vertical()),
        );

        // Nothing to render if inner area is empty
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // Determine which lines to show (tail behavior)
        let visible_count = self.height as usize;
        let start = if self.lines.len() > visible_count {
            self.lines.len() - visible_count
        } else {
            0
        };
        let visible_lines = &self.lines[start..];

        // Render visible lines into the inner area
        for (i, line_text) in visible_lines.iter().enumerate() {
            let row = inner.y + i as u16;
            if row >= inner.y + inner.height {
                break;
            }

            // Truncate to fit the viewport width
            let text = if line_text.len() > inner.width as usize {
                &line_text[..inner.width as usize]
            } else {
                line_text.as_str()
            };

            buf.set_string(inner.x, row, text, self.style);
        }
    }

    fn desired_height(&self, _width: u16, _state: &()) -> Option<u16> {
        let insets = self.border_insets();
        Some(self.height + insets.vertical())
    }

    fn content_inset(&self, _state: &()) -> Insets {
        self.border_insets()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::style::Color;

    #[test]
    fn viewport_renders_last_n_lines() {
        let viewport = Viewport::builder()
            .lines(vec![
                "line 1".into(),
                "line 2".into(),
                "line 3".into(),
                "line 4".into(),
                "line 5".into(),
            ])
            .height(3)
            .build();

        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        viewport.render(area, &mut buf, &());

        // Should show last 3 lines: "line 3", "line 4", "line 5"
        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), "l"); // "line 3"
        assert_eq!(buf.cell((5, 0)).unwrap().symbol(), "3");
        assert_eq!(buf.cell((5, 1)).unwrap().symbol(), "4");
        assert_eq!(buf.cell((5, 2)).unwrap().symbol(), "5");
    }

    #[test]
    fn viewport_fewer_lines_than_height() {
        let viewport = Viewport::builder()
            .lines(vec!["only one".into()])
            .height(5)
            .build();

        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        viewport.render(area, &mut buf, &());

        // Line should be at the top
        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), "o");
        assert_eq!(buf.cell((0, 1)).unwrap().symbol(), " ");
    }

    #[test]
    fn viewport_with_border() {
        let viewport = Viewport::builder()
            .lines(vec!["hello".into()])
            .height(3)
            .border(BorderType::Plain)
            .build();

        let area = Rect::new(0, 0, 20, 5); // 3 content + 2 border
        let mut buf = Buffer::empty(area);
        viewport.render(area, &mut buf, &());

        // Border at row 0
        let top_left = buf.cell((0, 0)).unwrap().symbol();
        assert!(top_left == "┌" || top_left == "+" || top_left == " ");
        // Content starts at row 1
        assert_eq!(buf.cell((1, 1)).unwrap().symbol(), "h");
    }

    #[test]
    fn viewport_desired_height_includes_border() {
        let viewport = Viewport::builder()
            .lines(vec![])
            .height(10)
            .border(BorderType::Plain)
            .build();

        // 10 content + 2 border (top + bottom) = 12
        assert_eq!(viewport.desired_height(80, &()), Some(12));
    }

    #[test]
    fn viewport_truncates_long_lines() {
        let viewport = Viewport::builder()
            .lines(vec![
                "this is a very long line that should be truncated".into()
            ])
            .height(1)
            .build();

        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        viewport.render(area, &mut buf, &());

        // Should show first 10 chars only
        assert_eq!(buf.cell((9, 0)).unwrap().symbol(), " ");
        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), "t"); // "this is a "
    }

    #[test]
    fn viewport_empty_lines() {
        let viewport = Viewport::builder().lines(vec![]).height(5).build();

        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        viewport.render(area, &mut buf, &());

        // All cells should be empty
        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), " ");
    }

    #[test]
    fn viewport_applies_style() {
        let viewport = Viewport::builder()
            .lines(vec!["styled".into()])
            .height(1)
            .style(Style::default().fg(Color::Red))
            .build();

        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        viewport.render(area, &mut buf, &());

        assert_eq!(buf.cell((0, 0)).unwrap().fg, Color::Red);
    }
}
