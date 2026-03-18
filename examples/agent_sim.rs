use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use eye_delcare::{Component, InlineRenderer, Tracked};
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

/// A spinner with a label. Animates through frames on each state update.
struct Spinner;

struct SpinnerState {
    label: String,
    frame: usize,
    done: bool,
    done_label: Option<String>,
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl Component for Spinner {
    type State = SpinnerState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let line = if state.done {
            let check = Span::styled("✓ ", Style::default().fg(Color::Green));
            let label = Span::styled(
                state.done_label.as_deref().unwrap_or(&state.label),
                Style::default().fg(Color::Green),
            );
            Line::from(vec![check, label])
        } else {
            let spinner = Span::styled(
                format!("{} ", SPINNER_FRAMES[state.frame % SPINNER_FRAMES.len()]),
                Style::default().fg(Color::Cyan),
            );
            let label = Span::styled(
                state.label.as_str(),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            );
            Line::from(vec![spinner, label])
        };
        Paragraph::new(line).render(area, buf);
    }

    fn desired_height(&self, _width: u16, _state: &Self::State) -> u16 {
        1
    }

    fn initial_state(&self) -> SpinnerState {
        SpinnerState {
            label: String::new(),
            frame: 0,
            done: false,
            done_label: None,
        }
    }
}

/// A text block that streams content token by token.
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

    fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
        let visible: String = state.tokens[..state.revealed].join("");
        let count = visible.lines().count();
        // lines() returns 0 for empty string, but we want at least 0 height
        if visible.is_empty() {
            0
        } else if visible.ends_with('\n') {
            count as u16 + 1
        } else {
            count as u16
        }
    }

    fn initial_state(&self) -> StreamingState {
        StreamingState {
            tokens: vec![],
            revealed: 0,
            style: Style::default(),
        }
    }
}

/// A simple static text line (for separators, headers, etc.)
struct StaticLine;

impl Component for StaticLine {
    type State = (String, Style);

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let line = Line::styled(state.0.as_str(), state.1);
        Paragraph::new(line).render(area, buf);
    }

    fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
        if state.0.is_empty() { 0 } else { 1 }
    }

    fn initial_state(&self) -> (String, Style) {
        (String::new(), Style::default())
    }
}

