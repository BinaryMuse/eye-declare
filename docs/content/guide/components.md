---
title: Components
description: Building custom components with #[component], #[props], and the Component trait
---

# Components

Components are the building blocks of an eye-declare UI. Every piece of your interface — from a single line of styled text to a complex multi-part layout — is a component.

## Function components

The recommended way to define a component is with `#[props]` and `#[component]`:

```rust
use eye_declare::{component, props, Elements, View, Canvas, Hooks};
use ratatui_widgets::borders::BorderType;

#[props]
struct Card {
    title: String,                  // required — compile error if missing
    #[default(true)]
    visible: bool,                  // optional, defaults to true
    border: Option<BorderType>,     // optional, defaults to None
}

#[component(props = Card, children = Elements)]
fn card(props: &Card, children: Elements) -> Elements {
    if !props.visible {
        return Elements::new();
    }
    element! {
        View(border: props.border.unwrap_or(BorderType::Rounded),
             title: props.title.clone()) {
            #(children)
        }
    }
}
```

Then use it in `element!`:

```rust
element! {
    Card(title: "My Card") {
        "Card content goes here"
        Spinner(label: "Loading...")
    }
}
```

### `#[props]`

Defines the component's props struct. Generates a builder with compile-time
required field enforcement:

- Fields **without** `#[default]` are required — omitting them in `element!` is a compile error
- Fields **with** `#[default(expr)]` are optional — the expression is used when not specified
- All setters accept `impl Into<T>`, so `.into()` is rarely needed in `element!`

```rust
#[props]
struct Badge {
    label: String,              // required
    #[default(Color::Green)]
    color: Color,               // optional
}
```

### `#[component]`

Transforms a function into a `Component` impl on the props struct. Attributes:

| Attribute | Required | Description |
|-----------|----------|-------------|
| `props = Type` | Yes | The props struct (becomes the Component type) |
| `state = Type` | No | State type, defaults to `()` |
| `initial_state = expr` | No | Custom initial state (requires `state`) |
| `children = Elements` | No | Slot children: arbitrary element trees from the parent |
| `children = DataChildren<T>` | No | Data children: typed child collection via `Into<T>` |

### Function parameters

The function can take these parameters in order:

1. `props: &PropsType` — the component's props (required)
2. `state: &StateType` — the component's state (if `state` specified)
3. `hooks: &mut Hooks<StateType>` — for declaring behavioral hooks (optional)
4. `children: Elements` — slot children (if `children = Elements`)
5. `children: &DataChildren<T>` — data children by reference (if `children = DataChildren<T>`)

### Stateful component with hooks

```rust
#[derive(Default)]
struct TimerState {
    elapsed: u32,
}

#[props]
struct Timer {
    #[default(true)]
    running: bool,
}

#[component(props = Timer, state = TimerState)]
fn timer(props: &Timer, state: &TimerState, hooks: &mut Hooks<TimerState>) -> Elements {
    if props.running {
        hooks.use_interval(Duration::from_secs(1), |s| s.elapsed += 1);
    }

    element! {
        Canvas(render_fn: {
            let text = format!("Elapsed: {}s", state.elapsed);
            move |area: Rect, buf: &mut Buffer| {
                Paragraph::new(text.as_str()).render(area, buf);
            }
        })
    }
}
```

## Behavioral hooks

In function components, behavioral capabilities are declared through hooks
rather than trait method overrides:

| Hook | Purpose | Equivalent trait method |
|------|---------|----------------------|
| `hooks.use_focusable(true)` | Participate in Tab cycling | `is_focusable()` |
| `hooks.use_cursor(\|area, state\| ...)` | Position cursor when focused | `cursor_position()` |
| `hooks.use_event(\|event, state\| ...)` | Handle events (bubble phase) | `handle_event()` |
| `hooks.use_event_capture(\|event, state\| ...)` | Handle events (capture phase) | `handle_event_capture()` |
| `hooks.use_layout(Layout::Horizontal)` | Set child layout direction | `layout()` |
| `hooks.use_width_constraint(Fixed(n))` | Set width in horizontal parent | `width_constraint()` |
| `hooks.use_height_hint(n)` | Declare fixed height (skip measurement) | `desired_height()` |
| `hooks.use_autofocus()` | Request focus on mount | — |
| `hooks.use_interval(dur, \|state\| ...)` | Periodic callback | — |
| `hooks.use_mount(\|state\| ...)` | Fire on first build | — |
| `hooks.use_unmount(\|state\| ...)` | Fire on removal | — |

### Example: focusable input with cursor

```rust
#[props]
struct InputBox {
    text: String,
    #[default(0usize)]
    cursor: usize,
}

#[component(props = InputBox)]
fn input_box(props: &InputBox, hooks: &mut Hooks<()>) -> Elements {
    hooks.use_focusable(true);
    hooks.use_autofocus();

    let cursor_pos = props.cursor;
    hooks.use_cursor(move |area: Rect, _state: &()| {
        let col = 2 + cursor_pos as u16;
        if col < area.width.saturating_sub(1) {
            Some((col, 1))
        } else {
            Some((area.width.saturating_sub(2), 1))
        }
    });

    let text = props.text.clone();
    element! {
        View(border: BorderType::Plain) {
            Canvas(render_fn: move |area: Rect, buf: &mut Buffer| {
                let display = if text.is_empty() {
                    "Type a message...".to_string()
                } else {
                    text.clone()
                };
                Paragraph::new(display).render(area, buf);
            }, height: 1u16)
        }
    }
}
```

