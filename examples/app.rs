//! Application wrapper demo.
//!
//! Shows the Application API: state ownership, view function,
//! step API, Handle for async updates, and run_while_active.
//!
//! Run with: cargo run --example app

use std::io::{self, Write};
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

#[tokio::main]
async fn main() -> io::Result<()> {
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
    tokio::time::sleep(Duration::from_millis(800)).await;

    // Add more messages
    app.update(|s| {
        s.messages.push((
            "Starting background work...".into(),
            Style::default().fg(Color::Yellow),
        ));
        s.thinking = true;
    });
    app.flush(&mut stdout)?;

    // --- Handle: async task updates ---

    let h = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        h.update(|s| {
            s.thinking = false;
            s.messages.push((
                "✓ Background work complete (from async task)".into(),
                Style::default().fg(Color::Green),
            ));
        });
    });

    // --- run_while_active: animate until effects stop ---

    // Spinner animates until the async task sets thinking=false,
    // which removes the spinner element and clears all active effects.
    app.run_while_active(&mut stdout).await?;

    tokio::time::sleep(Duration::from_millis(1000)).await;

    app.update(|s| {
        s.thinking = true;
    });

    app.run_while_active(&mut stdout).await?;

    tokio::time::sleep(Duration::from_millis(3000)).await;

    // Final flush to render the completion message
    app.flush(&mut stdout)?;

    writeln!(stdout)?;
    Ok(())
}
