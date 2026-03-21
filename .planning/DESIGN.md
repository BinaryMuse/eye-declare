# eye_declare Target Design

This document captures the target API and architecture for eye_declare,
based on design discussions. It's a north star, not an implementation
plan — we build toward this incrementally.

## Current State (what's built)

- Component trait (leaf rendering, state, events, focus)
- Declarative elements (Element trait, Elements builder, rebuild)
- Reconciliation (keyed + positional matching, state preservation)
- Unified effect system (interval ticks, mount/unmount lifecycle hooks)
- Horizontal + vertical layout (HStack/VStack, WidthConstraint)
- InlineRenderer (growing scrollback), Terminal (event loop wrapper)

## Target Component Trait

```rust
pub trait Component: Send + Sync + 'static {
    type State: Send + Sync + 'static;

    // --- Rendering ---

    /// Render the component's own visual output (chrome, background,
    /// borders, text). For composite components, this draws the
    /// container chrome — children are rendered by the framework
    /// within the content_inset area.
    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State);

    /// How tall this component wants to be at the given width.
    /// For containers with children, the framework adds
    /// content_inset.top + content_inset.bottom to the children's
    /// measured height.
    fn desired_height(&self, width: u16, state: &Self::State) -> u16;

    /// Create the initial state for this component.
    fn initial_state(&self) -> Self::State;

    // --- Composite (optional, all have defaults) ---

    /// Return child elements for this component. Called during
    /// reconciliation. Returning Some means this is a composite
    /// component — the framework builds and reconciles the returned
    /// elements as children of this node.
    ///
    /// The framework memoizes: if the component's props and own state
    /// haven't changed since last reconciliation, children() is not
    /// called and the existing subtree is reused.
    fn children(&self, _state: &Self::State) -> Option<Elements> { None }

    /// Register lifecycle effects (intervals, mount/unmount, etc.).
    /// Called when the node is first created. The Hooks object provides
    /// typed registration methods.
    fn lifecycle(&self, _hooks: &mut Hooks<Self::State>) {}

    /// Insets for the content area within this component's render area.
    /// The framework lays out children inside the inset region.
    /// Use this for borders, padding, margins.
    ///
    /// Default: ZERO (children get the full area).
    fn content_inset(&self, _state: &Self::State) -> Insets { Insets::ZERO }

    // --- Events (TBD — see Event Handling section) ---

    fn handle_event(
        &self, _event: &Event, _state: &mut Self::State,
    ) -> EventResult { EventResult::Ignored }
    fn is_focusable(&self, _state: &Self::State) -> bool { false }
    fn cursor_position(
        &self, _area: Rect, _state: &Self::State,
    ) -> Option<(u16, u16)> { None }
}
```

### Insets

```rust
pub struct Insets {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

impl Insets {
    pub const ZERO: Insets = Insets { top: 0, right: 0, bottom: 0, left: 0 };
    pub fn new() -> Self { ... }
    pub fn all(n: u16) -> Self { ... }
    pub fn symmetric(vertical: u16, horizontal: u16) -> Self { ... }

    pub top(self, top: u16) -> Self { ... }
    pub bottom(self, top: u16) -> Self { ... };
    pub left(self, top: u16) -> Self { ... };
    pub right(self, top: u16) -> Self { ... };
}
```

### Hooks

```rust
pub struct Hooks<S> { /* internal registration state */ }

impl<S: Send + Sync + 'static> Hooks<S> {
    /// Register a periodic callback.
    pub fn use_interval(&mut self, interval: Duration, handler: impl Fn(&mut S) + Send + Sync + 'static);

    /// Register a mount callback (fires once after build).
    pub fn use_mount(&mut self, handler: impl Fn(&mut S) + Send + Sync + 'static);

    /// Register an unmount callback (fires when node is tombstoned).
    pub fn use_unmount(&mut self, handler: impl Fn(&mut S) + Send + Sync + 'static);

    // Future:
    // pub fn use_timeout(&mut self, delay: Duration, handler: ...);
}
```

Hooks are sugar over the unified effect system. Internally, `use_interval`
calls `register_tick`, `use_mount` calls `on_mount`, etc.

## Composite Components

A composite component returns child elements from `children()` instead
of (or in addition to) rendering directly. The framework reconciles
these children as part of the tree.

