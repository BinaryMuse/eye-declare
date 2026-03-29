use ratatui_core::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};

use crate::children::{ChildCollector, DataChildren};
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
#[derive(Clone, Debug, Default, typed_builder::TypedBuilder)]
pub struct Span {
    /// The text content of this span.
    #[builder(default, setter(into))]
    pub text: String,
    /// The style applied to this span's text.
    #[builder(default, setter(into))]
    pub style: Style,
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
#[derive(Clone, Debug, Default, typed_builder::TypedBuilder)]
pub struct Line {
    /// The spans that compose this line.
    #[builder(default, setter(into))]
    pub spans: Vec<Span>,
}

// ---------------------------------------------------------------------------
// Data child types for Line and TextBlock
// ---------------------------------------------------------------------------

/// Child type accepted by [`Line`] in the `element!` macro.
///
/// Line accepts [`Span`] children via `From<Span>`.
pub enum LineChild {
    /// A styled text span.
    Span(Span),
}

impl From<Span> for LineChild {
    fn from(s: Span) -> Self {
        LineChild::Span(s)
    }
}

impl ChildCollector for Line {
    type Collector = DataChildren<LineChild>;
    type Output = Line;

    fn finish(mut self, collector: DataChildren<LineChild>) -> Line {
        self.spans = collector
            .into_vec()
            .into_iter()
            .map(|c| match c {
                LineChild::Span(s) => s,
            })
            .collect();
        self
    }
}

/// Child type accepted by [`TextBlock`] in the `element!` macro.
///
/// TextBlock accepts [`Line`] children via `From<Line>`.
pub enum TextBlockChild {
    /// A line of styled text.
    Line(Line),
}

impl From<Line> for TextBlockChild {
    fn from(l: Line) -> Self {
        TextBlockChild::Line(l)
    }
}

impl ChildCollector for TextBlock {
    type Collector = DataChildren<TextBlockChild>;
    type Output = TextBlock;

    fn finish(mut self, collector: DataChildren<TextBlockChild>) -> TextBlock {
        self.lines = collector
            .into_vec()
            .into_iter()
            .map(|c| match c {
                TextBlockChild::Line(l) => l,
            })
            .collect();
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
#[derive(typed_builder::TypedBuilder)]
pub struct TextBlock {
    /// The logical lines of styled text. Each [`Line`] contains one or more
    /// [`Span`]s. Word wrapping is computed at render time.
    #[builder(default, setter(into))]
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::style::Color;

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
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        tb.render(area, &mut buf, &());
        assert_eq!(buf[(0, 0)].symbol(), "h");
        assert_eq!(buf[(6, 0)].symbol(), "w");
    }
}
