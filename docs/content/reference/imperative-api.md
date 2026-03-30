---
title: Imperative API
description: Using InlineRenderer for direct control over the render loop
---

# Imperative API

For situations where you need synchronous, direct control over the render loop, eye-declare provides `InlineRenderer`. This is a lower-level API than `Application` — you manage the event loop, timing, and stdout yourself.

## When to use InlineRenderer

- Synchronous applications (no async runtime)
- Embedding eye-declare in an existing event loop
- Fine-grained control over render timing
- Testing and debugging

## Basic usage

```rust
use std::io::{self, Write};
use eye_declare::{InlineRenderer, Spinner, Text};

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut renderer = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    // Push components into the renderer
    let spinner_id = renderer.push(Spinner::new("Loading..."));

    // Render and write to stdout
    let output = renderer.render();
    stdout.write_all(&output)?;
    stdout.flush()?;

    // Animate
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(80));
        renderer.tick(); // fire interval effects
        let output = renderer.render();
        if !output.is_empty() {
            stdout.write_all(&output)?;
            stdout.flush()?;
        }
    }

    Ok(())
}
```

## Core operations

### push

Add a component to the renderer's root:

```rust
let id = renderer.push(Spinner::new("Working..."));
let header_id = renderer.push(Text::styled("Header", style));
```

Returns a `NodeId` for later reference.

### state_mut

Access a component's mutable state. Mutations automatically mark the component dirty:

```rust
let state = renderer.state_mut::<Spinner>(spinner_id);
state.label = "Still working...".into();
```

The type parameter must match the component's `type State`.

### tick

Fire interval effects (advance animations, etc.):

```rust
renderer.tick();
```

Call this regularly (e.g., every 50-80ms) to keep animations running.

### render

Produce ANSI escape sequences for the current frame:

```rust
let output: Vec<u8> = renderer.render();
stdout.write_all(&output)?;
```

Returns an empty `Vec` if nothing changed since the last render.

### rebuild

Replace a container's children with a new `Elements` tree:

```rust
let container = renderer.push(VStack);

// Build declarative subtrees
renderer.rebuild(container, element! {
    "Hello"
    Spinner(label: "Working...")
});

// Later, rebuild with new content
renderer.rebuild(container, element! {
    "Done!"
});
```

This triggers reconciliation — matched children preserve their state.

### freeze

Mark a component as frozen. Frozen components are no longer updated or re-rendered — they remain as static content:

```rust
let header = renderer.push(Text::styled("Header", style));
let output = renderer.render();
stdout.write_all(&output)?;
renderer.freeze(header);
```

This is an optimization for content that won't change. Frozen content remains visible but the renderer stops tracking it.

### resize

Handle terminal resize:

```rust
Event::Resize(new_width, _) => {
    let output = renderer.resize(*new_width);
    stdout.write_all(&output)?;
    stdout.flush()?;
}
```

### set_focus

Set focus to a specific component:

```rust
renderer.set_focus(input_id);
```

### handle_event

Deliver an event through two-phase dispatch (capture then bubble):

```rust
renderer.handle_event(&event);
```

### has_active

Check if any component has active effects (intervals, etc.):

```rust
while renderer.has_active() {
    renderer.tick();
    let output = renderer.render();
    // ...
    std::thread::sleep(Duration::from_millis(50));
}
```

## Complete example

Here's a sync event loop from the `interactive` example:

```rust
fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    // Build UI
    let header = r.push(
        Text::styled("Interactive Demo", Style::default().fg(Color::Cyan)),
    );
    flush(&mut r, &mut stdout)?;
    r.freeze(header);

    let log_id = r.push(MessageLog);
    let input_id = r.push(Input);
    r.set_focus(input_id);
    flush(&mut r, &mut stdout)?;

    // Enter raw mode for keystroke input
    crossterm::terminal::enable_raw_mode()?;

    loop {
        if event::poll(Duration::from_millis(50))? {
            let evt = event::read()?;

            match &evt {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers,
                    kind: KeyEventKind::Press,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => break,

                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    let text = {
                        let state = r.state_mut::<Input>(input_id);
                        let t = state.text.clone();
                        state.text.clear();
                        state.cursor = 0;
                        t
                    };
                    if !text.is_empty() {
                        r.state_mut::<MessageLog>(log_id).push(text);
                    }
                }

                Event::Resize(new_width, _) => {
                    let output = r.resize(*new_width);
                    stdout.write_all(&output)?;
                    stdout.flush()?;
                    continue;
                }

                _ => {
                    r.handle_event(&evt);
                }
            }

            let output = r.render();
            if !output.is_empty() {
                stdout.write_all(&output)?;
                stdout.flush()?;
            }
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}
```

## InlineRenderer vs Application

| | InlineRenderer | Application |
|---|---|---|
| Runtime | Synchronous | Async (Tokio) |
| Event loop | You manage it | Framework manages it |
| State updates | Direct via `state_mut()` | Via `Handle::update()` |
| Event handling | Manual | Automatic or closure-based |
| Best for | Sync apps, embedding, testing | Most applications |
