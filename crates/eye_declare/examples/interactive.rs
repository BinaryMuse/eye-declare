use std::io::{self, Write};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use eye_declare::{Component, EventResult, InlineRenderer, Text, Tracked};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;
use unicode_width::UnicodeWidthChar;

// ---------------------------------------------------------------------------
// Input component — a simple single-line text input
// ---------------------------------------------------------------------------

struct Input;

#[derive(Default)]
struct InputState {
    text: String,
    cursor: usize,
    label: String,
}

/// Compute the visual (col, row) position after walking through `content`
/// with character wrapping at `width`, starting at column `start_col`.
fn visual_position(content: &str, width: u16, start_col: u16) -> (u16, u16) {
    let mut col = start_col;
    let mut row: u16 = 0;
    for ch in content.chars() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0) as u16;
        if col + ch_w > width && width > 0 {
            row += 1;
            col = 0;
        }
        col += ch_w;
    }
    (col, row)
}

/// Split styled content into visual lines with character wrapping.
/// Returns a Vec of Lines, each fitting within `width` columns.
fn char_wrap_line(spans: Vec<Span<'_>>, width: u16) -> Vec<Line<'_>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut lines: Vec<Vec<Span>> = vec![vec![]];
    let mut col: u16 = 0;

    for span in spans {
        let style = span.style;
        let mut current_chunk = String::new();

        for ch in span.content.chars() {
            let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0) as u16;
            if col + ch_w > width {
                // Flush current chunk to the current line
                if !current_chunk.is_empty() {
                    lines
                        .last_mut()
                        .unwrap()
                        .push(Span::styled(current_chunk.clone(), style));
                    current_chunk.clear();
                }
                // Start new line
                lines.push(vec![]);
                col = 0;
            }
            current_chunk.push(ch);
            col += ch_w;
        }

        // Flush remaining chunk
        if !current_chunk.is_empty() {
            lines
                .last_mut()
                .unwrap()
                .push(Span::styled(current_chunk, style));
        }
    }

    lines.into_iter().map(Line::from).collect()
}

impl Component for Input {
    type State = InputState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let label_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::default().fg(Color::White);

        let spans = vec![
            Span::styled(format!("{}: ", state.label), label_style),
            Span::styled(state.text.clone(), text_style),
        ];

        let wrapped_lines = char_wrap_line(spans, area.width);
        for (i, line) in wrapped_lines.into_iter().enumerate() {
            if (i as u16) < area.height {
                let line_area = Rect::new(area.x, area.y + i as u16, area.width, 1);
                line.render(line_area, buf);
            }
        }
    }

    fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = event
        {
            let state = &mut **state;
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

    fn cursor_position(&self, area: Rect, state: &Self::State) -> Option<(u16, u16)> {
        // Walk through label + text up to cursor, wrapping at area.width
        let label_text = format!("{}: ", state.label);
        let before_cursor = format!("{}{}", label_text, &state.text[..state.cursor]);
        let (col, row) = visual_position(&before_cursor, area.width, 0);
        Some((col, row))
    }

    fn initial_state(&self) -> Option<InputState> {
        Some(InputState {
            text: String::new(),
            cursor: 0,
            label: String::from("Input"),
        })
    }
}

// ---------------------------------------------------------------------------
// Message log — displays submitted messages
// ---------------------------------------------------------------------------

struct MessageLog;

impl Component for MessageLog {
    type State = Vec<String>;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let lines: Vec<Line> = state
            .iter()
            .map(|msg| {
                Line::from(vec![
                    Span::styled("› ", Style::default().fg(Color::Green)),
                    Span::styled(msg.as_str(), Style::default().fg(Color::White)),
                ])
            })
            .collect();
        Paragraph::new(lines).render(area, buf);
    }

    fn initial_state(&self) -> Option<Vec<String>> {
        Some(vec![])
    }
}

// ---------------------------------------------------------------------------
// Demo
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    // Header
    let header1 = r.push(Text::styled(
        "Interactive Input Demo",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    let header2 = r.push(Text::styled(
        "Type a message and press Enter to submit. Ctrl+C to exit.",
        Style::default().fg(Color::DarkGray),
    ));
    let header3 = r.push(Text::unstyled(""));
    flush(&mut r, &mut stdout)?;
    r.freeze(header1);
    r.freeze(header2);
    r.freeze(header3);

    // Message log
    let log_id = r.push(MessageLog);

    // Spacer
    let _spacer = r.push(Text::unstyled(""));

    // Input field
    let input_id = r.push(Input);
    r.set_focus(input_id);

    flush(&mut r, &mut stdout)?;

    // Enable raw mode for keystroke-by-keystroke input
    crossterm::terminal::enable_raw_mode()?;

    loop {
        if event::poll(Duration::from_millis(50))? {
            let evt = event::read()?;

            match &evt {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers,
                    kind: KeyEventKind::Press,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => {
                    break;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    // Submit: move input text to the log
                    let text = {
                        let state = r.state_mut::<Input>(input_id);
                        let t = state.text.clone();
                        state.text.clear();
                        state.cursor = 0;
                        t
                    };
                    if !text.is_empty() {
                        r.state_mut::<MessageLog>(log_id).push(text);
                    }
                }
                Event::Resize(new_width, _) => {
                    let output = r.resize(*new_width);
                    stdout.write_all(&output)?;
                    stdout.flush()?;
                    continue;
                }
                _ => {
                    // Deliver to focused component
                    r.handle_event(&evt);
                }
            }

            let output = r.render();
            if !output.is_empty() {
                stdout.write_all(&output)?;
                stdout.flush()?;
            }
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    println!();
    Ok(())
}

fn flush(r: &mut InlineRenderer, stdout: &mut impl Write) -> io::Result<()> {
    let output = r.render();
    if !output.is_empty() {
        stdout.write_all(&output)?;
        stdout.flush()?;
    }
    Ok(())
}
