use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

use std::time::Duration;

use crate::components::Canvas;
use crate::element::Elements;
use crate::hooks::Hooks;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// An animated Braille spinner with a text label.
///
/// The spinner animates automatically via a lifecycle interval — no manual
/// ticking is needed when used with [`Application`](crate::Application).
/// When `done` is set, the spinner shows a checkmark instead of animating.
///
/// # Examples
///
/// ```ignore
/// // Basic spinner
/// Spinner::new("Loading...")
///
/// // Spinner that shows a completion label when done
/// Spinner::new("Compiling").done("Build complete!")
///
/// // In the element! macro
/// element! {
///     Spinner(key: "fetch", label: "Fetching data...".into())
/// }
/// ```
///
/// # Customization
///
/// All visual aspects are configurable via struct fields:
/// `label_style`, `spinner_style`, `done_label_style`, `checkmark_style`,
/// `label_first` (swap label/spinner order), and `hide_checkmark`.
#[derive(typed_builder::TypedBuilder)]
pub struct Spinner {
    /// Text displayed next to the spinner animation.
    #[builder(default, setter(into))]
    pub label: String,
    /// When `true`, the spinner stops animating and shows a checkmark.
    #[builder(default, setter(into))]
    pub done: bool,
    /// Replacement label shown in the done state. Falls back to `label`
    /// if `None`.
    #[builder(default, setter(into))]
    pub done_label: Option<String>,
    /// Hide the checkmark symbol in the done state.
    #[builder(default, setter(into))]
    pub hide_checkmark: bool,
    /// Place the label before the spinner/checkmark instead of after.
    #[builder(default, setter(into))]
    pub label_first: bool,
    /// Style for the label text while spinning.
    #[builder(default = Style::default().fg(Color::DarkGray), setter(into))]
    pub label_style: Style,
    /// Style for the label text in the done state.
    #[builder(default = Style::default().fg(Color::Green), setter(into))]
    pub done_label_style: Style,
    /// Style for the animated spinner character.
    #[builder(default = Style::default().fg(Color::DarkGray), setter(into))]
    pub spinner_style: Style,
    /// Style for the checkmark in the done state.
    #[builder(default = Style::default().fg(Color::Green), setter(into))]
    pub checkmark_style: Style,
}

impl Default for Spinner {
    fn default() -> Self {
        Self {
            label: String::new(),
            done: false,
            done_label: None,
            hide_checkmark: false,
            label_first: false,
            label_style: Style::default().fg(Color::DarkGray),
            done_label_style: Style::default().fg(Color::Green),
            spinner_style: Style::default().fg(Color::DarkGray),
            checkmark_style: Style::default().fg(Color::Green),
        }
    }
}

impl Spinner {
    /// Create a new spinner with the given label.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Default::default()
        }
    }

    /// Mark the spinner as already done, with a completion label.
    pub fn done(mut self, label: impl Into<String>) -> Self {
        self.done = true;
        self.done_label = Some(label.into());
        self
    }

    fn build_line(&self, frame: usize) -> Line<'static> {
        if self.done {
            let label = self.done_label.as_deref().unwrap_or(&self.label);

            if self.label_first {
                if self.hide_checkmark {
                    Line::from(vec![Span::styled(label.to_string(), self.done_label_style)])
                } else {
                    Line::from(vec![
                        Span::styled(label.to_string(), self.done_label_style),
                        Span::styled("✓ ", self.checkmark_style),
                    ])
                }
            } else {
                if self.hide_checkmark {
                    Line::from(vec![Span::styled(label.to_string(), self.done_label_style)])
                } else {
                    Line::from(vec![
                        Span::styled("✓ ", self.checkmark_style),
                        Span::styled(label.to_string(), self.done_label_style),
                    ])
                }
            }
        } else {
            let frame_str = FRAMES[frame % FRAMES.len()];
            let label = Span::styled(self.label.clone(), self.label_style);

            if self.label_first {
                let frame = Span::styled(format!(" {}", frame_str), self.spinner_style);
                Line::from(vec![label, frame])
            } else {
                let frame = Span::styled(format!("{} ", frame_str), self.spinner_style);
                Line::from(vec![frame, label])
            }
        }
    }
}

/// Internal state for a [`Spinner`] component.
///
/// Tracks the current animation frame. You typically don't interact
/// with this directly — the spinner's lifecycle interval calls
/// [`tick`](SpinnerState::tick) automatically. Access it via
/// [`InlineRenderer::state_mut`](crate::InlineRenderer::state_mut)
/// if you need manual control.
pub struct SpinnerState {
    /// Current animation frame index. Increment to animate.
    pub frame: usize,
}

impl SpinnerState {
    /// Create initial spinner state at frame 0.
    pub fn new() -> Self {
        Self { frame: 0 }
    }

    /// Advance the animation by one frame.
    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }
}

impl Default for SpinnerState {
    fn default() -> Self {
        Self::new()
    }
}

#[eye_declare_macros::component(props = Spinner, state = SpinnerState, initial_state = SpinnerState::new(), crate_path = crate)]
fn spinner(props: &Spinner, state: &SpinnerState, hooks: &mut Hooks<SpinnerState>) -> Elements {
    if !props.done {
        hooks.use_interval(Duration::from_millis(80), |s| s.tick());
    }

    let line = props.build_line(state.frame);

    let mut els = Elements::new();
    els.add(Canvas::new(move |area: Rect, buf: &mut Buffer| {
        Paragraph::new(line.clone()).render(area, buf);
    }));
    els
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_renders_frame() {
        let spinner = Spinner::new("Loading...");
        let state = SpinnerState::new();
        let line = spinner.build_line(state.frame);
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        Paragraph::new(line).render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "⠋");
    }

    #[test]
    fn spinner_done_shows_checkmark() {
        let spinner = Spinner::new("Loading...").done("Done!");
        let state = SpinnerState::new();
        let line = spinner.build_line(state.frame);
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        Paragraph::new(line).render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "✓");
    }

    #[test]
    fn tick_advances_frame() {
        let mut state = SpinnerState::new();
        assert_eq!(state.frame, 0);
        state.tick();
        assert_eq!(state.frame, 1);
        state.tick();
        assert_eq!(state.frame, 2);
    }
}
