use std::sync::Arc;

use ratatui_core::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};

use crate::children::DataChildren;
use crate::components::Canvas;
use crate::element::Elements;
use crate::wrap;

// ---------------------------------------------------------------------------
// Span — a segment of styled text (data child of Text)
// ---------------------------------------------------------------------------

/// A segment of text with a single style.
///
/// `Span` is a data child of [`Text`] — it does not implement `Component`.
/// Use it inside `Text` blocks in the `element!` macro for mixed styling:
///
/// ```ignore
/// Text {
///     "Hello "
///     Span(text: "world".into(), style: Style::default().fg(Color::Green))
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
// TextChild — what Text accepts as children
// ---------------------------------------------------------------------------

/// Child type accepted by [`Text`] in the `element!` macro.
///
/// Text accepts [`Span`] children and plain strings.
pub enum TextChild {
    /// A styled text span.
    Span(Span),
}

impl From<Span> for TextChild {
    fn from(s: Span) -> Self {
        TextChild::Span(s)
    }
}

impl From<String> for TextChild {
    fn from(s: String) -> Self {
        TextChild::Span(Span {
            text: s,
            style: Style::default(),
        })
    }
}

impl From<&str> for TextChild {
    fn from(s: &str) -> Self {
        TextChild::Span(Span {
            text: s.to_string(),
            style: Style::default(),
        })
    }
}

// ---------------------------------------------------------------------------
// Text — styled text with word wrapping
// ---------------------------------------------------------------------------

/// Styled text with display-time word wrapping.
///
/// `Text` is the primary text component. Content is provided through
/// children — either [`Span`]s for styled segments or plain strings
/// for unstyled text. Word wrapping is computed at render time, so
/// content reflows automatically when the terminal is resized.
///
/// # Simple text
///
/// ```ignore
/// // String literal sugar (element! converts bare strings to Text)
/// element! { "Hello, world!" }
///
/// // Explicit
/// element! { Text { "Hello, world!" } }
///
/// // With base style
/// element! { Text(style: Style::default().fg(Color::Green)) { "Success!" } }
/// ```
///
/// # Mixed styling
///
/// ```ignore
/// element! {
///     Text {
///         "Name: "
///         Span(text: name.clone(), style: Style::default().fg(Color::Cyan))
///     }
/// }
/// ```
///
/// # Multi-line text
///
/// Use [`View`](crate::View) for vertical stacking:
///
/// ```ignore
/// element! {
///     View {
///         Text { "Line one" }
///         Text {
///             "Line "
///             Span(text: "two", style: Style::default().add_modifier(Modifier::BOLD))
///         }
///     }
/// }
/// ```
#[derive(Default, typed_builder::TypedBuilder)]
pub struct Text {
    /// Base style applied to all spans. Individual [`Span`] styles
    /// are patched on top of this.
    #[builder(default, setter(into))]
    pub style: Style,
}

// Convenience constructors for imperative API usage.
// Return `impl Component` to hide the generated wrapper type.
impl Text {
    /// Create a Text with a single unstyled span.
    ///
    /// For use with the imperative API (`Elements::add`). In `element!`,
    /// use string literal sugar or `Text { "content" }` instead.
    pub fn unstyled(content: impl Into<String>) -> impl crate::component::Component<State = ()> {
        let mut collector = DataChildren::default();
        crate::children::AddTo::add_to(content.into(), &mut collector);
        <Text as crate::children::ChildCollector>::finish(Text::default(), collector)
    }

    /// Create a Text with a single styled span.
    pub fn styled(
        content: impl Into<String>,
        style: Style,
    ) -> impl crate::component::Component<State = ()> {
        let mut collector = DataChildren::default();
        crate::children::AddTo::add_to(
            Span {
                text: content.into(),
                style,
            },
            &mut collector,
        );
        <Text as crate::children::ChildCollector>::finish(Text::default(), collector)
    }
}

fn build_line(children: &[TextChild], base_style: Style) -> ratatui_core::text::Line<'static> {
    let spans: Vec<ratatui_core::text::Span<'static>> = children
        .iter()
        .map(|c| match c {
            TextChild::Span(s) => {
                let effective_style = base_style.patch(s.style);
                ratatui_core::text::Span::styled(s.text.clone(), effective_style)
            }
        })
        .collect();
    ratatui_core::text::Line::from(spans)
}

#[eye_declare_macros::component(props = Text, children = DataChildren<TextChild>, crate_path = crate)]
fn text(props: &Text, children: &DataChildren<TextChild>) -> Elements {
    let spans = children.as_slice();
    if spans.is_empty() {
        return Elements::new();
    }

    let line = Arc::new(build_line(spans, props.style));
    let mut els = Elements::new();
    els.add(Canvas::new(move |area: Rect, buf: &mut Buffer| {
        let text = ratatui_core::text::Text::from(vec![(*line).clone()]);
        wrap::wrapping_paragraph(text).render(area, buf);
    }));
    els
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::Hooks;
    use ratatui_core::style::Color;

    #[test]
    fn text_renders_single_span() {
        // Simulate what the element! sugar does: create wrapper with one string child
        let mut collector = DataChildren::<TextChild>::default();
        crate::children::AddTo::add_to(String::from("hello"), &mut collector);
        let wrapper = <Text as crate::children::ChildCollector>::finish(Text::default(), collector);

        // The wrapper is a Component — build it and render
        use crate::component::Component;
        let hooks_output = {
            let mut hooks = Hooks::<()>::new();
            let result = wrapper.update(&mut hooks, &(), Elements::new());
            (hooks.decompose(), result)
        };
        let elements = hooks_output.1;

        // Should have one Canvas child
        assert!(!elements.is_empty());
    }

    #[test]
    fn text_renders_multiple_spans() {
        let mut collector = DataChildren::<TextChild>::default();
        crate::children::AddTo::add_to(
            Span {
                text: "hello ".into(),
                style: Style::default(),
            },
            &mut collector,
        );
        crate::children::AddTo::add_to(
            Span {
                text: "world".into(),
                style: Style::default().fg(Color::Green),
            },
            &mut collector,
        );
        let wrapper = <Text as crate::children::ChildCollector>::finish(Text::default(), collector);

        use crate::component::Component;
        let mut hooks = Hooks::<()>::new();
        let elements = wrapper.update(&mut hooks, &(), Elements::new());
        assert!(!elements.is_empty());
    }

    #[test]
    fn text_with_base_style() {
        let base = Style::default().fg(Color::Red);
        let children = vec![TextChild::Span(Span {
            text: "hello".into(),
            style: Style::default(),
        })];
        let line = build_line(&children, base);
        // The span should have the base style (Red fg)
        assert_eq!(line.spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn span_style_overrides_base() {
        let base = Style::default().fg(Color::Red);
        let children = vec![TextChild::Span(Span {
            text: "hello".into(),
            style: Style::default().fg(Color::Green),
        })];
        let line = build_line(&children, base);
        // Span's Green overrides base Red
        assert_eq!(line.spans[0].style.fg, Some(Color::Green));
    }

    #[test]
    fn empty_children_produces_no_elements() {
        use crate::component::Component;
        let text = Text::default();
        let mut hooks = Hooks::<()>::new();
        let elements = text.update(&mut hooks, &(), Elements::new());
        assert!(elements.is_empty());
    }
}
