//! Unified layout container with optional borders, padding, and background.
//!
//! [`View`] consolidates vertical/horizontal layout, borders, padding, and
//! background styling into a single component. It replaces the need to
//! manually combine [`VStack`](crate::VStack)/[`HStack`](crate::HStack),
//! [`Column`](crate::Column), and hand-drawn borders.
//!
//! # Examples
//!
//! ```ignore
//! use eye_declare::{element, View, Direction, BorderType, WidthConstraint};
//!
//! // Simple vertical container (default)
//! element! {
//!     View {
//!         "Line one"
//!         "Line two"
//!     }
//! }
//!
//! // Bordered card with title and padding
//! element! {
//!     View(border: BorderType::Rounded, title: "My Card".into(), padding: 1u16) {
//!         "Card content"
//!     }
//! }
//!
//! // Horizontal layout with fixed-width sidebar
//! element! {
//!     View(direction: Direction::Row) {
//!         View(width: WidthConstraint::Fixed(20), border: BorderType::Plain) {
//!             "Sidebar"
//!         }
//!         View {
//!             "Main content"
//!         }
//!     }
//! }
//! ```

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;
use ratatui_core::text::Line;
use ratatui_core::widgets::Widget;
use ratatui_widgets::block::{Block, Padding};
use ratatui_widgets::borders::{BorderType, Borders};

use crate::cells::Cells;
use crate::component::Component;
use crate::impl_slot_children;
use crate::insets::Insets;
use crate::node::{Layout, WidthConstraint};

/// Layout direction for a [`View`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Children stack top-to-bottom, each receiving the full parent width.
    #[default]
    Column,
    /// Children lay out left-to-right, widths allocated by [`WidthConstraint`].
    Row,
}

/// A unified layout container with optional borders, padding, and background.
///
/// See the [module-level docs](self) for examples.
#[derive(typed_builder::TypedBuilder)]
pub struct View {
    /// Layout direction. Defaults to [`Direction::Column`] (vertical).
    #[builder(default, setter(into))]
    pub direction: Direction,

    /// Border type. `None` means no border (default).
    #[builder(default, setter(into))]
    pub border: Option<BorderType>,

    /// Style applied to the border lines.
    #[builder(default, setter(into))]
    pub border_style: Style,

    /// Title rendered at the top of the View. Most useful with a border.
    #[builder(default, setter(into))]
    pub title: Option<String>,

    /// Title rendered at the bottom of the View. Most useful with a border.
    #[builder(default, setter(into))]
    pub title_bottom: Option<String>,

    /// Style applied to the top title text.
    #[builder(default, setter(into))]
    pub title_style: Style,

    /// Style applied to the bottom title text.
    #[builder(default, setter(into))]
    pub title_bottom_style: Style,

    /// Base padding applied to all sides (default 0). Each side uses this
    /// value unless overridden by a side-specific field (`padding_top`, etc.).
    ///
    /// Accepts bare integer literals in the `element!` macro: `padding: 1`.
    #[builder(default, setter(into))]
    pub padding: Cells,

    /// Padding above content. Overrides `padding` for the top side.
    #[builder(default, setter(into))]
    pub padding_top: Option<Cells>,

    /// Padding below content. Overrides `padding` for the bottom side.
    #[builder(default, setter(into))]
    pub padding_bottom: Option<Cells>,

    /// Padding left of content. Overrides `padding` for the left side.
    #[builder(default, setter(into))]
    pub padding_left: Option<Cells>,

    /// Padding right of content. Overrides `padding` for the right side.
    #[builder(default, setter(into))]
    pub padding_right: Option<Cells>,

    /// Width constraint for this View when inside a [`Direction::Row`] parent.
    #[builder(default, setter(into))]
    pub width: WidthConstraint,

    /// Background/foreground style applied to the entire View area.
    #[builder(default, setter(into))]
    pub style: Style,
}

impl Default for View {
    fn default() -> Self {
        Self {
            direction: Direction::Column,
            border: None,
            border_style: Style::default(),
            title: None,
            title_bottom: None,
            title_style: Style::default(),
            title_bottom_style: Style::default(),
            padding: Cells::ZERO,
            padding_top: None,
            padding_bottom: None,
            padding_left: None,
            padding_right: None,
            width: WidthConstraint::Fill,
            style: Style::default(),
        }
    }
}

impl View {
    /// Compute the effective padding for each side, in raw u16 cells.
    fn effective_padding(&self) -> (u16, u16, u16, u16) {
        let base = self.padding.0;
        (
            self.padding_top.map_or(base, |c| c.0),
            self.padding_right.map_or(base, |c| c.0),
            self.padding_bottom.map_or(base, |c| c.0),
            self.padding_left.map_or(base, |c| c.0),
        )
    }

    /// Build the ratatui Block for rendering.
    fn build_block(&self) -> Block<'_> {
        let mut block = Block::new().style(self.style);

        if let Some(border_type) = self.border {
            block = block
                .borders(Borders::ALL)
                .border_type(border_type)
                .border_style(self.border_style);
        }

        if let Some(ref title) = self.title {
            block = block.title_top(Line::from(title.as_str()).style(self.title_style));
        }

        if let Some(ref title) = self.title_bottom {
            block = block.title_bottom(Line::from(title.as_str()).style(self.title_bottom_style));
        }

