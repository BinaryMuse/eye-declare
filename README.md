# eye-declare

A declarative inline TUI rendering library for Rust, built on [ratatui](https://ratatui.rs).

eye-declare provides a React-like component model for building terminal UIs that render inline (growing into terminal scrollback) rather than taking over the full screen. Designed for CLI tools, AI assistants, and interactive prompts.

## Status

eye-declare is in early development; expect breaking changes.

Coming changes:

- [ ] Better HStack layout options
- [ ] More ergonomic "leaf" API

## Quick Start

```rust
use eye_declare::{element, Application, Elements, Spinner, TextBlock};
use ratatui_core::style::Style;

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

    // Send updates from any thread or async task
    tokio::spawn(async move {
        handle.update(|s| s.messages.push("Hello from eye_declare!".into()));
    });

    app.run().await
}
```

## The `element!` Macro

The `element!` macro is the primary way to build UIs. It provides JSX-like syntax for composing component trees:

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

- **Props** are set as `Component(prop: value, ...)` — these are struct fields on the component
- **Children** go inside braces: `VStack { ... }`
- **Keys** provide stable identity across rebuilds: `Spinner(key: "s", label: "...")`
- **String literals** are auto-wrapped as `TextBlock`: `"hello"` becomes a single-line text block
- **Control flow** uses `#(if)`, `#(if let)`, and `#(for)` for conditional and repeated elements

## Components

Components are the building blocks. Props live on `&self` (immutable, set by parent). Internal state lives in `State` (mutable, framework-managed via dirty tracking).

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
}
```

Then use it in a view:

```rust
element! {
    StatusBadge(label: "Online".into(), color: Color::Green)
}
```

### Composite Components

Components can generate their own child trees via the `children()` method. The `slot` parameter carries externally-provided children (like React's `props.children`), letting components wrap, replace, or pass through content:

```rust
#[derive(Default)]
struct Card {
    pub title: String,
}

impl Component for Card {
    type State = ();

    fn children(&self, _state: &(), slot: Option<Elements>) -> Option<Elements> {
        let mut els = Elements::new();
        els.add(TextBlock::new().line(&self.title, heading_style));
        if let Some(children) = slot {
            els.group(children); // slot children rendered here
        }
        Some(els)
    }

    fn content_inset(&self, _state: &()) -> Insets {
        Insets::all(1) // border chrome
    }

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        // draw border chrome; children render inside the inset
    }

    // ignored for containers (height is computed from children + insets)
    fn desired_height(&self, _: u16, _: &()) -> u16 { 0 }
}
```

Usage with `element!`:

```rust
element! {
    Card(title: "My Card".into()) {
        Spinner(label: "Loading...")
        "Some content"
    }
}
```

Three patterns:
- **Pass through** (default); ex: VStack, HStack accept external children as-is
- **Generate own tree**; ex: a Spinner builds its own frame + label layout
- **Wrap slot**; ex: a Card wraps external children in a header + border

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

## Layout

Vertical stacking is the default. `HStack` provides horizontal layout, and children declare width constraints via the imperative API:

```rust
let mut els = Elements::new();
els.add(TextBlock::new().lines(sidebar)).width(WidthConstraint::Fixed(20));
els.add(TextBlock::new().lines(content)); // Fill (takes remaining space)
```

Components can declare `content_inset()` for borders and padding — children render inside the inset area while the component draws chrome in the full area.

## Reconciliation

Elements are matched by key (stable identity) or position (implicit). State is preserved across rebuilds when nodes are reused.

```rust
element! {
    // Keyed: survives reordering, state preserved by key
    #(for msg in &state.messages {
        Markdown(key: format!("msg-{}", msg.id), source: msg.content.clone())
    })

    // Positional: matched by index + type
    "Footer"
}
```

## Application

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

## Imperative API

For cases where you need direct control over the render loop (sync event loops, embedding in other frameworks), use `InlineRenderer` directly:

```rust
let mut renderer = InlineRenderer::new(width);
let id = renderer.push(Spinner::new("Loading..."));

// Mutate state, render, write to stdout
renderer.state_mut::<Spinner>(id).tick();
let output = renderer.render();
stdout.write_all(&output)?;

// Rebuild with element! for declarative subtrees
let container = renderer.push(VStack);
renderer.rebuild(container, element! {
    TextBlock(lines: vec![("Hello".into(), Style::default())])
});
```

See the `terminal_demo` and `lifecycle` examples for complete sync event loop patterns.

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
cargo run --example chat            # Interactive chat with streaming
cargo run --example app             # Application wrapper with Handle updates
cargo run --example declarative     # View function pattern with element! macro
cargo run --example lifecycle       # Mount/unmount lifecycle hooks
cargo run --example interactive     # Focus, Tab cycling, text input
cargo run --example terminal_demo   # Sync imperative API with InlineRenderer
cargo run --example agent_sim       # Multi-component agent simulation
cargo run --example markdown_demo   # Markdown rendering showcase
cargo run --example growing         # Dynamically growing content
cargo run --example nested          # Nested component trees
cargo run --example wrapping        # Word wrapping and resize behavior
```

## Architecture

```
Application        State + view function + async event loop
  InlineRenderer   Rendering, reconciliation, layout, diffing, scrollback
    ratatui-core   Buffer, Cell, Style, Widget primitives
    crossterm      Terminal I/O, event types
```

### Inline rendering model

eye-declare uses an **inline rendering model** — content grows downward into the terminal's native scrollback, like standard CLI output. This is fundamentally different from full-screen TUI frameworks (ratatui's `Terminal`, tui-realm, cursive) that redraw a fixed viewport.

The tradeoff is deliberate. Inline rendering is the right model for AI assistants, build tools, and interactive prompts where output accumulates and earlier results should persist in scrollback for the user to review. Full-screen mode would erase that history.

**How it works:**

1. **Reconciliation** matches new elements against existing nodes by key or position. State is preserved when nodes are reused, so animations continue seamlessly and internal component state survives rebuilds.

2. **Layout** measures each node's desired height (with word wrapping computed at render time) and allocates widths for horizontal containers. Content insets allow components to declare border/padding chrome while children render inside.

3. **Rendering** produces a ratatui `Buffer` for each frame. The `InlineRenderer` diffs against the previous frame and emits only changed cells as ANSI escape sequences, wrapped in DEC synchronized output (`?2026h/l`) to prevent tearing.

4. **Growth** is handled by emitting newlines to claim new terminal rows before writing content. Old rows naturally scroll into terminal scrollback.

**Scrollback handling:** When content height exceeds the terminal height, the terminal scrolls rows into scrollback. The framework tracks terminal height and filters diff output to only address visible rows, preventing cursor tracking drift. The `on_commit` callback provides an additional optimization by evicting committed content from application state entirely.

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