```rust
struct Spinner {
    label: String,
}

impl Component for Spinner {
    type State = SpinnerState;

    fn children(&self, state: &Self::State) -> Option<Elements> {
        let mut els = Elements::new();
        let mut row = Elements::new();
        let frame = SPINNER_FRAMES[state.frame % SPINNER_FRAMES.len()];
        row.add(TextBlockEl::new().unstyled(frame))
            .width(WidthConstraint::Fixed(2));
        row.add(TextBlockEl::new().unstyled(&self.label));
        els.hstack(row);
        Some(els)
    }

    fn lifecycle(&self, hooks: &mut Hooks<SpinnerState>) {
        hooks.use_interval(Duration::from_millis(80), |state| {
            state.frame = state.frame.wrapping_add(1);
        });
    }

    fn render(&self, _area: Rect, _buf: &mut Buffer, _state: &SpinnerState) {
        // No chrome — children handle all rendering
    }

    fn desired_height(&self, _width: u16, _state: &SpinnerState) -> u16 {
        1
    }

    fn initial_state(&self) -> SpinnerState {
        SpinnerState { frame: 0 }
    }
}
```

### Two paths to children

Nodes get children from ONE of two sources:

1. **External** — via `ElementEntry.children` / `add_with_children`
   Used by: VStack, HStack (layout containers)
2. **Internal** — via `Component::children(state)`
   Used by: Spinner, View, any composite component

If `children()` returns `Some(...)`, those are the canonical children.
ElementEntry.children is ignored for that node.

### Memoization

The framework can skip calling `children()` when:
1. The component's props haven't changed (detected via `update()`)
2. The component's own state hasn't changed (detected via dirty flag)

When skipped, the existing child subtree is reused. Descendants with
their own state changes (e.g., a spinner frame tick) still re-render
via dirty tracking — reconciliation is skipped, not rendering.

## Event Handling

### Three event categories

| Category | Source | Examples | Mechanism |
|----------|--------|----------|-----------|
| **Terminal** | crossterm | Keys, mouse, resize | `handle_event` + focus + bubble |
| **System** | Framework | Tick, mount, unmount, focus gained/lost | Effects / hooks |
| **Domain** | Application | "submit", "message received", "stream chunk" | TBD (see below) |

### Terminal events: focus + bubble (current model, extended)

Terminal events are delivered to the focused component. Unhandled
events bubble up through parents — including through composite
component boundaries. A composite is just another node in the tree;
bubbling passes through it naturally.

### System events: hooks (current model)

System events are lifecycle callbacks managed by the framework:
intervals, mount, unmount. These aren't pub/sub events — they're
registered via hooks and fire at specific lifecycle points.

### Domain events: callbacks + context

Domain events (component signals something application-specific)
use **callback props** as the primary mechanism:

```rust
struct InputEl {
    on_submit: Option<Box<dyn Fn(&str) + Send + Sync>>,
}
```

Callbacks are simple, traceable (follow the call stack), and
debuggable without extra tooling. Evented/bus systems suffer from
debuggability problems that require dedicated tooling to solve.

In a declarative system, prop drilling through view functions is
manageable — it's just another prop on the element. The worst part
is declaring the types in the component struct, which we can make
ergonomic.

**Typed context for deep hierarchies (future):**

When callback drilling becomes unwieldy (deeply nested components
that need to signal distant ancestors), a typed context system
provides an escape hatch:

```rust
// Parent provides a value by type
hooks.provide_context(ChatActions {
    submit: Box::new(|text| { /* ... */ }),
});

// Deep descendant consumes it — no intermediate drilling
let actions = hooks.use_context::<ChatActions>();
actions.submit("hello");
```

Context uses TypeId for lookup — "find the nearest ancestor that
provides a value of this type." It's dependency injection through
the component tree, similar to React Context or Flutter's
InheritedWidget.

Use cases: shared services, theme propagation, navigation,
dispatching actions to a parent-owned store. Context objects could
also signal state changes or trigger rebuilds.

> **For now:** callback props. Context added when drilling becomes
> a real pain point. No event bus planned — callbacks + context
> cover the design space without the debuggability tax.

### Focus management

**`use_autofocus` hook:**
```rust
hooks.use_autofocus();
```
When a node mounts with autofocus, the framework gives it focus.
If multiple nodes mount with autofocus in the same rebuild, the
first one wins. Eliminates imperative `set_focus(id)` calls.

**Focus scopes (gpui-inspired):**

A focus scope creates a boundary for tab cycling. Tab only cycles
within the active scope. This handles modals, popups, and nested
forms without boolean state wiring.

```rust
// A popup traps focus within itself
hooks.use_focus_scope();
```

Scope behavior:
- Tab/Shift-Tab cycle within the scope only
- The scope "traps" focus — can't tab out
- Removing the scope node returns focus to the parent scope
- Scopes nest: a form section within a modal can have its own scope

Implementation: the renderer's `cycle_focus` walks focusable nodes
within the deepest active scope, not the entire tree. Scope
boundaries are marked on nodes (via a hook or component method).

## Application Wrapper

The Application owns state + view function + renderer, eliminating
manual rebuild/render/tick ceremony.

