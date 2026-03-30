# eye-declare

[![Crates.io Version](https://img.shields.io/crates/v/eye_declare)](https://crates.io/crates/eye_declare) [![docs.rs](https://img.shields.io/docsrs/eye_declare)](https://docs.rs/eye_declare)

A declarative inline TUI rendering library for Rust, built on [Ratatui](https://ratatui.rs).

eye-declare provides a React-like component model for building terminal UIs that render **inline** — content grows into the terminal's native scrollback rather than taking over the full screen. Designed for CLI tools, AI assistants, and interactive prompts where output accumulates and earlier results should remain visible.

![Demo](https://github.com/BinaryMuse/eye-declare/blob/main/assets/demo.gif?raw=true)

## Status

eye-declare is in early development; expect breaking changes.

Coming changes:

- [ ] More ergonomic "leaf" API
- [ ] Improvements to height measurement and vertical layout

## Installation

Add to your project with:

```bash
cargo add eye_declare
```

## Quick Start

```rust
use eye_declare::{element, Application, ControlFlow, Elements, Spinner};

struct AppState {
    messages: Vec<String>,
    thinking: bool,
}

fn chat_view(state: &AppState) -> Elements {
    element! {
        #(for (i, msg) in state.messages.iter().enumerate() {
            #(msg.clone())
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
            "Dashboard"

            #(for (i, item) in state.items.iter().enumerate() {
                Markdown(key: format!("item-{i}"), source: item.clone())
            })

            #(if state.loading {
                Spinner(label: "Refreshing...")
            })

            #(if let Some(ref err) = state.error {
                Markdown(source: err.clone())
            })

            #(footer_view(state))
        }
    }
}
```

### Syntax reference

| Syntax | Description |
|--------|-------------|
| `Component(prop: value)` | Construct with props (struct field init) |
| `Component { ... }` | Component with children |
| `Component(props) { children }` | Both |
| `"text"` | String literal — auto-wrapped as `Text` |
| `key: expr` | Special prop for stable identity across rebuilds |
| `#(if cond { ... })` | Conditional children |
| `#(if let pat = expr { ... })` | Pattern-matching conditional |
| `#(for pat in iter { ... })` | Loop children |
| `#(expr)` | Splice a pre-built `Elements` value inline |

## Components

The primary way to define components is with `#[component]` and `#[props]`:

```rust
use eye_declare::{component, element, props, Elements, Canvas, View, BorderType};
use ratatui_core::{buffer::Buffer, layout::Rect, style::{Color, Modifier, Style}, text::Line, widgets::Widget};
use ratatui_widgets::paragraph::Paragraph;

/// Props are defined with #[props] on a struct.
#[props]
struct Badge {
    label: String,
    #[default(Color::Green)]
    color: Color,
}

/// The component function receives props (and optionally hooks/state)
/// and returns Elements.
#[component(props = Badge)]
fn badge(props: &Badge) -> Elements {
    let label = props.label.clone();
    let color = props.color;

    element!(
        Canvas(render_fn: move |area: Rect, buf: &mut Buffer| {
            let line = Line::styled(
                format!(" {} ", label),
                Style::default().fg(Color::Black).bg(color),
            );
            Paragraph::new(line).render(area, buf);
        })
    )
}
```

Then use it in a view:

```rust
element! {
    Badge(label: "Online", color: Color::Green)
}
```

### Manual Component impl

For full control, implement the `Component` trait directly. Props live on `&self` (immutable, set by parent). Internal state lives in the associated `State` type (mutable, framework-managed via automatic dirty tracking).

```rust
use eye_declare::Component;
use ratatui_core::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};
use ratatui_widgets::paragraph::Paragraph;

#[derive(Default)]
struct StatusBadge {
    pub label: String,
    pub color: Color,
}

impl Component for StatusBadge {
    type State = (); // no internal state needed

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        let line = Line::from(Span::styled(&self.label, Style::default().fg(self.color)));
        Paragraph::new(line).render(area, buf);
    }
}
```

### Composite Components

Components can accept children using `#[component(children = Elements)]`. Use `View` for layout and chrome:

```rust
#[props]
struct Card {
    title: String,
}

#[component(props = Card, children = Elements)]
fn card(props: &Card, children: Elements) -> Elements {
    element!(
        View(
            border: BorderType::Rounded,
            border_style: Style::default().fg(Color::DarkGray),
            title: props.title.clone(),
            title_style: Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ) {
            #(children)
        }
    )
}
```

Usage with `element!`:

```rust
element! {
    Card(title: "My Card") {
        Spinner(label: "Loading...")
        "Some content"
    }
}
```

Three patterns:
- **Pass through** (default) — VStack, HStack accept external children as-is
- **Generate own tree** — a Spinner builds its own frame + label layout internally
- **Wrap slot** — a Card wraps external children in a header + border

### Lifecycle Hooks

Components declare effects via hooks. In a `#[component]` function, accept `hooks` and optionally `state`:

```rust
#[derive(Default)]
struct TimerState {
    elapsed: u64,
    started_at: Option<Instant>,
}

#[props]
struct Timer {
    running: bool,
}

#[component(props = Timer, state = TimerState)]
fn timer(props: &Timer, state: &TimerState, hooks: &mut Hooks<TimerState>) -> Elements {
    if props.running {
        hooks.use_interval(Duration::from_secs(1), |s| s.elapsed += 1);
    }
    hooks.use_mount(|s| s.started_at = Some(Instant::now()));
    hooks.use_unmount(|s| {
        if let Some(at) = s.started_at {
            println!("Timer ran for {:?}", at.elapsed());
        }
    });

    element! {
        #(format!("Elapsed: {}s", state.elapsed))
    }
}
```

Available hooks:

| Hook | Fires when |
|------|------------|
| `use_interval(duration, handler)` | Periodically, at the given duration |
| `use_mount(handler)` | Once, after the component is first built |
| `use_unmount(handler)` | Once, when the component is removed |
| `use_autofocus()` | Requests focus when the component mounts |
| `use_focusable(bool)` | Declares the component as focusable for Tab cycling |
| `use_cursor(handler)` | Returns cursor position when focused |
| `use_event(handler)` | Handles events in the bubble phase (focused -> root) |
| `use_event_capture(handler)` | Handles events in the capture phase (root -> focused) |
| `use_layout(layout)` | Overrides the component's layout direction |
| `use_width_constraint(constraint)` | Sets width constraint in horizontal layout |
| `use_height_hint(height)` | Declares a fixed height, skipping probe measurement |
| `provide_context(value)` | Makes a value available to all descendants |
| `use_context::<T>(handler)` | Reads a value provided by an ancestor |

### Context

The context system lets ancestor components provide typed values to their descendants without prop-drilling. This is the primary mechanism for connecting components to app-level services.

**Root-level context** — register values on the application builder:

```rust
let (mut app, handle) = Application::builder()
    .state(MyState::default())
    .view(my_view)
    .with_context(event_sender)       // available to all components
    .with_context(AppConfig::new())   // multiple types supported
    .build()?;
```

**Component-level context** — provide and consume in lifecycle:

```rust
// Provider: makes a value available to descendants
fn lifecycle(&self, hooks: &mut Hooks<MyState>, _state: &MyState) {
    hooks.provide_context(self.theme.clone());
}

// Consumer: reads a value from an ancestor
fn lifecycle(&self, hooks: &mut Hooks<MyState>, _state: &MyState) {
    hooks.use_context::<Theme>(|theme, state| {
        state.current_theme = theme.cloned();
    });
}
```

The `use_context` handler always fires with `Option<&T>` — `None` if no ancestor provides the type. Inner providers shadow outer providers of the same type within their subtree.

## Layout

Vertical stacking is the default. `HStack` provides horizontal layout with width constraints:

```rust
use eye_declare::{Elements, HStack, Column, Text};
use eye_declare::WidthConstraint::Fixed;

fn two_column_view(state: &MyState) -> Elements {
    element! {
        HStack {
            Column(width: Fixed(20)) {
                Text { "Sidebar content" }
            }
            Column {
                // Fill: takes remaining space
                Text { "Main content" }
            }
        }
    }
}
```

Components can declare `content_inset()` for borders and padding — children render inside the inset area while the component draws chrome in the full area.

## Reconciliation

Elements are matched by key (stable identity) or position (implicit). State is preserved across rebuilds when nodes are reused:

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

`Application` owns your state and manages the render loop. `Handle` sends updates from any thread or async task:

```rust
let (mut app, handle) = Application::builder()
    .state(MyState::new())
    .view(my_view)
    .build()?;

// Non-interactive: exits when handle is dropped and effects stop
app.run().await?;
```

```rust
// Component-driven interactive: raw mode with context-based event handling
// Components handle their own events and dispatch app-domain actions via channels
app.run_loop().await?;
```

```rust
// Raw interactive: direct access to terminal events (escape hatch)
app.run_interactive(|event, state| {
    // handle terminal events, mutate state
    ControlFlow::Continue
}).await?;
```

Multiple handle updates between frames are batched into a single rebuild.

### Terminal Options

The builder supports configuring terminal protocols for interactive modes:

```rust
Application::builder()
    .state(state)
    .view(view)
    .ctrl_c(CtrlCBehavior::Deliver)         // route Ctrl+C to components (default: Exit)
    .keyboard_protocol(KeyboardProtocol::Enhanced)  // kitty protocol (default: Legacy)
    .bracketed_paste(true)                   // distinguish pastes from typing (default: false)
    .build()?;
```

| Option | Default | Description |
|--------|---------|-------------|
| `ctrl_c` | `Exit` | `Exit` intercepts Ctrl+C; `Deliver` routes it to components |
| `keyboard_protocol` | `Legacy` | `Enhanced` enables kitty protocol for key disambiguation |
| `bracketed_paste` | `false` | Delivers pasted text as `Event::Paste(String)` |

### Committed Scrollback

For long-running apps, content that scrolls into terminal scrollback can be evicted from state via an `on_commit` callback:

```rust
Application::builder()
    .state(state)
    .view(view)
    .on_commit(|committed, state| {
        // `committed.key` identifies which element scrolled off
        state.messages.remove(0);
    })
    .build()?;
```

This is an opt-in performance optimization. Without it, the framework handles all content normally.

## Imperative API

For direct control over the render loop, use `InlineRenderer`:

```rust
use eye_declare::{InlineRenderer, Spinner, VStack, Text};

let mut renderer = InlineRenderer::new(width);
let spinner_id = renderer.push(Spinner::new("Loading..."));

// Mutate state, render, write to stdout
std::thread::sleep(Duration::from_millis(100));
renderer.tick();
let output = renderer.render();
stdout.write_all(&output)?;

// Declarative subtrees via rebuild
let container = renderer.push(VStack);
renderer.rebuild(container, element! {
    "Hello"
});
```

See the `terminal_demo` and `lifecycle` examples for complete sync event loop patterns.

## Built-in Components

| Component | Description |
|-----------|-------------|
| `Text` | Styled text with word wrapping. Accepts `Span` and string children. |
| `Spinner` | Animated Braille spinner with auto-tick. Shows a checkmark when `.done()`. |
| `Markdown` | Headings, bold, italic, inline code, code blocks, and lists. |
| `Canvas` | Raw buffer rendering via a user-provided closure. Direct access to the ratatui `Buffer`. |
| `View` | Unified layout container with optional borders, padding, background, and direction. |
| `VStack` | Vertical container — children stack top-to-bottom. |
| `HStack` | Horizontal container — children lay left-to-right with `WidthConstraint`-based layout. |
| `Column` | Width-constrained wrapper for use inside `HStack`. |

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
cargo run --example data_children   # Typed data children with #[component]
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

The tradeoff is deliberate. Inline rendering is the right model for AI assistants, build tools, and interactive prompts where output accumulates and earlier results should persist in scrollback for the user to review.

**How it works:**

1. **Reconciliation** matches new elements against existing nodes by key or position. State is preserved when nodes are reused, so animations continue seamlessly and internal component state survives rebuilds.

2. **Layout** measures each node's desired height (with word wrapping computed at render time) and allocates widths for horizontal containers. Content insets allow components to declare border/padding chrome while children render inside.

3. **Rendering** produces a Ratatui `Buffer` for each frame. The `InlineRenderer` diffs against the previous frame and emits only changed cells as ANSI escape sequences, wrapped in DEC synchronized output (`?2026h/l`) to prevent tearing.

4. **Growth** is handled by emitting newlines to claim new terminal rows before writing content. Old rows naturally scroll into terminal scrollback.

**Scrollback handling:** When content height exceeds the terminal height, the terminal scrolls rows into scrollback. The framework tracks terminal height and filters diff output to only address visible rows. The `on_commit` callback provides an additional optimization by evicting committed content from application state entirely.

## Crate Structure

```
crates/
  eye_declare/         Main library
  eye_declare_macros/  element! proc macro
```

The macro is behind the `macros` feature flag (enabled by default).

## License

MIT
