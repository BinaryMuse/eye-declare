use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use eye_declare::{Component, InlineRenderer, NodeId, Tracked, VStack};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

struct Spinner;
struct SpinnerState {
    label: String,
    frame: usize,
    done: bool,
    done_label: Option<String>,
}

impl Component for Spinner {
    type State = SpinnerState;
    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let line = if state.done {
            Line::from(vec![
                Span::styled("✓ ", Style::default().fg(Color::Green)),
                Span::styled(
                    state.done_label.as_deref().unwrap_or(&state.label),
                    Style::default().fg(Color::Green),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    format!("{} ", SPINNER_FRAMES[state.frame % SPINNER_FRAMES.len()]),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    state.label.as_str(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])
        };
        Paragraph::new(line).render(area, buf);
    }
    fn desired_height(&self, _w: u16, _s: &Self::State) -> u16 {
        1
    }
    fn initial_state(&self) -> Option<SpinnerState> {
        Some(SpinnerState {
            label: String::new(),
            frame: 0,
            done: false,
            done_label: None,
        })
    }
}

struct StreamingText;
struct StreamingState {
    tokens: Vec<String>,
    revealed: usize,
    style: Style,
}

impl Component for StreamingText {
    type State = StreamingState;
    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let visible: String = state.tokens[..state.revealed].join("");
        let lines: Vec<Line> = visible
            .lines()
            .map(|l| Line::styled(l, state.style))
            .collect();
        Paragraph::new(lines).render(area, buf);
    }
    fn desired_height(&self, _w: u16, state: &Self::State) -> u16 {
        let visible: String = state.tokens[..state.revealed].join("");
        if visible.is_empty() {
            return 0;
        }
        let count = visible.lines().count();
        if visible.ends_with('\n') {
            count as u16 + 1
        } else {
            count as u16
        }
    }
    fn initial_state(&self) -> Option<StreamingState> {
        Some(StreamingState {
            tokens: vec![],
            revealed: 0,
            style: Style::default(),
        })
    }
}

struct StaticLine;
impl Component for StaticLine {
    type State = (String, Style);
    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        Paragraph::new(Line::styled(state.0.as_str(), state.1)).render(area, buf);
    }
    fn desired_height(&self, _w: u16, s: &Self::State) -> u16 {
        if s.0.is_empty() { 0 } else { 1 }
    }
    fn initial_state(&self) -> Option<(String, Style)> {
        Some((String::new(), Style::default()))
    }
}

