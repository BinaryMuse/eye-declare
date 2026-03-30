---
title: Installation
description: How to add eye-declare to your Rust project
---

# Installation

## Requirements

- Rust 1.85+ (2024 edition)
- A terminal emulator that supports ANSI escape sequences (virtually all modern terminals)

## Add the dependency

```sh
cargo add eye_declare
```

Or add it to your `Cargo.toml` manually:

```toml
[dependencies]
eye_declare = "0.1"
```

## Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `macros` | Enabled | The `element!` proc macro for JSX-like component trees |

The `macros` feature is enabled by default. If you only need the imperative API, you can disable it:

```toml
[dependencies]
eye_declare = { version = "0.1", default-features = false }
```

## Companion crates

eye-declare re-exports the types you need from its dependencies, but you may want direct access for advanced usage:

| Crate | Purpose |
|-------|---------|
| `ratatui-core` | `Buffer`, `Rect`, `Style`, `Color`, `Modifier`, `Widget` — the drawing primitives used inside `render()` |
| `ratatui-widgets` | `Paragraph`, `Block`, and other widgets you can use inside components |
| `crossterm` | Terminal event types (`Event`, `KeyEvent`, `KeyCode`) for `handle_event()` |

```toml
[dependencies]
eye_declare = "0.1"
ratatui-core = "0.1"
ratatui-widgets = "0.1"
crossterm = "0.28"
```

## For async applications

The `Application` API uses Tokio for its async event loop:

```toml
[dependencies]
eye_declare = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Verify the installation

Create a minimal example to confirm everything works:

```rust
use eye_declare::{element, Application, Elements};

struct State;

fn view(_state: &State) -> Elements {
    element! { "Hello from eye-declare!" }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (mut app, _handle) = Application::builder()
        .state(State)
        .view(view)
        .build()?;
    app.run().await
}
```

```sh
cargo run
```

You should see "Hello from eye-declare!" printed inline in your terminal.
