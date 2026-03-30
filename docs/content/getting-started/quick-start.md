---
title: Quick Start
description: Build your first inline TUI with eye-declare in 5 minutes
---

# Quick Start

This guide walks you through building a small application that displays styled messages and an animated spinner — enough to see how the core pieces fit together.

## The application

We'll build a non-interactive app that:

1. Shows a header
2. Adds styled messages over time
3. Shows a spinner while "working"
4. Exits automatically when done

## Define your state

Start with a struct that holds everything your UI needs:

```rust
struct AppState {
    messages: Vec<(String, Style)>,
    thinking: bool,
}
```

## Write a view function

The view function is a pure function from state to UI. It runs on every state change:

```rust
use eye_declare::{element, Elements, Spinner, Text};
use ratatui_core::style::{Color, Modifier, Style};

fn app_view(state: &AppState) -> Elements {
    element! {
        #(for (text, style) in &state.messages {
            Text(style: *style) { Span(text: text.clone()) }
        })
        #(if state.thinking {
            Spinner(label: "Processing...")
        })
    }
}
```

Key things to notice:

- `element!` returns `Elements` — a list of component descriptions
- `#(for ...)` iterates over data and produces components for each item
- `#(if ...)` conditionally includes components
- `"string literals"` are automatically wrapped in `Text`
- Components accept props with struct-init syntax: `Spinner(label: "...")`

## Wire up the Application

`Application` owns your state and manages the render loop. `Handle` lets you send updates from any thread or async task:

```rust
use eye_declare::Application;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (mut app, handle) = Application::builder()
        .state(AppState {
            messages: vec![],
            thinking: false,
        })
        .view(app_view)
        .build()?;

    tokio::spawn(async move {
        // Add a header
        handle.update(|s| {
            s.messages.push((
                "Application demo".into(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        });

        tokio::time::sleep(Duration::from_millis(800)).await;

        // Start "work"
        handle.update(|s| {
            s.messages.push((
                "Starting background work...".into(),
                Style::default().fg(Color::Yellow),
            ));
            s.thinking = true;
        });

        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Finish
        handle.update(|s| {
            s.thinking = false;
            s.messages.push((
                "Done!".into(),
                Style::default().fg(Color::Green),
            ));
        });

        // handle dropped here — app exits when all effects stop
    });

    app.run().await
}
```

## What just happened?

1. `Application::builder()` creates a builder that takes your state and view function
2. `.build()` returns an `Application` and a `Handle`
3. The spawned task sends state updates through the handle — each `update()` call triggers a re-render
4. Multiple updates between frames are batched into a single rebuild
5. `app.run()` runs the event loop until the handle is dropped and all component effects (like the spinner's animation timer) have stopped

## Running the examples

The repository includes several examples that demonstrate different patterns:

```sh
cargo run --example app             # This quick start pattern
cargo run --example declarative     # View function with element! macro
cargo run --example chat            # Interactive chat with streaming
cargo run --example interactive     # Focus, Tab cycling, text input
cargo run --example lifecycle       # Mount/unmount lifecycle hooks
cargo run --example agent_sim       # Multi-component agent simulation
cargo run --example markdown_demo   # Markdown rendering
cargo run --example terminal_demo   # Sync imperative API
```

## Next steps

- [The element! macro](../guide/element-macro.md) — full syntax reference
- [Components](../guide/components.md) — building custom components
- [Application](../guide/application.md) — the full Application API
