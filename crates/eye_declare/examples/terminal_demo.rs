use std::io::Write;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use eye_declare::{Component, EventResult, InlineRenderer, TextBlock, Tracked, VStack};
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

#[derive(Default)]
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
// Demo using InlineRenderer directly (imperative sync API)
// ---------------------------------------------------------------------------

fn main() -> std::io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut renderer = InlineRenderer::new(width);

    // Header
    let header = renderer.push(
        TextBlock::new()
            .line(
                "eye_declare InlineRenderer Demo",
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
    flush(&mut renderer)?;
    renderer.freeze(header);

    // Message area
    let messages = renderer.push(VStack);

    // Spacer
    let _spacer = renderer.push(TextBlock::new().unstyled(""));

    // Two input fields — Tab cycles between them
    let name_id = renderer.push(Input);
    renderer.state_mut::<Input>(name_id).label = "Name".into();

    let msg_id = renderer.push(Input);
    renderer.state_mut::<Input>(msg_id).label = "Message".into();

    renderer.set_focus(name_id);

    // Initial render before entering raw mode
    flush(&mut renderer)?;

    crossterm::terminal::enable_raw_mode()?;
    let result = event_loop(&mut renderer, name_id, msg_id, messages);
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = std::io::stdout().write_all(b"\x1b[?25h");
    let _ = std::io::stdout().flush();
    println!();

    result
}

fn flush(renderer: &mut InlineRenderer) -> std::io::Result<()> {
    let output = renderer.render();
    if !output.is_empty() {
        let mut stdout = std::io::stdout();
        stdout.write_all(&output)?;
        stdout.flush()?;
    }
    Ok(())
}

fn event_loop(
    renderer: &mut InlineRenderer,
    name_id: eye_declare::NodeId,
    msg_id: eye_declare::NodeId,
    messages: eye_declare::NodeId,
) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();

    loop {
        if !event::poll(Duration::from_millis(50))? {
            if renderer.tick() {
                let output = renderer.render();
                if !output.is_empty() {
                    stdout.write_all(&output)?;
                    stdout.flush()?;
                }
            }
            continue;
        }

        let evt = event::read()?;

        // Ctrl+C exits
        if let Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers,
            kind: KeyEventKind::Press,
            ..
        }) = &evt
            && modifiers.contains(KeyModifiers::CONTROL)
        {
            break;
        }

        // Resize
        if let Event::Resize(new_width, _) = &evt {
            let output = renderer.resize(*new_width);
            stdout.write_all(&output)?;
            stdout.flush()?;
            continue;
        }

        // Enter submits a message
        if let Event::Key(KeyEvent {
            code: KeyCode::Enter,
            kind: KeyEventKind::Press,
            ..
        }) = &evt
        {
            let name = renderer.state_mut::<Input>(name_id).text.clone();
            let text = renderer.state_mut::<Input>(msg_id).take_text();
            if !text.is_empty() {
                let display_name = if name.is_empty() {
                    "anon".to_string()
                } else {
                    name
                };
                renderer.append_child(
                    messages,
                    TextBlock::new().line(
                        format!("{}: {}", display_name, text),
                        Style::default().fg(Color::White),
                    ),
                );
            }
            renderer.set_focus(msg_id);
        }

        // Framework event handling (Tab cycling, focus routing)
        renderer.handle_event(&evt);
        renderer.tick();

        let output = renderer.render();
        if !output.is_empty() {
            stdout.write_all(&output)?;
            stdout.flush()?;
        }
    }

    Ok(())
}
