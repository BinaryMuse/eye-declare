//! Lifecycle hooks: mount and unmount effects.
//!
//! Demonstrates on_mount and on_unmount handlers that fire when
//! elements enter and leave the tree during reconciliation.
//! Also shows the unified effect system — ticks, mount, and unmount
//! all coexist on the same node.
//!
//! Run with: cargo run --example lifecycle

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use eye_declare::{
    Component, Element, Elements, InlineRenderer, Renderer, SpinnerEl, TextBlockEl, VStack,
};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

use eye_declare::NodeId;

// ---------------------------------------------------------------------------
// A status log component that records lifecycle events
// ---------------------------------------------------------------------------

struct StatusLog;

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

    fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
        state.entries.len() as u16
    }

    fn initial_state(&self) -> StatusLogState {
        StatusLogState {
            entries: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// A task element with mount/unmount lifecycle hooks
// ---------------------------------------------------------------------------

struct TaskEl {
    name: String,
}

impl TaskEl {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Element for TaskEl {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
        let id = renderer.append_child(parent, StatusLog);
        let state = renderer.state_mut::<StatusLog>(id);
        state.log(
            format!("  {} created", self.name),
            Style::default().fg(Color::DarkGray),
        );

        // On mount: log when this task enters the tree
        let name = self.name.clone();
        renderer.on_mount::<StatusLog>(id, move |state| {
            state.log(
                format!("  {} mounted", name),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::ITALIC),
            );
        });

        // On unmount: log when this task leaves the tree
        let name = self.name;
        renderer.on_unmount::<StatusLog>(id, move |state| {
            state.log(
                format!("  {} unmounted", name),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::ITALIC),
            );
        });

        id
    }

    fn update(self: Box<Self>, renderer: &mut Renderer, node_id: NodeId) {
        let state = renderer.state_mut::<StatusLog>(node_id);
        state.log(
            format!("  {} updated", self.name),
            Style::default().fg(Color::Yellow),
        );
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

    els.add(TextBlockEl::new().line(
        format!("Tasks ({})", state.tasks.len()),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));

    for task in &state.tasks {
        els.add(TaskEl::new(task)).key(task.clone());
    }

    if state.processing {
        els.add(SpinnerEl::new("Processing...")).key("spinner");
    }

    els.add(TextBlockEl::new().line("---", Style::default().fg(Color::DarkGray)));

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
