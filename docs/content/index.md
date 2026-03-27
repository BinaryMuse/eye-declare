---
title: eye-declare
description: A declarative inline TUI rendering library for Rust
---

# eye-declare

A declarative, React-like TUI rendering library for Rust, built on [Ratatui](https://ratatui.rs).

eye-declare provides a component model for building terminal UIs that render **inline** — content grows into the terminal's native scrollback rather than taking over the full screen. Designed for CLI tools, AI assistants, and interactive prompts where output accumulates and earlier results should remain visible.

```rust
use eye_declare::{element, Application, Elements, Spinner, TextBlock};

struct AppState {
    messages: Vec<String>,
    thinking: bool,
}

fn chat_view(state: &AppState) -> Elements {
    element! {
        #(for msg in &state.messages {
            TextBlock {
                Line { Span(text: msg.clone()) }
            }
        })
        #(if state.thinking {
            Spinner(key: "thinking", label: "Thinking...")
        })
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (mut app, handle) = Application::builder()
        .state(AppState { messages: vec![], thinking: false })
        .view(chat_view)
        .build()?;

    tokio::spawn(async move {
        handle.update(|s| s.messages.push("Hello from eye-declare!".into()));
    });

    app.run().await
}
```

## Key features

- **Inline rendering** — content grows downward into terminal scrollback, like normal CLI output
- **React-like component model** — props, state, reconciliation, and lifecycle hooks
- **`element!` macro** — JSX-like syntax for composing component trees
- **Automatic dirty tracking** — only changed components re-render
- **Frame diffing** — minimal ANSI output, wrapped in [DEC 2026 synchronized output](https://gist.github.com/christianparpart/d8a62cc1ab659194571cd32e81e8406d) to prevent tearing
- **Async-first** — `Handle` sends updates from any thread or async task

## Get started

- [Introduction](/getting-started/introduction) — why eye-declare exists and how it works
- [Installation](/getting-started/installation) — add it to your project
- [Quick Start](/getting-started/quick-start) — build your first inline TUI

## Learn

- [The element! Macro](/guide/element-macro) — full syntax reference
- [Components](/guide/components) — the Component trait and composition patterns
- [Layout](/guide/layout) — vertical and horizontal stacking
- [Lifecycle Hooks](/guide/lifecycle-hooks) — intervals, mount/unmount, autofocus
- [Events and Focus](/guide/events-and-focus) — keyboard handling and focus management
- [Context](/guide/context) — sharing data without prop-drilling
- [Reconciliation](/guide/reconciliation) — how state survives rebuilds
- [Application](/guide/application) — the Application API and running modes

## Reference

- [Built-in Components](/reference/built-in-components) — TextBlock, Spinner, Markdown, VStack, HStack, Column
- [Terminal Options](/reference/terminal-options) — Ctrl+C behavior, keyboard protocols, bracketed paste
- [Imperative API](/reference/imperative-api) — InlineRenderer for direct control

## Status

eye-declare is in early development — expect breaking changes before 1.0.

## License

MIT
