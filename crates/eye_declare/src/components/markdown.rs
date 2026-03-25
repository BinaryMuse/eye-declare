use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Widget,
};

use crate::component::Component;
use crate::wrap;

/// Renders a subset of Markdown as styled terminal text.
///
/// Supports headings (`#`, `##`, `###`), **bold**, *italic*, `inline code`,
/// fenced code blocks, and unordered lists. Designed for rendering
/// LLM/AI chat output in the terminal.
///
/// The markdown source is a prop; style configuration lives in
/// [`MarkdownState`] (internal state with sensible defaults).
///
/// # Examples
///
/// ```ignore
/// // Constructor
/// Markdown::new("# Hello\n\nThis is **bold** and `code`.")
///
/// // In the element! macro
/// element! {
///     Markdown(key: "response", source: response_text.clone())
/// }
/// ```
#[derive(Default)]
pub struct Markdown {
    /// The markdown source text to render.
    pub source: String,
}

impl Markdown {
    /// Create a new markdown component with the given source text.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

/// Style configuration for a [`Markdown`] component.
///
/// Initialized with sensible terminal defaults (cyan headings, yellow
/// inline code, green code blocks, etc.). Override individual fields
/// to match your application's color scheme.
///
/// The markdown source text is a prop on the [`Markdown`] struct,
/// not part of state.
pub struct MarkdownState {
    /// Base style for normal text.
    pub base_style: Style,
    /// Style for inline code.
    pub code_style: Style,
    /// Style for code blocks.
    pub block_code_style: Style,
    /// Style for bold text.
    pub bold_style: Style,
    /// Style for italic text.
    pub italic_style: Style,
    /// Style for headings.
    pub heading_style: Style,
    /// Style for list markers.
    pub marker_style: Style,
}

impl MarkdownState {
    /// Create the default style configuration: cyan bold headings, yellow
    /// inline code, green code blocks, bold/italic modifiers, dark gray
    /// list markers.
    pub fn new() -> Self {
        let base = Style::default();
        Self {
            base_style: base,
            code_style: Style::default().fg(Color::Yellow),
            block_code_style: Style::default().fg(Color::Green),
            bold_style: base.add_modifier(Modifier::BOLD),
            italic_style: base.add_modifier(Modifier::ITALIC),
            heading_style: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            marker_style: Style::default().fg(Color::DarkGray),
        }
    }
}

impl Default for MarkdownState {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Markdown {
    type State = MarkdownState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        if self.source.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }
        let text = render_markdown(&self.source, state);
        wrap::wrapping_paragraph(text).render(area, buf);
    }

    fn desired_height(&self, width: u16, state: &Self::State) -> u16 {
        if self.source.is_empty() || width == 0 {
            return 0;
        }
        let text = render_markdown(&self.source, state);
        wrap::wrapped_line_count(&text, width)
    }

    fn initial_state(&self) -> Option<MarkdownState> {
        Some(MarkdownState::new())
    }
}

/// Parse markdown source into styled ratatui Text.
fn render_markdown(source: &str, styles: &MarkdownState) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;

    for line in source.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            if in_code_block {
                // Opening fence — skip the line (or show language hint)
                let lang = line.trim_start_matches('`').trim();
                if !lang.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", lang),
                        styles.marker_style,
                    )));
                }
            }
            // Closing fence — just skip the line
            continue;
        }

        if in_code_block {
            // Code block content: render as-is with code style
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                styles.block_code_style,
            )));
            continue;
        }

        // Heading
        if let Some(content) = line.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                content.to_string(),
                styles.heading_style,
            )));
            continue;
        }
        if let Some(content) = line.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                content.to_string(),
                styles.heading_style,
            )));
            continue;
        }
        if let Some(content) = line.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                content.to_string(),
                styles.heading_style.add_modifier(Modifier::UNDERLINED),
            )));
            continue;
        }

        // Unordered list item
        let list_prefix = if line.starts_with("- ") || line.starts_with("* ") {
            Some(&line[..2])
        } else if line.starts_with("  - ") || line.starts_with("  * ") {
            Some(&line[..4])
        } else {
            None
        };

        if let Some(prefix) = list_prefix {
            let content = &line[prefix.len()..];
            let mut spans = vec![Span::styled(prefix.to_string(), styles.marker_style)];
            spans.extend(parse_inline_formatting(content, styles));
            lines.push(Line::from(spans));
            continue;
        }

        // Empty line
        if line.trim().is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // Regular paragraph line with inline formatting
        let spans = parse_inline_formatting(line, styles);
        lines.push(Line::from(spans));
    }

    Text::from(lines)
}