## Props vs. State

eye-declare separates data into two categories:

**Props** are fields on the props struct — immutable data set by the parent.
Use `#[props]` to define them with required/optional field enforcement.

**State** is the associated `State` type — mutable data managed by the framework.
State is automatically wrapped in `Tracked<S>`, which detects mutations and
marks the component dirty for re-rendering.

```rust
#[derive(Default)]
struct CounterState {
    count: u32,
}

#[props]
struct Counter {
    #[default("Count".to_string())]
    label: String,
}

#[component(props = Counter, state = CounterState)]
fn counter(props: &Counter, state: &CounterState) -> Elements {
    element! {
        Canvas(render_fn: {
            let text = format!("{}: {}", props.label, state.count);
            move |area: Rect, buf: &mut Buffer| {
                Paragraph::new(text.as_str()).render(area, buf);
            }
        })
    }
}
```

## Composition patterns

The `view()` method (generated by `#[component]`, or overridden manually)
receives slot children and returns an element tree. Three patterns:

1. **Pass through** (default) — return `children` unchanged. Layout containers
   like `VStack` and `HStack` use this.

2. **Wrap children** — incorporate `children` into a larger tree. A `Card`
   wraps children with borders via `View`.

3. **Generate own tree** — ignore `children` and return custom `Elements`.
   A status badge generates its own Canvas rendering.

## Accepting slot children

For a component to accept children in `element!` braces, it needs slot
children support. With `#[component]`, specify `children = Elements`:

```rust
#[component(props = Panel, children = Elements)]
fn panel(props: &Panel, children: Elements) -> Elements {
    // children contains whatever was in the braces
    children
}
```

Without `#[component]`, use the `impl_slot_children!` macro on a manual
Component impl.

## Accepting data children

Data children let a component accept typed children — useful when the
component needs structured input rather than arbitrary element trees.
Define a child enum with `From` conversions, then use `children = DataChildren<T>`:

```rust
use eye_declare::{component, props, Elements, DataChildren, Canvas};

// Child types
struct Item { label: String, value: String }

enum TableChild { Item(Item) }
impl From<Item> for TableChild {
    fn from(item: Item) -> Self { TableChild::Item(item) }
}

#[props]
struct Table {
    title: String,
}

#[component(props = Table, children = DataChildren<TableChild>)]
fn table(props: &Table, children: &DataChildren<TableChild>) -> Elements {
    // children.as_slice() gives &[TableChild]
    // ... render items
}
```

Usage in `element!`:

```rust
element! {
    Table(title: "Info") {
        Item(label: "OS".into(), value: "macOS".into())
        Item(label: "Rust".into(), value: "1.86".into())
    }
}
```

Data children are collected via `Into<T>` conversions — any type implementing
`Into<TableChild>` can be used inside the braces. Invalid child types produce
a compile error.

Components with data children can also be used without braces:

```rust
element! {
    Table(title: "Empty")   // gets default empty DataChildren
}
```

## Canvas for raw rendering

`Canvas` is a leaf component for direct buffer access. Use it inside
`view()` when you need to render with ratatui widgets:

```rust
element! {
    Canvas(render_fn: |area: Rect, buf: &mut Buffer| {
        Paragraph::new("Hello!").render(area, buf);
    })
}

// With explicit height (skips probe measurement)
element! {
    Canvas(render_fn: |area, buf| { /* draw */ }, height: 3u16)
}
```

Canvas is useful for wrapping third-party ratatui widgets, custom charts,
or any rendering the built-in components don't cover.

## Manual Component impl

For primitive components or advanced use cases, you can implement the
`Component` trait directly. This is how the built-in components (View,
Canvas, Spinner, Text) are implemented:

```rust
#[derive(Default, TypedBuilder)]
struct Badge {
    #[builder(default, setter(into))]
    pub label: String,
    #[builder(default, setter(into))]
    pub color: Color,
}

impl Component for Badge {
    type State = ();

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        let style = Style::default().fg(self.color);
        Paragraph::new(Span::styled(&self.label, style)).render(area, buf);
    }
}
```

Most user components should use `#[props]` + `#[component]` instead.

## Component trait reference

The `Component` trait is an implementation detail — `#[component]` generates
it automatically. Most trait methods are superseded by hooks in function
components. The only method you might encounter directly is `update()`,
which `#[component]` overrides to call your function.

| Method | Default | Used by |
|--------|---------|---------|
| `update()` | chains `lifecycle()` → `view()` | `#[component]` overrides this |
| `view()` | passthrough | Default `update()` calls this |
| `lifecycle()` | no-op | Default `update()` calls this |
| `render()` | no-op | Primitives only (View, Canvas, Text) |
| `desired_height()` | `None` | Primitives only; use `use_height_hint` |
| `content_inset()` | `Insets::ZERO` | Primitives only (View) |
| `handle_event()` | `Ignored` | Use `use_event` hook |
| `handle_event_capture()` | `Ignored` | Use `use_event_capture` hook |
| `is_focusable()` | `false` | Use `use_focusable` hook |
| `cursor_position()` | `None` | Use `use_cursor` hook |
| `layout()` | `Vertical` | Use `use_layout` hook |
| `width_constraint()` | `Fill` | Use `use_width_constraint` hook |
| `initial_state()` | `State::default()` | Use `initial_state = expr` attribute |
