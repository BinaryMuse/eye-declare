use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

use crate::component::Component;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// A built-in animated spinner component with a label.
///
/// Advances the animation by incrementing `SpinnerState::frame`.
/// When `done` is set, displays a checkmark with the done label.
pub struct Spinner;

/// State for a [`Spinner`] component.
pub struct SpinnerState {
    /// Label shown next to the spinner.
    pub label: String,
    /// Current animation frame index. Increment to animate.
    pub frame: usize,
    /// When true, shows a completion checkmark instead of the spinner.
    pub done: bool,
    /// Optional label to show when done (defaults to `label` if None).
    pub done_label: Option<String>,
    /// Style for the spinner character. Defaults to cyan.
    pub spinner_style: Style,
    /// Style for the label text. Defaults to dim italic.
    pub label_style: Style,
    /// Style for the done checkmark + label. Defaults to green.
    pub done_style: Style,
}

impl SpinnerState {
    /// Create a new spinner state with the given label.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            frame: 0,
            done: false,
            done_label: None,
            spinner_style: Style::default().fg(Color::Cyan),
            label_style: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            done_style: Style::default().fg(Color::Green),
        }
    }

    /// Mark the spinner as complete with an optional label.
    pub fn complete(&mut self, label: Option<String>) {
        self.done = true;
        self.done_label = label;
    }

    /// Advance the animation by one frame.
    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }
}

impl Component for Spinner {
    type State = SpinnerState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let line = if state.done {
            let label = state.done_label.as_deref().unwrap_or(&state.label);
            Line::from(vec![
                Span::styled("✓ ", state.done_style),
                Span::styled(label.to_string(), state.done_style),
            ])
        } else {
            let frame_str = FRAMES[state.frame % FRAMES.len()];
            Line::from(vec![
                Span::styled(format!("{} ", frame_str), state.spinner_style),
                Span::styled(state.label.clone(), state.label_style),
            ])
        };
        Paragraph::new(line).render(area, buf);
    }

    fn desired_height(&self, _width: u16, _state: &Self::State) -> u16 {
        1
    }

    fn initial_state(&self) -> SpinnerState {
        SpinnerState::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_height_is_one() {
        let spinner = Spinner;
        let state = SpinnerState::new("Loading...");
        assert_eq!(spinner.desired_height(80, &state), 1);
    }

    #[test]
    fn spinner_renders_frame() {
        let spinner = Spinner;
        let state = SpinnerState::new("Loading...");
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf, &state);
        // First frame char should be the first spinner frame
        assert_eq!(buf[(0, 0)].symbol(), "⠋");
    }

    #[test]
    fn spinner_done_shows_checkmark() {
        let spinner = Spinner;
        let mut state = SpinnerState::new("Loading...");
        state.complete(Some("Done!".into()));
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf, &state);
        assert_eq!(buf[(0, 0)].symbol(), "✓");
    }

    #[test]
    fn tick_advances_frame() {
        let mut state = SpinnerState::new("test");
        assert_eq!(state.frame, 0);
        state.tick();
        assert_eq!(state.frame, 1);
        state.tick();
        assert_eq!(state.frame, 2);
    }
}
