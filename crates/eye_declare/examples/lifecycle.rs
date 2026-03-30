//! Lifecycle hooks: mount and unmount effects.
//!
//! Demonstrates the hooks system — components declare their effects
//! via lifecycle(), and the framework manages them automatically.
//! Mount fires when elements enter the tree, unmount when they leave.
//!
//! Run with: cargo run --example lifecycle

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use eye_declare::{Component, Elements, Hooks, InlineRenderer, Spinner, Text, VStack};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

// ---------------------------------------------------------------------------
// A status log component that records lifecycle events.
// `name` is a prop on the component; `entries` is internal state.
// ---------------------------------------------------------------------------

struct StatusLog {
    name: String,
}

impl StatusLog {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[derive(Default)]
struct StatusLogState {
    entries: Vec<(String, Style)>,
}

impl StatusLogState {
    fn log(&mut self, msg: impl Into<String>, style: Style) {
        self.entries.push((msg.into(), style));
    }
}

impl Component for StatusLog {
    type State = StatusLogState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let lines: Vec<Line> = state
            .entries
            .iter()
            .map(|(text, style)| Line::from(Span::styled(text.as_str(), *style)))
            .collect();
        Paragraph::new(lines).render(area, buf);
    }

    fn initial_state(&self) -> Option<StatusLogState> {
        let mut state = StatusLogState {
            entries: Vec::new(),
        };
        if !self.name.is_empty() {
            state.log(
                format!("  {} created", self.name),
                Style::default().fg(Color::DarkGray),
            );
        }
        Some(state)
    }

    fn lifecycle(&self, hooks: &mut Hooks<StatusLogState>, _state: &StatusLogState) {
        if !self.name.is_empty() {
            let mount_name = self.name.clone();
            hooks.use_mount(move |state| {
                state.log(
                    format!("  {} mounted", mount_name),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::ITALIC),
                );
            });

            let unmount_name = self.name.clone();
            hooks.use_unmount(move |state| {
                state.log(
                    format!("  {} unmounted", unmount_name),
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::ITALIC),
                );
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct AppState {
    tasks: Vec<String>,
    processing: bool,
}

fn task_view(state: &AppState) -> Elements {
    let mut els = Elements::new();

    els.add(Text::styled(
        format!("Tasks ({})", state.tasks.len()),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));

    for task in &state.tasks {
        els.add(StatusLog::new(task)).key(task.clone());
    }

    if state.processing {
        els.add(Spinner::new("Processing...")).key("spinner");
    }

    els.add(Text::styled("---", Style::default().fg(Color::DarkGray)));

    els
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    let container = r.push(VStack);
    let mut state = AppState {
        tasks: vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
        processing: false,
    };

    // Initial build — all three tasks mount
    r.rebuild(container, task_view(&state));
    flush(&mut r, &mut stdout)?;
    thread::sleep(Duration::from_millis(1000));

    // Remove "Beta" — triggers unmount for Beta, others stay
    state.tasks.retain(|t| t != "Beta");
    r.rebuild(container, task_view(&state));
    flush(&mut r, &mut stdout)?;
    thread::sleep(Duration::from_millis(1000));

    // Add "Delta" — triggers mount for Delta, Alpha & Gamma get updated
    state.tasks.push("Delta".into());
    r.rebuild(container, task_view(&state));
    flush(&mut r, &mut stdout)?;
    thread::sleep(Duration::from_millis(1000));

    // Start processing — spinner mounts with auto-tick
    state.processing = true;
    r.rebuild(container, task_view(&state));
    // Let spinner animate
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(1500) && r.has_active() {
        r.tick();
        flush(&mut r, &mut stdout)?;
        thread::sleep(Duration::from_millis(50));
    }

    // Clear all tasks — everything unmounts
    state.tasks.clear();
    state.processing = false;
    r.rebuild(container, task_view(&state));
    flush(&mut r, &mut stdout)?;

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
