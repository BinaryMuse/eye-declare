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
    Application, BorderType, Canvas, Cells, ControlFlow, Elements, Handle, Hooks, Markdown, Span,
    Text, View, component, element, props,
};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
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
// InputBox component — bordered text input using #[component]
// ---------------------------------------------------------------------------

#[props]
struct InputBox {
    text: String,
    #[default(0usize)]
    cursor: usize,
    #[default("".to_string())]
    prompt: String,
}

#[component(props = InputBox)]
fn input_box(props: &InputBox, hooks: &mut Hooks<()>) -> Elements {
    hooks.use_autofocus();
    hooks.use_focusable(true);

    let cursor_pos = props.cursor;
    hooks.use_cursor(move |area: Rect, _state: &()| {
        let col = 2 + cursor_pos as u16;
        if col < area.width.saturating_sub(1) {
            Some((col, 1))
        } else {
            Some((area.width.saturating_sub(2), 1))
        }
    });

    let text = props.text.clone();

    element! {
        View(
            border: BorderType::Plain,
            border_style: Style::default().fg(Color::DarkGray),
            title: format!(" {} ", props.prompt),
            title_style: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            padding_left: Some(Cells(1)),
            padding_right: Some(Cells(1)),
        ) {
            Canvas(render_fn: move |area: Rect, buf: &mut Buffer| {
                if area.width == 0 || area.height == 0 {
                    return;
                }
                let display = if text.is_empty() {
                    Line::from(ratatui_core::text::Span::styled(
                        "Type a message...",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    ))
                } else {
                    Line::from(ratatui_core::text::Span::styled(
                        &text,
                        Style::default().fg(Color::White),
                    ))
                };
                Paragraph::new(display).render(area, buf);
            }, height: 1u16)
        }
    }
}

// ---------------------------------------------------------------------------
// StreamingDots — animated indicator while streaming
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StreamingDotsState {
    frame: usize,
}

#[props]
struct StreamingDots {}

#[component(props = StreamingDots, state = StreamingDotsState)]
fn streaming_dots(
    _props: &StreamingDots,
    state: &StreamingDotsState,
    hooks: &mut Hooks<StreamingDotsState>,
) -> Elements {
    hooks.use_interval(Duration::from_millis(300), |s| {
        s.frame = s.frame.wrapping_add(1);
    });

    let dots = match state.frame % 4 {
        0 => "   ",
        1 => ".  ",
        2 => ".. ",
        _ => "...",
    };
    let dots = dots.to_string();

    element! {
        Canvas(render_fn: move |area: Rect, buf: &mut Buffer| {
            let line = Line::from(ratatui_core::text::Span::styled(&dots, Style::default().fg(Color::DarkGray)));
            Paragraph::new(line).render(area, buf);
        })
    }
}

// ---------------------------------------------------------------------------
// View function
// ---------------------------------------------------------------------------

fn chat_view(state: &AppState) -> Elements {
    element! {
        #(for msg in &state.messages {
            #(message_element(msg))
        })

        Text { "" }

        InputBox(key: "input", text: state.input.clone(), cursor: state.cursor, prompt: "You")
    }
}

fn message_element(msg: &ChatMessage) -> Elements {
    let key = format!("msg-{}", msg.id);
    match &msg.kind {
        MessageKind::User(text) => {
            element! {
                Text(key: key) {
                    Span(text: format!("> {}", text), style: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                }
            }
        }
        MessageKind::Assistant { content, done } => {
            if *done {
                element! { Markdown(key: key, source: content.clone()) }
            } else if content.is_empty() {
                element! { StreamingDots(key: key) }
            } else {
                element! { Markdown(key: key, source: format!("{}▌", content)) }
            }
        }
    }
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
    let response = RESPONSES[(msg_id / 2) as usize % RESPONSES.len()];

    tokio::time::sleep(Duration::from_millis(500)).await;

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
        let delay = if word.contains('\n') {
            80
        } else {
            25 + (word.len() as u64 * 5)
        };
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

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

    app.update(|_| {});
    app.flush(&mut io::stdout())?;

    let h = handle;
    app.run_interactive(move |event, state| {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        }) = event
        {
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
                        let text = std::mem::take(&mut state.input);
                        state.cursor = 0;
                        let user_id = state.next_id();
                        state.messages.push(ChatMessage {
                            id: user_id,
                            kind: MessageKind::User(text),
                        });

                        let assistant_id = state.next_id();
                        state.messages.push(ChatMessage {
                            id: assistant_id,
                            kind: MessageKind::Assistant {
                                content: String::new(),
                                done: false,
                            },
                        });

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
