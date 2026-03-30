---
title: Introduction
description: What eye-declare is, why it exists, and who it's for
---

# Introduction

eye-declare is a declarative, React-like TUI rendering library for Rust, built on [Ratatui](https://ratatui.rs). It provides a component model for building terminal UIs that render **inline** — content grows into the terminal's native scrollback rather than taking over the full screen.

## Why inline rendering?

Traditional TUI frameworks (Ratatui's `Terminal`, cursive, tui-realm) operate in **full-screen mode**: they claim the entire terminal viewport and redraw it on every frame. This works well for dashboards and editors, but it's the wrong model for tools where output accumulates:

- **AI assistants** — streaming responses should persist in scrollback
- **Build tools** — earlier task output should remain visible
- **Interactive prompts** — submitted answers shouldn't disappear
- **CLI tools** — output should behave like normal terminal programs

eye-declare's inline rendering model treats the terminal like a document that grows downward. New content appears below old content, and earlier rows naturally scroll into the terminal's scrollback buffer. This means your TUI output coexists with regular shell output — no alternate screen, no viewport takeover.

## The programming model

If you've used React, the mental model will feel familiar:

1. **State** — your application data, owned by the framework
2. **View function** — a pure function that maps state to a component tree
3. **Components** — reusable UI building blocks with props and internal state
4. **Reconciliation** — the framework diffs old and new trees, preserving component state across rebuilds

```rust
use eye_declare::{element, Application, Elements, Spinner, Text};

struct AppState {
    messages: Vec<String>,
    loading: bool,
}

fn view(state: &AppState) -> Elements {
    element! {
        #(for msg in &state.messages {
            Text { Span(text: msg.clone()) }
        })
        #(if state.loading {
            Spinner(label: "Working...")
        })
    }
}
```

The `element!` macro provides JSX-like syntax. Components are Rust structs that implement the `Component` trait. State is automatically tracked for dirty detection, so only changed components re-render.

## What eye-declare is not

- **Not a full-screen TUI framework** — if you need a fixed viewport with scrollable panes, use Ratatui directly
- **Not a widget library** — it provides a small set of built-in components; the value is in the rendering model and component system
- **Not stable yet** — the API is evolving; expect breaking changes before 1.0

## Built on Ratatui

eye-declare uses Ratatui's buffer, cell, style, and widget primitives internally. Components render into Ratatui `Buffer`s, and you can use any Ratatui `Widget` inside a component's `render()` method. If you know Ratatui, you already know how to draw things — eye-declare adds the component model, reconciliation, and inline rendering on top.

## Next steps

- [Installation](installation.md) — add eye-declare to your project
- [Quick Start](quick-start.md) — build your first inline TUI