// ---------------------------------------------------------------------------
// Demo
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    // --- Phase 1: User prompt ---
    let prompt_id = r.push(StaticLine);
    {
        let s = r.state_mut::<StaticLine>(prompt_id);
        s.0 = "› How do I implement a binary search in Rust?".into();
        s.1 = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
    }
    flush_render(&mut r, &mut stdout)?;
    r.freeze(prompt_id);
    thread::sleep(Duration::from_millis(400));

    // --- Phase 2: Thinking spinner ---
    let spacer1_id = r.push(StaticLine);
    {
        let s = r.state_mut::<StaticLine>(spacer1_id);
        s.0 = " ".into();
    }
    r.freeze(spacer1_id);

    let think_id = r.push(Spinner);
    {
        let s = r.state_mut::<Spinner>(think_id);
        s.label = "Thinking...".into();
    }

    // Animate the spinner for a bit
    animate_spinner(&mut r, &mut stdout, think_id, Duration::from_millis(1500))?;

    // Complete thinking
    {
        let s = r.state_mut::<Spinner>(think_id);
        s.done = true;
        s.done_label = Some("Thought for 1.5s".into());
    }
    flush_render(&mut r, &mut stdout)?;
    r.freeze(think_id);

    // --- Phase 3: Stream the response ---
    let spacer2_id = r.push(StaticLine);
    {
        let s = r.state_mut::<StaticLine>(spacer2_id);
        s.0 = " ".into();
    }
    r.freeze(spacer2_id);

    let response_id = r.push(StreamingText);
    {
        let s = r.state_mut::<StreamingText>(response_id);
        s.style = Style::default().fg(Color::White);
        s.tokens = tokenize(
            "Here's a binary search implementation in Rust:\n\
             \n\
             ```rust\n\
             fn binary_search(arr: &[i32], target: i32) -> Option<usize> {\n\
             \x20   let mut low = 0;\n\
             \x20   let mut high = arr.len();\n\
             \x20   while low < high {\n\
             \x20       let mid = low + (high - low) / 2;\n\
             \x20       match arr[mid].cmp(&target) {\n\
             \x20           Ordering::Less => low = mid + 1,\n\
             \x20           Ordering::Greater => high = mid,\n\
             \x20           Ordering::Equal => return Some(mid),\n\
             \x20       }\n\
             \x20   }\n\
             \x20   None\n\
             }\n\
             ```\n",
        );
    }

    // Stream tokens
    stream_tokens(&mut r, &mut stdout, response_id, Duration::from_millis(25))?;
    r.freeze(response_id);

    // --- Phase 4: Tool call ---
    let spacer3_id = r.push(StaticLine);
    {
        let s = r.state_mut::<StaticLine>(spacer3_id);
        s.0 = " ".into();
    }
    r.freeze(spacer3_id);

    let tool_id = r.push(Spinner);
    {
        let s = r.state_mut::<Spinner>(tool_id);
        s.label = "Running cargo clippy...".into();
    }

    animate_spinner(&mut r, &mut stdout, tool_id, Duration::from_millis(2000))?;

    {
        let s = r.state_mut::<Spinner>(tool_id);
        s.done = true;
        s.done_label = Some("cargo clippy passed (0 warnings)".into());
    }
    flush_render(&mut r, &mut stdout)?;
    r.freeze(tool_id);

    // --- Phase 5: Final summary ---
    let spacer4_id = r.push(StaticLine);
    {
        let s = r.state_mut::<StaticLine>(spacer4_id);
        s.0 = " ".into();
    }
    r.freeze(spacer4_id);

    let summary_id = r.push(StreamingText);
    {
        let s = r.state_mut::<StreamingText>(summary_id);
        s.style = Style::default().fg(Color::White);
        s.tokens = tokenize(
            "The implementation passes clippy with no warnings. \
             The function takes a sorted slice and a target value, \
             returning the index if found.\n",
        );
    }
    stream_tokens(&mut r, &mut stdout, summary_id, Duration::from_millis(20))?;

    // Final newline
    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn flush_render(r: &mut InlineRenderer, stdout: &mut impl Write) -> io::Result<()> {
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
    id: eye_delcare::NodeId,
    duration: Duration,
) -> io::Result<()> {
    let start = Instant::now();
    let tick = Duration::from_millis(80);
    while start.elapsed() < duration {
        {
            let s = r.state_mut::<Spinner>(id);
            s.frame += 1;
        }
        flush_render(r, stdout)?;
        thread::sleep(tick);
    }
    Ok(())
}

fn stream_tokens(
    r: &mut InlineRenderer,
    stdout: &mut impl Write,
    id: eye_delcare::NodeId,
    delay: Duration,
) -> io::Result<()> {
    let total = {
        let s: &Tracked<StreamingState> = r.state_mut::<StreamingText>(id);
        s.tokens.len()
    };

    for i in 1..=total {
        {
            let s = r.state_mut::<StreamingText>(id);
            s.revealed = i;
        }
        flush_render(r, stdout)?;
        thread::sleep(delay);
    }
    Ok(())
}

/// Split text into small tokens for streaming simulation.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' || c == '\n' {
            // Include whitespace with the next word
            let mut token = String::new();
            token.push(c);
            // Grab the next few chars as a "token"
            for _ in 0..4 {
                if let Some(&next) = chars.peek() {
                    if next == ' ' || next == '\n' {
                        break;
                    }
                    token.push(chars.next().unwrap());
                }
            }
            tokens.push(token);
        } else if tokens.is_empty() {
            // First token
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
            // Append to last token if it doesn't have whitespace
            let last = tokens.last_mut().unwrap();
            last.push(c);
        }
    }
    tokens
}