```rust
let mut app = Application::builder()
    .mode(RenderMode::Inline)
    .state(AppState::new())
    .view_fn(chat_view)
    .build()?;

// Mutate state — auto rebuilds and renders
app.set_state(|state| state.thinking = true);

// Framework handles tick loop for active animations
app.run_while_active(Duration::from_millis(1500))?;

// For interactive apps:
app.run(|event, state| {
    // Handle events, mutate state
    // Framework auto-rebuilds and re-renders
    false // return true to exit
})?;
```

### Render modes

```rust
pub enum RenderMode {
    /// Growing inline region. Content scrolls into terminal scrollback.
    Inline,
    // Future:
    // Viewport { height: u16 },  // Fixed-height bounded region with scrolling
    // Popup { rect: Rect },      // Overlay on terminal content
}
```

### Who owns output

Application owns stdout (for its render mode). The builder pattern
allows injecting alternate output targets for testing or embedding.

## Rendering Model

### Three independent passes

| Pass | What | Can skip? |
|------|------|-----------|
| **Reconciliation** | Match elements to nodes (tree structure) | Yes — memoized subtrees, committed scrollback |
| **Layout** | Measure heights, allocate widths (geometry) | Partially — layout invalidation (future) |
| **Rendering** | Produce pixels in buffers | Yes — skip clean leaves via dirty tracking |

### Content inset in layout

```
measure_height(container, width):
    insets = component.content_inset(state)
    inner_width = width - insets.left - insets.right
    children_height = measure_children(inner_width)
    return insets.top + children_height + insets.bottom

render_node(container, area, buffer):
    component.render(area, buffer, state)  // draws chrome
    inner = area adjusted by insets
    layout children within inner
```

### Committed scrollback

In inline mode, content that scrolls into terminal scrollback is
**physically permanent** — the framework can't change it. Nodes
whose content is committed can be dropped from the framework:

- No reconciliation (tree structure finalized)
- No layout (geometry finalized)
- No rendering (pixels in terminal scrollback)
- No memory (node dropped from arena)

The Application manages this automatically based on what the
InlineRenderer has emitted. The user's view function still returns
the full logical state — the framework internally only processes
the active portion.

## Element Macro (future)

```rust
fn chat_view(state: &AppState) -> Elements {
    element! {
        VStack(gap: 1) {
            #(for (i, msg) in state.messages.iter().enumerate() {
                Markdown(key: format!("msg-{i}"), content: msg)
            })
            #(if state.thinking {
                Spinner(key: "thinking", label: "Thinking...")
            })
        }
    }
}
```

The macro is syntax sugar over the Elements builder API.
Everything works without it — it's a developer experience
improvement, not a capability expansion.

## Architecture Layers

```
┌─────────────────────────────────────────────┐
│  Application                                │
│  - Owns state + view_fn + render mode       │
│  - set_state triggers rebuild + render      │
│  - Manages event loop and tick loop         │
│  - Commits scrollback automatically         │
├─────────────────────────────────────────────┤
│  Declarative Layer                          │
│  - Elements + Element trait                 │
│  - Reconciliation (keyed + positional)      │
│  - Composite components (children method)   │
│  - Hooks (lifecycle effects)                │
│  - Memoization (skip unchanged subtrees)    │
├─────────────────────────────────────────────┤
│  Renderer                                   │
│  - Node arena + tree structure              │
│  - Layout (measure + render, V/H/insets)    │
│  - Dirty tracking (Tracked<S>)              │
│  - Effect system (interval, mount, unmount) │
│  - Focus + event delivery                   │
├─────────────────────────────────────────────┤
│  InlineRenderer / ViewportRenderer          │
│  - Terminal output mode                     │
│  - Diff → escape sequences                 │
│  - Cursor management                        │
│  - Scrollback tracking                      │
├─────────────────────────────────────────────┤
│  ratatui-core + crossterm                   │
│  - Buffer, Cell, Rect, Style, Widget        │
│  - Terminal I/O, event types                │
└─────────────────────────────────────────────┘
```

## Implementation Sequence

Building toward this incrementally, each step is independently
useful and testable:

1. ✅ Declarative elements + rebuild
2. ✅ Reconciliation + keying
3. ✅ Tick registration (framework-driven animation)
4. ✅ Unified effect system (mount/unmount)
5. ✅ Horizontal layout (HStack, WidthConstraint)
6. **Content inset** — Insets type, content_inset on Component,
   renderer uses insets for child layout
7. **Composite children** — children() on Component, renderer
   calls during reconciliation, two-path child resolution
8. **Hooks** — Hooks<S> wrapper over effect system, lifecycle()
   on Component
9. **Application wrapper** — owns state + view fn, set_state,
   managed event/tick loop
10. **Committed scrollback** — InlineRenderer drops nodes that
    scroll past, reconciliation skips committed content
11. **Memoization** — skip children() for unchanged composites
12. **element! macro** — JSX-like syntax sugar
