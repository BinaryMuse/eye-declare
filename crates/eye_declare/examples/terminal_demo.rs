use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use eye_declare::{Component, EventResult, Terminal, TextBlock, VStack};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthChar;

// ---------------------------------------------------------------------------
// Input component (reusable)
// ---------------------------------------------------------------------------

struct Input;

struct InputState {
    text: String,
    cursor: usize,
    label: String,
}

impl InputState {
    fn new(label: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            label: label.into(),
        }
    }

    fn take_text(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }
}

fn char_wrap_spans(spans: Vec<Span<'_>>, width: u16) -> Vec<Line<'_>> {
    if width == 0 {
        return vec![Line::from("")];
    }
    let mut lines: Vec<Vec<Span>> = vec![vec![]];
    let mut col: u16 = 0;
    for span in spans {
        let style = span.style;
        let mut chunk = String::new();
        for ch in span.content.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0) as u16;
            if col + w > width {
                if !chunk.is_empty() {
                    lines
                        .last_mut()
                        .unwrap()
                        .push(Span::styled(chunk.clone(), style));
                    chunk.clear();
                }
                lines.push(vec![]);
                col = 0;
            }
            chunk.push(ch);
            col += w;
        }
        if !chunk.is_empty() {
            lines.last_mut().unwrap().push(Span::styled(chunk, style));
        }
    }
    lines.into_iter().map(Line::from).collect()
}

fn visual_position(content: &str, width: u16) -> (u16, u16) {
    let mut col: u16 = 0;
    let mut row: u16 = 0;
    for ch in content.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0) as u16;
        if col + w > width && width > 0 {
            row += 1;
            col = 0;
        }
        col += w;
    }
    (col, row)
}

impl Component for Input {
    type State = InputState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let spans = vec![
            Span::styled(
                format!("{}: ", state.label),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(state.text.clone(), Style::default().fg(Color::White)),
        ];
        let lines = char_wrap_spans(spans, area.width);
        for (i, line) in lines.into_iter().enumerate() {
            if (i as u16) < area.height {
                line.render(Rect::new(area.x, area.y + i as u16, area.width, 1), buf);
            }
        }
    }

    fn desired_height(&self, width: u16, state: &Self::State) -> u16 {
        if width == 0 {
            return 0;
        }
        let label_w = state.label.chars().count() as u16 + 2;
        let text_w: u16 = state
            .text
            .chars()
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(0) as u16)
            .sum();
        let total = label_w + text_w;
        if total == 0 {
            return 1;
        }
        (total as u32).div_ceil(width as u32).max(1) as u16
    }

    fn handle_event(&self, event: &Event, state: &mut Self::State) -> EventResult {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = event
        {
            match code {
                KeyCode::Char(c) => {
                    state.text.insert(state.cursor, *c);
                    state.cursor += c.len_utf8();
                    EventResult::Consumed
                }
                KeyCode::Backspace => {
                    if state.cursor > 0 {
                        let prev = state.text[..state.cursor]
                            .chars()
                            .last()
                            .map(|c| c.len_utf8())
                            .unwrap_or(0);
                        state.cursor -= prev;
                        state.text.remove(state.cursor);
                    }
                    EventResult::Consumed
                }
                KeyCode::Left => {
                    if state.cursor > 0 {
                        let prev = state.text[..state.cursor]
                            .chars()
                            .last()
                            .map(|c| c.len_utf8())
                            .unwrap_or(0);
                        state.cursor -= prev;
                    }
                    EventResult::Consumed
                }
                KeyCode::Right => {
                    if state.cursor < state.text.len() {
                        let next = state.text[state.cursor..]
                            .chars()
                            .next()
                            .map(|c| c.len_utf8())
                            .unwrap_or(0);
                        state.cursor += next;
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            }
        } else {
            EventResult::Ignored
        }
    }

    fn is_focusable(&self, _state: &Self::State) -> bool {
        true
    }

    fn cursor_position(&self, area: Rect, state: &Self::State) -> Option<(u16, u16)> {
        let before = format!("{}: {}", state.label, &state.text[..state.cursor]);
        let (col, row) = visual_position(&before, area.width);
        Some((col, row))
    }

    fn initial_state(&self) -> Option<InputState> {
        Some(InputState::new("Input"))
    }
}

// ---------------------------------------------------------------------------
// Demo using Terminal wrapper
// ---------------------------------------------------------------------------

fn main() -> std::io::Result<()> {
    let mut term = Terminal::new()?;

    // Header
    let header = term.push(
        TextBlock::new()
            .line(
                "eye_declare Terminal Demo",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .line(
                "Tab switches between inputs. Enter submits. Ctrl+C exits.",
                Style::default().fg(Color::DarkGray),
            )
            .unstyled(""),
    );
    term.flush()?;
    term.freeze(header);

    // Message area
    let messages = term.push(VStack);

    // Spacer
    let _spacer = term.push(TextBlock::new().unstyled(""));

    // Two input fields — Tab cycles between them
    let name_id = term.push(Input);
    {
        let s = term.state_mut::<Input>(name_id);
        s.label = "Name".into();
    }

    let msg_id = term.push(Input);
    {
        let s = term.state_mut::<Input>(msg_id);
        s.label = "Message".into();
    }

    term.set_focus(name_id);

    term.run(|event, renderer| {
        if let Event::Key(KeyEvent {
            code: KeyCode::Enter,
            kind: KeyEventKind::Press,
            ..
        }) = event
        {
            let name = renderer.state_mut::<Input>(name_id).text.clone();
            let text = renderer.state_mut::<Input>(msg_id).take_text();
            if !text.is_empty() {
                let display_name = if name.is_empty() {
                    "anon".to_string()
                } else {
                    name
                };
                let msg = renderer.append_child(
                    messages,
                    TextBlock::new().line(
                        format!("{}: {}", display_name, text),
                        Style::default().fg(Color::White),
                    ),
                );
                let _ = msg;
            }
            // Move focus back to message input after submit
            renderer.set_focus(msg_id);
            return false;
        }
        false
    })?;

    Ok(())
}
