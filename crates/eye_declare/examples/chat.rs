//! Chat assistant demo.
//!
//! A boxed input prompt where you type a message, press Enter,
//! and a streaming "AI response" appears above. As messages
//! accumulate, old ones scroll into terminal scrollback and
//! are committed (evicted from state).
//!
//! Demonstrates: event handling, custom focusable components,
//! content insets (bordered input), streaming via Handle,
//! committed scrollback via on_commit.
//!
//! Run with: cargo run --example chat
//! Press Esc or Ctrl+C to exit.

use std::io;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use eye_declare::{
    Application, Canvas, Component, ControlFlow, Elements, Handle, Hooks, Markdown, TextBlock, View,
};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct AppState {
    messages: Vec<ChatMessage>,
    next_id: u64,
    input: String,
    cursor: usize,
}

impl AppState {
    fn new() -> Self {
        Self {
            messages: vec![],
            next_id: 0,
            input: String::new(),
            cursor: 0,
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

struct ChatMessage {
    id: u64,
    kind: MessageKind,
}

enum MessageKind {
    User(String),
    Assistant { content: String, done: bool },
}

// ---------------------------------------------------------------------------
// InputBox component — bordered text input
// ---------------------------------------------------------------------------

#[derive(Default)]
struct InputBox {
    pub text: String,
    pub cursor: usize,
    pub prompt: String,
}

impl Component for InputBox {
    type State = ();

    fn lifecycle(&self, hooks: &mut Hooks<Self::State>, _state: &Self::State) {
        hooks.use_autofocus();
    }

    fn uses_view(&self) -> bool {
        true
    }

    fn view(&self, _state: &(), _children: Elements) -> Elements {
        // Capture owned data for the Canvas closure
        let text = self.text.clone();

        let mut view_children = Elements::new();
        view_children.add(
            Canvas::new(move |area: Rect, buf: &mut Buffer| {
                if area.width == 0 || area.height == 0 {
                    return;
                }
                let display = if text.is_empty() {
                    Line::from(Span::styled(
                        "Type a message...",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    ))
                } else {
                    Line::from(Span::styled(&text, Style::default().fg(Color::White)))
                };
                Paragraph::new(display).render(area, buf);
            })
            .with_height(1),
        );

        let mut els = Elements::new();
        els.add_with_children(
            View {
                border: Some(eye_declare::BorderType::Plain),
                border_style: Style::default().fg(Color::DarkGray),
                title: Some(format!(" {} ", self.prompt)),
                title_style: Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                padding_left: Some(eye_declare::Cells(1)),
                padding_right: Some(eye_declare::Cells(1)),
                ..View::default()
            },
            view_children,
        );
        els
    }

    fn is_focusable(&self, _state: &()) -> bool {
        true
    }

    fn cursor_position(&self, area: Rect, _state: &()) -> Option<(u16, u16)> {
        // Cursor inside the border, offset by left border + padding
        let col = 2 + self.cursor as u16;
        if col < area.width.saturating_sub(1) {
            Some((col, 1))
        } else {
            Some((area.width.saturating_sub(2), 1))
        }
    }

    fn handle_event(
        &self,
        _event: &crossterm::event::Event,
        _state: &mut eye_declare::Tracked<()>,
    ) -> eye_declare::EventResult {
        // Events handled by the app handler, not here
        eye_declare::EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// StreamingDots — animated indicator while streaming
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StreamingDots;

#[derive(Default)]
struct StreamingDotsState {
    frame: usize,
}

impl Component for StreamingDots {
    type State = StreamingDotsState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let dots = match state.frame % 4 {
            0 => "   ",
            1 => ".  ",
            2 => ".. ",
            _ => "...",
        };
        let line = Line::from(Span::styled(dots, Style::default().fg(Color::DarkGray)));
        Paragraph::new(line).render(area, buf);
    }

    fn initial_state(&self) -> Option<StreamingDotsState> {
        Some(StreamingDotsState { frame: 0 })
    }

    fn lifecycle(&self, hooks: &mut Hooks<StreamingDotsState>, _state: &StreamingDotsState) {
        hooks.use_interval(Duration::from_millis(300), |s| {
            s.frame = s.frame.wrapping_add(1);
        });
    }
}

// ---------------------------------------------------------------------------
// View function
// ---------------------------------------------------------------------------

fn chat_view(state: &AppState) -> Elements {
    let mut els = Elements::new();

    for msg in &state.messages {
        let key = format!("msg-{}", msg.id);
        match &msg.kind {
            MessageKind::User(text) => {
                els.add(
                    TextBlock::new().line(
                        format!("> {}", text),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .key(key);
            }
            MessageKind::Assistant { content, done } => {
                if *done {
                    els.add(Markdown::new(content)).key(key);
                } else if content.is_empty() {
                    els.add(StreamingDots).key(key);
                } else {
                    // Show content with a blinking cursor
                    els.add(Markdown::new(format!("{}▌", content))).key(key);
                }
            }
        }
    }

    // Separator
    els.add(TextBlock::new());

    // Input box
    els.add(InputBox {
        text: state.input.clone(),
        cursor: state.cursor,
        prompt: "You".into(),
    })
    .key("input");

    els
}

// ---------------------------------------------------------------------------
// Streaming response simulation
// ---------------------------------------------------------------------------

const RESPONSES: &[&str] = &[
    "Here's a quick overview of the key concepts:\n\n\
     **Ownership** is Rust's most unique feature. Every value has a single \
     owner, and the value is dropped when the owner goes out of scope. This \
     eliminates the need for a garbage collector.\n\n\
     **Borrowing** lets you reference a value without taking ownership. \
     You can have either one mutable reference or any number of immutable \
     references at a time.\n\n\
     ```rust\nlet s = String::from(\"hello\");\nlet r = &s; // immutable borrow\nprintln!(\"{}\", r);\n```\n\n\
     The borrow checker enforces these rules at compile time, preventing \
     data races and use-after-free bugs entirely.",
    "Here are a few approaches you could consider:\n\n\
     - **Pattern matching** with `match` is exhaustive — the compiler \
     ensures you handle every case\n\
     - **Iterator chains** like `.filter().map().collect()` are zero-cost \
     abstractions that compile to the same code as hand-written loops\n\
     - **Error handling** with `Result<T, E>` and the `?` operator makes \
     error propagation clean and explicit\n\n\
     The Rust compiler is your ally here — lean into its suggestions.",
    "Let me break that down step by step:\n\n\
     ### Step 1: Define the trait\n\n\
     ```rust\ntrait Drawable {\n    fn draw(&self, buf: &mut Buffer);\n}\n```\n\n\
     ### Step 2: Implement for your types\n\n\
     Each type provides its own `draw` implementation. The compiler \
     generates static dispatch when possible.\n\n\
     ### Step 3: Use trait objects for dynamic dispatch\n\n\
     When you need heterogeneous collections, use `Box<dyn Drawable>`. \
     This adds a vtable pointer but enables runtime polymorphism.",
];

async fn stream_response(handle: Handle<AppState>, msg_id: u64) {
    // Pick a response based on message ID
    let response = RESPONSES[(msg_id / 2) as usize % RESPONSES.len()];

    // Small initial delay (thinking)
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Stream word by word
    let words: Vec<&str> = response
        .split_inclusive(|c: char| c.is_whitespace() || c == '\n')
        .collect();
    for word in words {
        let w = word.to_string();
        handle.update(move |state| {
            if let Some(msg) = state.messages.iter_mut().find(|m| m.id == msg_id)
                && let MessageKind::Assistant { content, .. } = &mut msg.kind
            {
                content.push_str(&w);
            }
        });
        // Vary speed for natural feel
        let delay = if word.contains('\n') {
            80
        } else {
            25 + (word.len() as u64 * 5)
        };
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

    // Mark as done
    handle.update(move |state| {
        if let Some(msg) = state.messages.iter_mut().find(|m| m.id == msg_id)
            && let MessageKind::Assistant { done, .. } = &mut msg.kind
        {
            *done = true;
        }
    });
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> io::Result<()> {
    let (mut app, handle) = Application::builder()
        .state(AppState::new())
        .view(chat_view)
        .on_commit(|_, state: &mut AppState| {
            state.messages.remove(0);
        })
        .build()?;

    // Initial build + set focus on the input box
    app.update(|_| {});
    app.flush(&mut io::stdout())?;
    // Find and focus the input
    // let renderer = app.renderer();
    // let container = renderer.children(renderer.root())[0];
    // if let Some(input_id) = renderer.find_by_key(container, "input") {
    //     renderer.set_focus(input_id);
    // }

    let h = handle;
    app.run_interactive(move |event, state| {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        }) = event
        {
            // Ignore Ctrl+key combos (Ctrl+C handled by framework)
            if modifiers.contains(KeyModifiers::CONTROL) {
                return ControlFlow::Continue;
            }

            match code {
                KeyCode::Char(c) => {
                    state.input.insert(state.cursor, *c);
                    state.cursor += c.len_utf8();
                }
                KeyCode::Backspace => {
                    if state.cursor > 0 {
                        state.cursor -= 1;
                        state.input.remove(state.cursor);
                    }
                }
                KeyCode::Left => {
                    state.cursor = state.cursor.saturating_sub(1);
                }
                KeyCode::Right => {
                    if state.cursor < state.input.len() {
                        state.cursor += 1;
                    }
                }
                KeyCode::Enter => {
                    if !state.input.is_empty() {
                        // Add user message
                        let text = std::mem::take(&mut state.input);
                        state.cursor = 0;
                        let user_id = state.next_id();
                        state.messages.push(ChatMessage {
                            id: user_id,
                            kind: MessageKind::User(text),
                        });

                        // Add assistant placeholder
                        let assistant_id = state.next_id();
                        state.messages.push(ChatMessage {
                            id: assistant_id,
                            kind: MessageKind::Assistant {
                                content: String::new(),
                                done: false,
                            },
                        });

                        // Start streaming
                        let h2 = h.clone();
                        tokio::spawn(async move {
                            stream_response(h2, assistant_id).await;
                        });
                    }
                }
                KeyCode::Esc => {
                    return ControlFlow::Exit;
                }
                _ => {}
            }
        }
        ControlFlow::Continue
    })
    .await
}