// ---------------------------------------------------------------------------
// Demo: nested tree structure
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    // ─── Turn 1: User message ───
    let user_msg = r.push(StaticLine);
    {
        let s = r.state_mut::<StaticLine>(user_msg);
        s.0 = "› Explain how async/await works in Rust".into();
        s.1 = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
    }
    flush(&mut r, &mut stdout)?;
    r.freeze(user_msg);
    thread::sleep(Duration::from_millis(300));

    // ─── Turn 2: Agent response (nested container) ───
    // response_group is a VStack containing all sub-parts of the response
    let spacer = r.push(StaticLine);
    {
        r.state_mut::<StaticLine>(spacer).0 = " ".into();
    }
    r.freeze(spacer);

    let response_group = r.push(VStack);

    // 2a: Thinking spinner (child of response_group)
    let think = r.append_child(response_group, Spinner);
    {
        r.state_mut::<Spinner>(think).label = "Thinking...".into();
    }
    animate_spinner(&mut r, &mut stdout, think, Duration::from_millis(1200))?;
    {
        let s = r.state_mut::<Spinner>(think);
        s.done = true;
        s.done_label = Some("Thought for 1.2s".into());
    }
    flush(&mut r, &mut stdout)?;
    r.freeze(think);

    // 2b: Streaming text (child of response_group)
    let text = r.append_child(response_group, StreamingText);
    {
        let s = r.state_mut::<StreamingText>(text);
        s.style = Style::default().fg(Color::White);
        s.tokens = tokenize(
            "Async/await in Rust is built on top of the `Future` trait. \
             When you write `async fn`, the compiler transforms it into a \
             state machine that implements `Future`.\n",
        );
    }
    stream(&mut r, &mut stdout, text, Duration::from_millis(20))?;
    r.freeze(text);

    // 2c: Tool call (nested container within response_group)
    let tool_group = r.append_child(response_group, VStack);

    let tool_spinner = r.append_child(tool_group, Spinner);
    {
        r.state_mut::<Spinner>(tool_spinner).label = "Reading src/executor.rs...".into();
    }
    animate_spinner(
        &mut r,
        &mut stdout,
        tool_spinner,
        Duration::from_millis(1500),
    )?;
    {
        let s = r.state_mut::<Spinner>(tool_spinner);
        s.done = true;
        s.done_label = Some("Read src/executor.rs (234 lines)".into());
    }
    flush(&mut r, &mut stdout)?;
    r.freeze(tool_spinner);

    // 2d: More streaming text after the tool call
    let text2 = r.append_child(response_group, StreamingText);
    {
        let s = r.state_mut::<StreamingText>(text2);
        s.style = Style::default().fg(Color::White);
        s.tokens = tokenize(
            "Looking at your executor, it uses `tokio::spawn` to schedule \
             futures onto the runtime's thread pool. Each `.await` point is \
             a yield point where the runtime can switch to another task.\n",
        );
    }
    stream(&mut r, &mut stdout, text2, Duration::from_millis(20))?;

    // Freeze the entire response group at once
    r.freeze(response_group);

    // ─── Turn 3: Follow-up ───
    let spacer2 = r.push(StaticLine);
    {
        r.state_mut::<StaticLine>(spacer2).0 = " ".into();
    }
    r.freeze(spacer2);

    let user2 = r.push(StaticLine);
    {
        let s = r.state_mut::<StaticLine>(user2);
        s.0 = "› What happens if a future is dropped?".into();
        s.1 = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
    }
    flush(&mut r, &mut stdout)?;
    r.freeze(user2);
    thread::sleep(Duration::from_millis(300));

    let spacer3 = r.push(StaticLine);
    {
        r.state_mut::<StaticLine>(spacer3).0 = " ".into();
    }
    r.freeze(spacer3);

    let response2 = r.push(VStack);
    let think2 = r.append_child(response2, Spinner);
    {
        r.state_mut::<Spinner>(think2).label = "Thinking...".into();
    }
    animate_spinner(&mut r, &mut stdout, think2, Duration::from_millis(800))?;
    {
        let s = r.state_mut::<Spinner>(think2);
        s.done = true;
        s.done_label = Some("Thought for 0.8s".into());
    }
    flush(&mut r, &mut stdout)?;
    r.freeze(think2);

    let text3 = r.append_child(response2, StreamingText);
    {
        let s = r.state_mut::<StreamingText>(text3);
        s.style = Style::default().fg(Color::White);
        s.tokens = tokenize(
            "When a future is dropped, its destructor runs and cleans up any \
             resources. Crucially, the computation simply stops — it won't \
             resume. This is Rust's cancellation model: drop = cancel.\n",
        );
    }
    stream(&mut r, &mut stdout, text3, Duration::from_millis(20))?;

    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers (same as agent_sim)
// ---------------------------------------------------------------------------

fn flush(r: &mut InlineRenderer, stdout: &mut impl Write) -> io::Result<()> {
    let output = r.render();
    if !output.is_empty() {
        stdout.write_all(&output)?;
        stdout.flush()?;
    }
    Ok(())
}

fn animate_spinner(
    r: &mut InlineRenderer,
    stdout: &mut impl Write,
    id: NodeId,
    dur: Duration,
) -> io::Result<()> {
    let start = Instant::now();
    while start.elapsed() < dur {
        {
            r.state_mut::<Spinner>(id).frame += 1;
        }
        flush(r, stdout)?;
        thread::sleep(Duration::from_millis(80));
    }
    Ok(())
}

fn stream(
    r: &mut InlineRenderer,
    stdout: &mut impl Write,
    id: NodeId,
    delay: Duration,
) -> io::Result<()> {
    let total = {
        let s: &Tracked<StreamingState> = r.state_mut::<StreamingText>(id);
        s.tokens.len()
    };
    for i in 1..=total {
        {
            r.state_mut::<StreamingText>(id).revealed = i;
        }
        flush(r, stdout)?;
        thread::sleep(delay);
    }
    Ok(())
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' || c == '\n' || tokens.is_empty() {
            let mut token = String::new();
            token.push(c);
            for _ in 0..4 {
                if let Some(&next) = chars.peek() {
                    if next == ' ' || next == '\n' {
                        break;
                    }
                    token.push(chars.next().unwrap());
                }
            }
            tokens.push(token);
        } else {
            tokens.last_mut().unwrap().push(c);
        }
    }
    tokens
}