        let (pt, pr, pb, pl) = self.effective_padding();
        if pt > 0 || pr > 0 || pb > 0 || pl > 0 {
            block = block.padding(Padding::new(pl, pr, pt, pb));
        }

        block
    }
}

impl Component for View {
    type State = ();

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        self.build_block().render(area, buf);
    }

    fn content_inset(&self, _state: &()) -> Insets {
        let has_border = self.border.is_some();
        let border: u16 = if has_border { 1 } else { 0 };
        let (pt, pr, pb, pl) = self.effective_padding();

        Insets {
            top: border + pt,
            right: border + pr,
            bottom: border + pb,
            left: border + pl,
        }
    }

    fn layout(&self) -> Layout {
        match self.direction {
            Direction::Column => Layout::Vertical,
            Direction::Row => Layout::Horizontal,
        }
    }

    fn width_constraint(&self) -> WidthConstraint {
        self.width
    }
}

impl_slot_children!(View);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_view_is_vertical_no_border() {
        let v = View::default();
        assert_eq!(v.direction, Direction::Column);
        assert!(v.border.is_none());
        assert_eq!(v.padding, Cells::ZERO);
        assert_eq!(v.layout(), Layout::Vertical);
        assert_eq!(v.content_inset(&()), Insets::ZERO);
    }

    #[test]
    fn row_direction_maps_to_horizontal_layout() {
        let v = View {
            direction: Direction::Row,
            ..View::default()
        };
        assert_eq!(v.layout(), Layout::Horizontal);
    }

    #[test]
    fn border_adds_one_cell_inset_per_side() {
        let v = View {
            border: Some(BorderType::Plain),
            ..View::default()
        };
        let insets = v.content_inset(&());
        assert_eq!(insets, Insets::all(1));
    }

    #[test]
    fn border_plus_padding() {
        let v = View {
            border: Some(BorderType::Rounded),
            padding: Cells(2),
            ..View::default()
        };
        let insets = v.content_inset(&());
        // 1 (border) + 2 (padding) = 3 on each side
        assert_eq!(insets, Insets::all(3));
    }

    #[test]
    fn padding_without_border() {
        let v = View {
            padding: Cells(1),
            ..View::default()
        };
        let insets = v.content_inset(&());
        assert_eq!(insets, Insets::all(1));
    }

    #[test]
    fn side_specific_padding_overrides_general() {
        let v = View {
            padding: Cells(1),
            padding_left: Some(Cells(3)),
            padding_top: Some(Cells(0)),
            ..View::default()
        };
        let insets = v.content_inset(&());
        assert_eq!(
            insets,
            Insets {
                top: 0,
                right: 1,
                bottom: 1,
                left: 3,
            }
        );
    }

    #[test]
    fn side_specific_padding_with_border() {
        let v = View {
            border: Some(BorderType::Plain),
            padding: Cells(1),
            padding_left: Some(Cells(2)),
            ..View::default()
        };
        let insets = v.content_inset(&());
        assert_eq!(
            insets,
            Insets {
                top: 2,    // 1 border + 1 padding
                right: 2,  // 1 border + 1 padding
                bottom: 2, // 1 border + 1 padding
                left: 3,   // 1 border + 2 padding_left
            }
        );
    }

    #[test]
    fn width_constraint_passthrough() {
        let v = View {
            width: WidthConstraint::Fixed(20),
            ..View::default()
        };
        assert_eq!(v.width_constraint(), WidthConstraint::Fixed(20));
    }

    #[test]
    fn render_plain_border() {
        let v = View {
            border: Some(BorderType::Plain),
            ..View::default()
        };
        let area = Rect::new(0, 0, 10, 5);
        let mut buf = Buffer::empty(area);
        v.render(area, &mut buf, &());

        // Top-left corner should be the plain border character
        let tl = buf.cell((0, 0)).unwrap();
        assert_eq!(tl.symbol(), "┌");

        // Top-right corner
        let tr = buf.cell((9, 0)).unwrap();
        assert_eq!(tr.symbol(), "┐");

        // Bottom-left corner
        let bl = buf.cell((0, 4)).unwrap();
        assert_eq!(bl.symbol(), "└");

        // Bottom-right corner
        let br = buf.cell((9, 4)).unwrap();
        assert_eq!(br.symbol(), "┘");
    }

    #[test]
    fn render_with_title() {
        let v = View {
            border: Some(BorderType::Plain),
            title: Some("Test".into()),
            ..View::default()
        };
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        v.render(area, &mut buf, &());

        // Title should appear in top border
        let t = buf.cell((1, 0)).unwrap();
        assert_eq!(t.symbol(), "T");
    }

    #[test]
    fn render_with_title_bottom() {
        let v = View {
            border: Some(BorderType::Plain),
            title_bottom: Some("Bottom".into()),
            ..View::default()
        };
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        v.render(area, &mut buf, &());

        // Title should appear in bottom border row
        let b = buf.cell((1, 4)).unwrap();
        assert_eq!(b.symbol(), "B");
    }

    #[test]
    fn render_no_border_produces_empty_buffer() {
        let v = View::default();
        let area = Rect::new(0, 0, 10, 5);
        let mut buf = Buffer::empty(area);
        v.render(area, &mut buf, &());

        // All cells should be default (space)
        for y in 0..5 {
            for x in 0..10 {
                assert_eq!(buf.cell((x, y)).unwrap().symbol(), " ");
            }
        }
    }
}
