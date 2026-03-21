//! Declarative view-function pattern for building UIs.
//!
//! This example shows how to use `Elements` and `rebuild` to describe
//! the UI as a function of state, instead of imperative tree manipulation.
//! Spinners animate automatically via the tick registration system —
//! no manual ticking needed.
//!
//! Run with: cargo run --example declarative

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use eye_declare::{Elements, InlineRenderer, MarkdownEl, SpinnerEl, TextBlockEl, VStack};
use ratatui_core::style::{Color, Style};

struct Spinner {
    pub label: String,
}

struct SpinnerState {
    frame: usize,
}

impl Default for SpinnerState {
    fn default() -> Self {
        Self { frame: 0 }
    }
}

impl Spinner {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl Something for Spinner {
    type State = SpinnerState;

    fn lifecycle(&self, hooks: Hooks) {
        hooks.use_interval(Duration::from_millis(80), |state| {
            state.frame = (state.frame + 1) % SPINNER_FRAMES.len();
        });
    }

    fn children(&self) -> Option<Element> {
        Some(element! {
            View(padding: 1, border: BorderStyle::Single) {
                HStack(constraints: [Fixed(2), Fill], gap: 1) {
                    TextBlock(style: bold_white) {
                        format!("{}", SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]),
                    }
                    TextBlock(style: bold_white) {
                        self.label,
                    }
                }
            }
        })
    }

    fn height(&self, _width: u16, state: &Self::State) -> u16 {
        1 // more advanced component would measure
    }
}

// ---------------------------------------------------------------------------
// Application state — user-owned, not framework-managed
// ---------------------------------------------------------------------------

struct AppState {
    thinking: bool,
    messages: Vec<String>,
    tool_running: Option<String>,
}

impl AppState {
    fn new() -> Self {
        Self {
            thinking: false,
            messages: Vec::new(),
            tool_running: None,
        }
    }
}

// ---------------------------------------------------------------------------
// View function: state in, elements out
// ---------------------------------------------------------------------------

fn chat_view(state: &AppState) -> Elements {
    element! {
        VStack(gap: 1) {
            #(for (i, msg) in state.messages.iter().enumerate() {
                Markdown(key: format!("msg-{i}"), content: msg),
            })
            #(if state.thinking {
                Spinner(key: "thinking", label: "Thinking..."),
            })
            #(if let Some(ref tool) = state.tool_running {
                Spinner(key: "tool", label: format!("Running {}...", tool)),
            })
            #(if !state.messages.is_empty() || state.thinking || state.tool_running.is_some() {
                TextBlock(style: Style::default().fg(Color::DarkGray)) {
                    "---",
                }
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Main: simulate an agent conversation
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let mut r = Application::builder()
        .mode(RenderMode::Inline)
        .state(AppState::new())
        .view_fn(chat_view)
        .build()?;

    // --- Phase 1: Thinking ---
    r.set_state(|&mut state| state.thinking = true);

    // TODO: spinner registers timer, application ticks it automatically
    // animate_while_active(&mut r, &mut stdout, Duration::from_millis(1500))?;

    // --- Phase 2: First response ---
    r.set_state(|&mut state| {
        state.thinking = false;
        state.messages.push(
            "Here's a binary search implementation in Rust:\n\n\
         ```rust\n\
         fn binary_search(arr: &[i32], target: i32) -> Option<usize> {\n\
         \x20   let mut low = 0;\n\
         \x20   let mut high = arr.len();\n\
         \x20   while low < high {\n\
         \x20       let mid = low + (high - low) / 2;\n\
         \x20       match arr[mid].cmp(&target) {\n\
         \x20           std::cmp::Ordering::Less => low = mid + 1,\n\
         \x20           std::cmp::Ordering::Greater => high = mid,\n\
         \x20           std::cmp::Ordering::Equal => return Some(mid),\n\
         \x20       }\n\
         \x20   }\n\
         \x20   None\n\
         }\n\
         ```"
            .to_string(),
        );
    });
    thread::sleep(Duration::from_millis(800));

    // --- Phase 3: Tool call ---
    r.set_state(|&mut state| state.tool_running = Some("cargo clippy".to_string()));
    // TODO: Spinner auto-animates

    // --- Phase 4: Tool complete, add follow-up ---
    r.set_state(|&mut state| {
        state.tool_running = None;
        state.messages.push(
            "The implementation passes **clippy** with no warnings. \
         The function takes a sorted slice and a target value, \
         returning `Some(index)` if found or `None` otherwise."
                .to_string(),
        );
    });

    println!();
    Ok(())
}
