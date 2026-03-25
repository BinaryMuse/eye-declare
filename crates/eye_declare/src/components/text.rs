use ratatui_core::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};

use crate::children::{AddTo, ChildCollector, DataHandle};
use crate::component::Component;
use crate::wrap;

// ---------------------------------------------------------------------------
// Span — a segment of styled text
// ---------------------------------------------------------------------------

/// A segment of text with a single style.
///
/// `Span` is a data child of [`Line`] — it does not implement
/// [`Component`]. Use it inside `Line` blocks
/// in the `element!` macro to build multi-styled lines:
///
/// ```ignore
/// Line {
///     Span(text: "hello ".into(), style: Style::default().fg(Color::Green))
///     Span(text: "world".into())
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Span {
    /// The text content of this span.
    pub text: String,
    /// The style applied to this span's text.
    pub style: Style,
}

impl Default for Span {
    fn default() -> Self {
        Self {
            text: String::new(),
            style: Style::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Line — a line of text composed of Spans
// ---------------------------------------------------------------------------

/// A line of text composed of one or more [`Span`]s.
///
/// `Line` is a data child of [`TextBlock`] — it does not implement
/// [`Component`]. Use it inside `TextBlock` blocks
/// in the `element!` macro for multi-styled lines:
///
/// ```ignore
/// TextBlock {
///     Line {
///         Span(text: "Name: ".into(), style: Style::default().add_modifier(Modifier::BOLD))
///         Span(text: name.clone(), style: Style::default().fg(Color::Green))
///     }
///     Line {
///         Span(text: "plain text".into())
///     }
/// }
/// ```
#[derive(Clone, Debug, Default)]
pub struct Line {
    /// The spans that compose this line.
    pub spans: Vec<Span>,
}

// ---------------------------------------------------------------------------
// Collectors for Line and TextBlock
// ---------------------------------------------------------------------------

/// Collector for [`Span`] children inside a [`Line`].
#[derive(Default)]
pub struct LineChildren {
    pub(crate) spans: Vec<Span>,
}

impl AddTo<LineChildren> for Span {
    type Handle<'a> = DataHandle;

    fn add_to(self, collector: &mut LineChildren) -> DataHandle {
        collector.spans.push(self);
        DataHandle
    }
}

impl ChildCollector for Line {
    type Collector = LineChildren;
    type Output = Line;

    fn finish(mut self, collector: LineChildren) -> Line {
        self.spans = collector.spans;
        self
    }
}

/// Collector for [`Line`] children inside a [`TextBlock`].
#[derive(Default)]
pub struct TextBlockChildren {
    pub(crate) lines: Vec<Line>,
}

impl AddTo<TextBlockChildren> for Line {
    type Handle<'a> = DataHandle;

    fn add_to(self, collector: &mut TextBlockChildren) -> DataHandle {
        collector.lines.push(self);
        DataHandle
    }
}

impl ChildCollector for TextBlock {
    type Collector = TextBlockChildren;
    type Output = TextBlock;

    fn finish(mut self, collector: TextBlockChildren) -> TextBlock {
        self.lines = collector.lines;
        self
    }
}

// ---------------------------------------------------------------------------
// TextBlock
// ---------------------------------------------------------------------------

/// Styled text with display-time word wrapping.
///
/// `TextBlock` is the primary text component. It stores logical lines
/// of styled text as props and computes word wrapping at render time,
/// so content reflows automatically when the terminal is resized.
///
/// # Builder API
///
/// ```ignore
/// TextBlock::new()
///     .line("error: something failed", Style::default().fg(Color::Red))
///     .unstyled("see logs for details")
/// ```
///
/// # `element!` macro with data children
///
/// For multi-styled lines, use [`Line`] and [`Span`] as data children:
///
/// ```ignore
/// element! {
///     TextBlock {
///         Line {
///             Span(text: "Name: ".into(), style: Style::default().add_modifier(Modifier::BOLD))
///             Span(text: name.clone(), style: Style::default().fg(Color::Green))
///         }
///         Line {
///             Span(text: format!("Status: {status}"))
///         }
///     }
/// }
/// ```
///
/// # String literal shorthand
///
/// In the `element!` macro, bare string literals are automatically
/// wrapped in a `TextBlock`:
///
/// ```ignore
/// element! {
///     "This becomes a TextBlock automatically"
/// }
/// ```
pub struct TextBlock {
    /// The logical lines of styled text. Each [`Line`] contains one or more
    /// [`Span`]s. Word wrapping is computed at render time.
    pub lines: Vec<Line>,
}

impl TextBlock {
    /// Create an empty text block. Use [`.line()`](TextBlock::line) or
    /// [`.unstyled()`](TextBlock::unstyled) to add content.
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Add a styled line (single span).
    pub fn line(mut self, text: impl Into<String>, style: Style) -> Self {
        self.lines.push(Line {
            spans: vec![Span {
                text: text.into(),
                style,
            }],
        });
        self
    }

    /// Add an unstyled line (default style, single span).
    pub fn unstyled(mut self, text: impl Into<String>) -> Self {
        self.lines.push(Line {
            spans: vec![Span {
                text: text.into(),
                style: Style::default(),
            }],
        });
        self
    }

    fn to_text(&self) -> ratatui_core::text::Text<'_> {
        let lines: Vec<ratatui_core::text::Line> = self
            .lines
            .iter()
            .map(|line| {
                let spans: Vec<ratatui_core::text::Span> = line
                    .spans
                    .iter()
                    .map(|span| ratatui_core::text::Span::styled(span.text.as_str(), span.style))
                    .collect();
                ratatui_core::text::Line::from(spans)
            })
            .collect();
        ratatui_core::text::Text::from(lines)
    }
}

impl Default for TextBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for TextBlock {
    type State = ();

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &Self::State) {
        if self.lines.is_empty() {
            return;
        }
        let text = self.to_text();
        wrap::wrapping_paragraph(text).render(area, buf);
    }

    fn desired_height(&self, width: u16, _state: &Self::State) -> u16 {
        if self.lines.is_empty() || width == 0 {
            return 0;
        }
        let text = self.to_text();
        wrap::wrapped_line_count(&text, width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::style::Color;

    #[test]
    fn empty_text_block_height_zero() {
        let tb = TextBlock::new();
        assert_eq!(tb.desired_height(80, &()), 0);
    }

    #[test]
    fn single_short_line() {
        let tb = TextBlock::new().unstyled("hello world");
        assert_eq!(tb.desired_height(80, &()), 1);
    }

    #[test]
    fn wraps_at_narrow_width() {
        let tb = TextBlock::new().unstyled("hello world this is a long line that should wrap");
        // At width 20, this ~47 char line should wrap to 3 lines
        let height = tb.desired_height(20, &());
        assert!(height >= 3, "expected >= 3, got {}", height);
    }

    #[test]
    fn multiple_lines_with_wrapping() {
        let tb = TextBlock::new()
            .unstyled("short")
            .unstyled("this is a longer line that will wrap at narrow widths");
        // At width 20: "short" = 1 line, long line = 3+ lines
        let height = tb.desired_height(20, &());
        assert!(height >= 4, "expected >= 4, got {}", height);
    }

    #[test]
    fn styled_text_wraps_correctly() {
        let tb = TextBlock::new().line(
            "important text that is fairly long",
            Style::default().fg(Color::Red),
        );
        let height_wide = tb.desired_height(80, &());
        let height_narrow = tb.desired_height(15, &());
        assert_eq!(height_wide, 1);
        assert!(
            height_narrow >= 3,
            "expected >= 3 at width 15, got {}",
            height_narrow
        );
    }

    #[test]
    fn renders_into_buffer() {
        let tb = TextBlock::new().unstyled("hello");

        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        tb.render(area, &mut buf, &());

        assert_eq!(buf[(0, 0)].symbol(), "h");
        assert_eq!(buf[(4, 0)].symbol(), "o");
    }

    #[test]
    fn default_is_empty() {
        let tb = TextBlock::default();
        assert_eq!(tb.desired_height(80, &()), 0);
    }

    #[test]
    fn multi_span_line() {
        let tb = TextBlock {
            lines: vec![Line {
                spans: vec![
                    Span {
                        text: "hello ".into(),
                        style: Style::default(),
                    },
                    Span {
                        text: "world".into(),
                        style: Style::default().fg(Color::Green),
                    },
                ],
            }],
        };
        assert_eq!(tb.desired_height(80, &()), 1);

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        tb.render(area, &mut buf, &());
        assert_eq!(buf[(0, 0)].symbol(), "h");
        assert_eq!(buf[(6, 0)].symbol(), "w");
    }
}
