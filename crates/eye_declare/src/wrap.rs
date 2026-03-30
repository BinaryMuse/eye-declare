use ratatui_core::text::Text;
use ratatui_widgets::paragraph::{Paragraph, Wrap};

/// Compute how many terminal rows `text` occupies at `width` with word wrapping.
///
/// Uses ratatui's `Paragraph` with `Wrap { trim: false }` to match
/// the rendering behavior of [`wrapping_paragraph`].
pub fn wrapped_line_count(text: &Text<'_>, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    let count = Paragraph::new(text.clone())
        .wrap(Wrap { trim: false })
        .line_count(width);
    count as u16
}

/// Create a `Paragraph` with word wrapping enabled (no trim).
///
/// This encodes the framework convention: word wrap at the terminal
/// width, preserving leading whitespace.
pub fn wrapping_paragraph<'a>(text: Text<'a>) -> Paragraph<'a> {
    Paragraph::new(text).wrap(Wrap { trim: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::text::Text;

    fn text_from(s: &str) -> Text<'_> {
        Text::from(s)
    }

    #[test]
    fn short_text_no_wrap() {
        let text = text_from("hello");
        assert_eq!(wrapped_line_count(&text, 80), 1);
    }

    #[test]
    fn text_wraps_at_width() {
        // "hello world" is 11 chars. At width 6, should wrap to 2 lines.
        let text = text_from("hello world");
        assert_eq!(wrapped_line_count(&text, 6), 2);
    }

    #[test]
    fn explicit_newlines_counted() {
        let text = text_from("line1\nline2\nline3");
        assert_eq!(wrapped_line_count(&text, 80), 3);
    }

    #[test]
    fn empty_text() {
        // ratatui's Paragraph counts an empty text as 1 line (the empty line).
        // Components should guard with is_empty() before calling wrapped_line_count.
        let text = text_from("");
        assert_eq!(wrapped_line_count(&text, 80), 1);
    }

    #[test]
    fn zero_width() {
        let text = text_from("hello");
        assert_eq!(wrapped_line_count(&text, 0), 0);
    }

    #[test]
    fn long_paragraph_wraps() {
        let text = text_from(
            "This is a longer paragraph that should wrap across multiple lines \
             when rendered at a narrow terminal width.",
        );
        let count = wrapped_line_count(&text, 40);
        assert!(count >= 3, "expected >= 3 lines at width 40, got {}", count);
    }

    #[test]
    fn wrap_with_newlines_and_long_lines() {
        let text = text_from("short\nthis line is longer than twenty characters");
        let count = wrapped_line_count(&text, 20);
        // "short" = 1 line, "this line is longer than twenty characters" wraps to 3+ lines
        assert!(count >= 3, "expected >= 3 lines, got {}", count);
    }
}
