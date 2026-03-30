//! Application wrapper demo.
//!
//! Shows the Application API: state ownership, view function,
//! Handle for async updates, and automatic render loop.
//!
//! Run with: cargo run --example app

use std::io;
use std::time::Duration;

use eye_declare::{Application, Elements, Span, Spinner, Text, element};
use ratatui_core::style::{Color, Modifier, Style};

// ---------------------------------------------------------------------------
// Application state + view
// ---------------------------------------------------------------------------

struct AppState {
    messages: Vec<(String, Style)>,
    thinking: bool,
}

fn app_view(state: &AppState) -> Elements {
    element! {
        #(for (text, style) in &state.messages {
            Text {
                Span(text: text.clone(), style: *style)
            }
        })
        #(if state.thinking {
            Spinner(label: "Processing...")
        })
    }
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

    // All updates flow through the handle. The app manages
    // rendering, ticking, and exits when the handle is dropped
    // and no effects remain.
    tokio::spawn(async move {
        handle.update(|s| {
            s.messages.push((
                "Application wrapper demo".into(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
            s.messages.push((
                "Updates flow through the Handle".into(),
                Style::default().fg(Color::DarkGray),
            ));
        });

        tokio::time::sleep(Duration::from_millis(800)).await;

        handle.update(|s| {
            s.messages.push((
                "Starting background work...".into(),
                Style::default().fg(Color::Yellow),
            ));
            s.thinking = true;
        });

        tokio::time::sleep(Duration::from_millis(1500)).await;

        handle.update(|s| {
            s.thinking = false;
            s.messages.push((
                "✓ Background work complete".into(),
                Style::default().fg(Color::Green),
            ));
            s.messages.push(("".into(), Style::default()));
        });

        // handle dropped here → app exits when effects stop
    });

    app.run().await?;

    println!();
    Ok(())
}