/// Parse inline markdown formatting (**bold**, *italic*, `code`)
/// into styled spans. Based on Atuin's parse_inline_formatting.
fn parse_inline_formatting(line: &str, styles: &MarkdownState) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '`' {
            // Flush accumulated plain text
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    styles.base_style,
                ));
            }
            // Collect until closing backtick
            let mut code_text = String::new();
            let mut closed = false;
            for next in chars.by_ref() {
                if next == '`' {
                    closed = true;
                    break;
                }
                code_text.push(next);
            }
            if closed {
                spans.push(Span::styled(code_text, styles.code_style));
            } else {
                // Unclosed backtick — render as-is
                current.push('`');
                current.push_str(&code_text);
            }
        } else if ch == '*' && chars.peek() == Some(&'*') {
            chars.next(); // consume second *
            // Flush accumulated plain text
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    styles.base_style,
                ));
            }
            // Collect until closing **
            let mut bold_text = String::new();
            let mut closed = false;
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    closed = true;
                    break;
                }
                bold_text.push(next);
            }
            if closed {
                spans.push(Span::styled(bold_text, styles.bold_style));
            } else {
                // Unclosed ** — render as-is
                current.push_str("**");
                current.push_str(&bold_text);
            }
        } else if ch == '*' {
            // Single * — italic
            // Flush accumulated plain text
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    styles.base_style,
                ));
            }
            // Collect until closing *
            let mut italic_text = String::new();
            let mut closed = false;
            for next in chars.by_ref() {
                if next == '*' {
                    closed = true;
                    break;
                }
                italic_text.push(next);
            }
            if closed {
                spans.push(Span::styled(italic_text, styles.italic_style));
            } else {
                // Unclosed * — render as-is
                current.push('*');
                current.push_str(&italic_text);
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, styles.base_style));
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_markdown() {
        let md = Markdown::new("");
        let state = md.initial_state().unwrap();
        assert_eq!(md.desired_height(80, &state), 0);
    }

    #[test]
    fn plain_text() {
        let md = Markdown::new("Hello world");
        let state = md.initial_state().unwrap();
        assert_eq!(md.desired_height(80, &state), 1);
    }

    #[test]
    fn heading_renders() {
        let md = Markdown::new("# Title");
        let state = md.initial_state().unwrap();
        let text = render_markdown(&md.source, &state);
        assert_eq!(text.lines.len(), 1);
        assert!(
            text.lines[0]
                .spans
                .iter()
                .any(|s| s.content.contains("Title"))
        );
    }

    #[test]
    fn code_block_indented() {
        let md = Markdown::new("```rust\nfn main() {}\n```");
        let state = md.initial_state().unwrap();
        let text = render_markdown(&md.source, &state);
        // Should have language hint + code line
        assert!(text.lines.len() >= 2);
        // Code should be indented
        assert!(text.lines.last().unwrap().to_string().contains("fn main"));
    }

    #[test]
    fn inline_bold() {
        let md = Markdown::new("This is **bold** text");
        let state = md.initial_state().unwrap();
        let text = render_markdown(&md.source, &state);
        let spans = &text.lines[0].spans;
        // Should have at least 3 spans: "This is ", "bold", " text"
        assert!(spans.len() >= 3);
        // Bold span should have BOLD modifier
        let bold_span = spans.iter().find(|s| s.content.contains("bold")).unwrap();
        assert!(bold_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inline_italic() {
        let md = Markdown::new("This is *italic* text");
        let state = md.initial_state().unwrap();
        let text = render_markdown(&md.source, &state);
        let spans = &text.lines[0].spans;
        let italic_span = spans.iter().find(|s| s.content.contains("italic")).unwrap();
        assert!(italic_span.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn inline_code() {
        let md = Markdown::new("Use `println!` here");
        let state = md.initial_state().unwrap();
        let text = render_markdown(&md.source, &state);
        let spans = &text.lines[0].spans;
        let code_span = spans
            .iter()
            .find(|s| s.content.contains("println!"))
            .unwrap();
        assert_eq!(code_span.style.fg, Some(Color::Yellow));
    }

    #[test]
    fn list_items() {
        let md = Markdown::new("- item one\n- item two");
        let state = md.initial_state().unwrap();
        let text = render_markdown(&md.source, &state);
        assert_eq!(text.lines.len(), 2);
    }

    #[test]
    fn unclosed_markers_render_as_text() {
        let md = Markdown::new("This has an unclosed **bold");
        let state = md.initial_state().unwrap();
        let text = render_markdown(&md.source, &state);
        // Should render without panic, showing ** as literal text
        let full_text: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(full_text.contains("**bold"));
    }

    #[test]
    fn mixed_formatting() {
        let md = Markdown::new(
            "# Welcome\n\nThis is **bold** and *italic* with `code`.\n\n```\nlet x = 1;\n```\n\n- item",
        );
        let state = md.initial_state().unwrap();
        let text = render_markdown(&md.source, &state);
        // Should have: heading, blank, paragraph, blank, code, blank, list item
        assert!(text.lines.len() >= 5);
    }

    #[test]
    fn wraps_at_width() {
        let md = Markdown::new(
            "This is a long paragraph that should wrap when rendered at a narrow width.",
        );
        let state = md.initial_state().unwrap();
        let height_wide = md.desired_height(80, &state);
        let height_narrow = md.desired_height(20, &state);
        assert_eq!(height_wide, 1);
        assert!(height_narrow >= 4);
    }
}
