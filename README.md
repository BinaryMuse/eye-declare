# eye-declare

A declarative inline TUI rendering library for Rust, built on [ratatui](https://ratatui.rs).

eye-declare provides a React-like component model for building terminal UIs that render inline (growing into terminal scrollback) rather than taking over the full screen. Designed for CLI tools, AI assistants, and interactive prompts.

## Status

eye-declare is in early development; expect breaking changes

## Quick Start

```rust
use eye_declare::{element, Application, Elements, Spinner, TextBlock};
use ratatui_core::style::{Color, Modifier, Style};

struct AppState {
    messages: Vec<String>,
    thinking: bool,
}

fn chat_view(state: &AppState) -> Elements {
    element! {
        #(for (i, msg) in state.messages.iter().enumerate() {
            TextBlock(key: format!("msg-{i}"), lines: vec![
                (msg.clone(), Style::default())
            ])
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

    // Send updates from async tasks
    tokio::spawn(async move {
        handle.update(|s| {
            s.messages.push("Hello from eye_declare!".into());
        });
    });

    app.run().await
}
```

## Features

### Component Model

Components carry their props directly and get automatic reconciliation.

```rust
use eye_declare::Component;

#[derive(Default)]
struct StatusBadge {
    pub label: String,
    pub color: Color,
}

impl Component for StatusBadge {
    type State = (); // no internal state

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        let line = Line::from(Span::styled(&self.label, Style::default().fg(self.color)));
        Paragraph::new(line).render(area, buf);
    }

    fn desired_height(&self, _width: u16, _state: &()) -> u16 { 1 }
    fn initial_state(&self) -> () {}
}
```

Props live on `&self` (immutable, set by parent). Internal state lives in `State` (mutable, framework-managed). The framework handles build, update, and reconciliation automatically.

### Declarative Views with `element!`

The `element!` macro provides JSX-like syntax for building element trees:

```rust
fn dashboard(state: &DashboardState) -> Elements {
    element! {
        VStack {
            TextBlock(lines: vec![("Dashboard".into(), bold_style)])
            #(for (i, item) in state.items.iter().enumerate() {
                Markdown(key: format!("item-{i}"), source: item.clone())
            })
            #(if state.loading {
                Spinner(label: "Refreshing...")
            })
            #(if let Some(ref err) = state.error {
                TextBlock(lines: vec![(err.clone(), error_style)])
            })
        }
    }
}
```

Supports components with props, nested children, string literals (auto-wrapped as `TextBlock`), `#(if)`, `#(if let)`, `#(for)`, and special `key`/`width` props.

### Async Application Wrapper

`Application` owns your state and manages the render loop. `Handle` sends updates from any thread or async task.

```rust
let (mut app, handle) = Application::builder()
    .state(MyState::new())
    .view(my_view)
    .build()?;

// Non-interactive: exits when handle is dropped and effects stop
app.run().await?;

// Interactive: raw mode, event handling, Ctrl+C
app.run_interactive(|event, state| {
    // handle terminal events, mutate state
    ControlFlow::Continue
}).await?;
```

Multiple handle updates between frames are batched into a single rebuild.

### Lifecycle Hooks

Components declare effects via `lifecycle()`. The framework manages registration and cleanup.

```rust
impl Component for Timer {
    fn lifecycle(&self, hooks: &mut Hooks<TimerState>, _state: &TimerState) {
        if self.running {
            hooks.use_interval(Duration::from_secs(1), |s| s.elapsed += 1);
        }
        hooks.use_mount(|s| s.started_at = Instant::now());
        hooks.use_unmount(|s| println!("Timer ran for {:?}", s.started_at.elapsed()));
    }
}
```

### Reconciliation

Elements are matched by key (stable identity) or position (implicit). State is preserved across rebuilds when nodes are reused.

```rust
// Keyed: survives reordering
els.add(Markdown::new(&msg.content)).key(format!("msg-{}", msg.id));

// Positional: matched by index + type
els.add(TextBlock::new().unstyled("Footer"));
```

### Layout

Vertical stacking (default) and horizontal layout with width constraints:

```rust
element! {
    HStack {
        TextBlock(width: WidthConstraint::Fixed(20), lines: sidebar_lines)
        TextBlock(lines: main_content) // Fill (takes remaining space)
    }
}
```

Components can declare `content_inset()` for borders and padding — children render inside the inset area while the component draws chrome in the full area.

### Committed Scrollback

For long-running apps, content that scrolls into terminal scrollback can be evicted from state via an `on_commit` callback:

```rust
Application::builder()
    .state(state)
    .view(view)
    .on_commit(|_committed, state| {
        state.messages.remove(0); // evict from front
    })
    .build()?;
```

This is an opt-in performance optimization. Without it, the framework handles all content normally.

## Built-in Components

| Component | Description |
|-----------|-------------|
| `TextBlock` | Styled text with display-time word wrapping |
| `Spinner` | Animated spinner with auto-tick via lifecycle hooks |
| `Markdown` | Headings, bold, italic, code, lists, code blocks |
| `VStack` | Vertical container (children stack top-to-bottom) |
| `HStack` | Horizontal container with width constraints |

## Examples

```sh
cargo run --example chat          # Interactive chat assistant with streaming
cargo run --example app           # Application wrapper with Handle updates
cargo run --example declarative   # View function pattern
cargo run --example lifecycle     # Mount/unmount lifecycle hooks
```

## Architecture

```
Application          State + view function + async event loop
  Renderer           Node arena, reconciliation, layout, effects
    InlineRenderer   Terminal output, frame diffing, scrollback
      ratatui-core   Buffer, Cell, Style, Widget primitives
        crossterm    Terminal I/O, event types
```

### Inline rendering model

eye-declare uses an **inline rendering model** — content grows downward into the terminal's native scrollback, like standard CLI output. This is fundamentally different from full-screen TUI frameworks (ratatui's `Terminal`, tui-realm, cursive) that redraw a fixed viewport.

The tradeoff is deliberate. Inline rendering is the right model for AI assistants, build tools, and interactive prompts where output accumulates and earlier results should persist in scrollback for the user to review. Full-screen mode would erase that history.

**How it works:**

1. **Reconciliation** matches new elements against existing nodes by key or position. State is preserved when nodes are reused, so animations continue seamlessly and internal component state survives rebuilds.

2. **Layout** measures each node's desired height (with word wrapping computed at render time) and allocates widths for horizontal containers. Content insets allow components to declare border/padding chrome while children render inside.

3. **Rendering** produces a ratatui `Buffer` for each frame. The `InlineRenderer` diffs against the previous frame and emits only changed cells as ANSI escape sequences, wrapped in DEC synchronized output (`?2026h/l`) to prevent tearing.

4. **Growth** is handled by emitting newlines to claim new terminal rows before writing content. Old rows naturally scroll into terminal scrollback.

**Known limitation:** When content height exceeds the terminal height, the terminal itself scrolls — an event invisible to the application. This can cause cursor tracking drift during rapid updates with many concurrent animations. For long-running interactive sessions, a viewport renderer (fixed-height region with internal scrolling) would solve this; it's planned but not yet implemented. The `on_commit` callback mitigates the issue by evicting content from state before it grows too tall.

### Design documents

The `.planning/` directory contains the research and design documents that guided these decisions, including the target API design, event handling strategy, and implementation sequence.

## Crate Structure

```
crates/
  eye_declare/         Main library
  eye_declare_macros/  element! proc macro
```

The macro is behind the `macros` feature flag (enabled by default).

## License

MIT
