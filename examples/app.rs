//! Application wrapper demo.
//!
//! Shows the Application API: state ownership, view function,
//! step API, Handle for cross-thread updates, and run_while_active.
//!
//! Run with: cargo run --example app

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use eye_declare::{Application, Elements, SpinnerEl, TextBlockEl};
use ratatui_core::style::{Color, Modifier, Style};

// ---------------------------------------------------------------------------
// Application state + view
// ---------------------------------------------------------------------------

struct AppState {
    messages: Vec<(String, Style)>,
    thinking: bool,
}

fn app_view(state: &AppState) -> Elements {
    let mut els = Elements::new();

    for (text, style) in &state.messages {
        els.add(TextBlockEl::new().line(text.as_str(), *style));
    }

    if state.thinking {
        els.add(SpinnerEl::new("Processing...")).key("spinner");
    }

    els
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (mut app, handle) = Application::builder()
        .state(AppState {
            messages: vec![],
            thinking: false,
        })
        .view(app_view)
        .build()?;

    let mut stdout = io::stdout();

    // --- Step API: scripted updates ---

    app.update(|s| {
        s.messages.push((
            "Application wrapper demo".into(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
        s.messages.push((
            "Using the step API for scripted updates".into(),
            Style::default().fg(Color::DarkGray),
        ));
    });
    app.flush(&mut stdout)?;
    thread::sleep(Duration::from_millis(800));

    // Add more messages
    app.update(|s| {
        s.messages.push((
            "Starting background work...".into(),
            Style::default().fg(Color::Yellow),
        ));
        s.thinking = true;
    });
    app.flush(&mut stdout)?;

    // --- Handle: cross-thread updates ---

    let h = handle.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(1500));
        h.update(|s| {
            s.thinking = false;
            s.messages.push((
                "✓ Background work complete (from another thread)".into(),
                Style::default().fg(Color::Green),
            ));
        });
    });

    // --- run_while_active: animate until effects stop ---

    // Spinner animates until the background thread sets thinking=false,
    // which removes the spinner element and clears all active effects.
    app.run_while_active(&mut stdout)?;

    // Final flush to render the completion message
    app.flush(&mut stdout)?;

    writeln!(stdout)?;
    Ok(())
}
