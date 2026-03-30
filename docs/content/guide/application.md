---
title: Application
description: The Application API, Handle, and running modes
---

# Application

`Application` is the high-level entry point for eye-declare apps. It owns your state, manages the render loop, and provides a `Handle` for sending updates from any thread or async task.

## Building an application

```rust
use eye_declare::Application;

let (mut app, handle) = Application::builder()
    .state(MyState::new())
    .view(my_view)
    .build()?;
```

The builder requires:
- `.state(S)` — your application state
- `.view(fn(&S) -> Elements)` — a function that maps state to UI

`.build()` returns a tuple of `(Application, Handle)`.

## The Handle

`Handle<S>` is the primary way to update state. It's `Clone + Send + Sync`, so you can use it from any thread or async task:

```rust
let handle = handle.clone();
tokio::spawn(async move {
    // Fetch data, process events, etc.
    let result = do_work().await;

    // Update state — triggers a re-render
    handle.update(|state| {
        state.result = Some(result);
        state.loading = false;
    });
});
```

### Batching

Multiple `update()` calls between frames are batched into a single rebuild. This means you can call `update()` rapidly without causing unnecessary re-renders:

```rust
// These might all happen before the next frame
handle.update(|s| s.messages.push(msg1));
handle.update(|s| s.messages.push(msg2));
handle.update(|s| s.messages.push(msg3));
// → one rebuild with all three messages
```

### Exiting

Call `handle.exit()` to stop the event loop, or simply drop the handle. When using `run()`, the app exits when the handle is dropped *and* all component effects (intervals, etc.) have stopped.

## Running modes

### Non-interactive: run()

```rust
app.run().await?;
```

No terminal events are captured. The app runs until:
1. The handle is dropped, AND
2. All component effects (intervals, etc.) have stopped

This is the right choice for output-only applications — streaming displays, progress indicators, build tools.

### Component-driven interactive: run_loop()

```rust
app.run_loop().await?;
```

Enters raw mode and routes terminal events through the component tree:
- Tab/Shift+Tab cycle focus among focusable components
- Events go through two-phase dispatch: capture (root → focused) then bubble (focused → root)
- `handle_event_capture()` fires during capture; `handle_event()` fires during bubble

Use this when your components handle their own input (text fields, buttons, etc.).

### Manual interactive: run_interactive()

```rust
app.run_interactive(|event, state| {
    match event {
        Event::Key(KeyEvent { code: KeyCode::Esc, .. }) => {
            ControlFlow::Exit
        }
        Event::Key(KeyEvent { code: KeyCode::Char(c), .. }) => {
            state.input.push(*c);
            ControlFlow::Continue
        }
        _ => ControlFlow::Continue,
    }
}).await?;
```

Enters raw mode but gives you direct access to every terminal event. You mutate state in the closure; the framework re-renders after each event. Return `ControlFlow::Exit` to stop.

This is an escape hatch for when you need full control over event handling.

## Builder options

### Context

Register typed values available to all components:

```rust
.with_context(event_sender)
.with_context(AppConfig::new())
```

See [Context](context.md) for details.

### Terminal protocols

```rust
.ctrl_c(CtrlCBehavior::Deliver)
.keyboard_protocol(KeyboardProtocol::Enhanced)
.bracketed_paste(true)
```

See [Terminal Options](../reference/terminal-options.md) for details.

### Committed scrollback

For long-running apps, content that scrolls into terminal scrollback can be evicted from state:

```rust
.on_commit(|committed, state| {
    // `committed` has .key and .index identifying what scrolled off
    state.messages.remove(0);
})
```

This is an opt-in performance optimization. Without it, the framework handles everything normally — but your state grows unboundedly. With `on_commit`, you can keep state lean by removing elements that the user can no longer see (they're in terminal scrollback).

The callback fires when the framework detects that a top-level element has been fully scrolled above the visible terminal area. The element's key and index help you identify which state to remove.

## Complete example

Here's the full `app` example demonstrating the Application lifecycle:

```rust
use std::io;
use std::time::Duration;

use eye_declare::{element, Application, Elements, Spinner, Text};
use ratatui_core::style::{Color, Modifier, Style};

struct AppState {
    messages: Vec<(String, Style)>,
    thinking: bool,
}

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

#[tokio::main]
async fn main() -> io::Result<()> {
    let (mut app, handle) = Application::builder()
        .state(AppState {
            messages: vec![],
            thinking: false,
        })
        .view(app_view)
        .build()?;

    tokio::spawn(async move {
        handle.update(|s| {
            s.messages.push((
                "Application demo".into(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
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
                "Done!".into(),
                Style::default().fg(Color::Green),
            ));
        });

        // handle dropped → app exits when effects stop
    });

    app.run().await
}
```
