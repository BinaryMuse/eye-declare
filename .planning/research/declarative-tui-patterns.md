# Declarative & Reactive UI Patterns in Rust TUI Libraries

*Research conducted: 2026-03-18*

---

## Research Summary

The Rust TUI ecosystem has fragmented into two camps: the dominant **immediate-mode** approach exemplified by Ratatui, and an emerging set of **declarative/reactive** frameworks that borrow from React, Elm, Vue, and SwiftUI. The most mature declarative option is **iocraft** (actively maintained, React-style hooks + `element!` macro, flexbox via taffy). **Cursive** provides a retained-mode view tree closer to traditional widget toolkits. **tui-realm** layers an Elm-like MVU pattern on top of Ratatui. Dioxus TUI (rink) has been discontinued. Several newer experimental libraries (rxtui, revue) exist but are early-stage.

---

## 1. Dioxus TUI / Rink

### History and Status

Dioxus is a React-inspired, renderer-agnostic Rust UI framework. It originally shipped a terminal renderer called **rink**, later rebranded as `dioxus-tui`. The package was:

- **Archived**: The standalone `rink` repo was archived June 2023
- **Relocated**: Briefly moved to `dioxus/packages/tui` and then into the Blitz project
- **Discontinued**: As of Dioxus v0.5, the TUI package was removed. Maintainer Evan Almloff confirmed: "The TUI renderer was briefly moved into blitz, but was removed with the blitz rewrite."
- **Last stable crates.io version**: `dioxus-tui` 0.4.3, roughly 2 years old
- **Future**: "Forks are welcome if anyone is interested in maintaining a version of `dioxus-tui`. Once Dioxus and Blitz are more stable, we might be able to restore dioxus-tui backed by the blitz representation of the dom/layout/styles."

Notably, the Dioxus team's own `dx` CLI tool now uses **Ratatui** instead of dioxus-tui.

### Component Model

Dioxus uses **function components** with a `cx: Scope` parameter (older API) or hooks directly (newer API). The `rsx!` macro provides JSX-style declarative UI composition.

```rust
fn app(cx: Scope) -> Element {
    cx.render(rsx! {
        div {
            width: "100%",
            height: "10px",
            background_color: "red",
            justify_content: "center",
            align_items: "center",
            "Hello world!"
        }
    })
}

fn main() {
    dioxus_tui::launch(app);
}
```

### Layout Model

Flexbox-based, powered by the Taffy layout library. CSS-style properties (`width`, `height`, `justify_content`, `align_items`, `flex_direction`, etc.) were specified directly in the `rsx!` block as string values. Terminals only render a subset of what a browser would, so some CSS behavior was quirky.

### State Management

Used Dioxus hooks: `use_state`, `use_ref`, `use_future`, `use_effect` — all the standard React-equivalent hooks from the Dioxus ecosystem.

### Event Handling

Had a built-in focus system. Keyboard events could be handled through Dioxus event handlers on elements.

### Key Takeaways

- Proved the concept of a React/JSX model for TUI works in principle
- CSS string-based property values (e.g., `"100%"`, `"10px"`) are ergonomically awkward in Rust
- Terminal constraints meant HTML/CSS didn't map cleanly — this seems to be a fundamental tension
- Abandoned in favor of Ratatui even by the Dioxus team themselves

---

## 2. Cursive

**Repository**: https://github.com/gyscos/cursive
**Maintenance**: Actively maintained. Latest release (cursive-core v0.4.6) October 2025, 1,786 commits, 50 releases, 110 contributors.

### Architecture Overview

Cursive is a **retained-mode, callback-driven** TUI library. The design is more traditional widget-toolkit than functional/reactive:

1. Setup phase: build the view tree imperatively using composable view constructors
2. Register callbacks on views
3. Call `siv.run()` to enter the event loop
4. Callbacks receive `&mut Cursive` to mutate the running application

```rust
use cursive::views::{Dialog, TextView};

fn main() {
    let mut siv = cursive::default();
    siv.add_layer(
        Dialog::around(TextView::new("Hello Dialog!"))
            .title("Cursive")
            .button("Quit", |s| s.quit())
    );
    siv.run();
}
```

### The View Trait

The `View` trait is the core abstraction. Only `draw` is required; all other methods have default no-op implementations:

| Method | Required? | Purpose |
|--------|-----------|---------|
| `draw(&self, printer: &Printer)` | **Yes** | Render to the terminal using Printer |
| `layout(&mut self, constraint: XY<usize>)` | No | Called after size is determined; configure children |
| `needs_relayout(&self) -> bool` | No | Signal that layout recomputation is needed |
| `required_size(&mut self, constraint: XY<usize>) -> XY<usize>` | No | Report minimum size needs |
| `on_event(&mut self, event: Event) -> EventResult` | No | Handle keyboard/mouse input |
| `call_on_any(&mut self, selector, callback)` | No | Find and operate on nested views by name |
| `focus_view(&mut self, selector) -> Result<...>` | No | Move focus to a named view |
| `take_focus(&mut self, source: Direction) -> Result<...>` | No | Accept focus from Tab/arrow navigation |
| `important_area(&self, view_size) -> Rect` | No | Define priority visible region (for scroll) |
| `type_name(&self) -> &'static str` | No | Runtime type info |

The `Printer` provides a drawing interface bounded to the view's allocated area. Views cannot draw outside their bounds.

### Layout Model

Layout is a **two-phase negotiation**:

1. **Size negotiation**: Parent calls `required_size()` on children to learn their minimum sizes; children communicate constraints (fixed vs. flexible)
2. **Finalization**: Parent calls `layout()` on each child with the final allocated size — at this point the size is final and non-negotiable

This is similar to the classic "measure/layout" pass (Android, SwiftUI) or the CSS box model. It's synchronous and explicit.

Key built-in layout views:
- `LinearLayout` — arranges children horizontally or vertically
- `StackView` — layered views (like a navigation stack)
- `ScrollView` — wraps a view with scrolling
- `ResizedView` — forces a view to a given size
- `Layer` — a transparent overlay layer

### Composition Model

Views compose via **wrapper structs and builder patterns**:

```rust
LinearLayout::vertical()
    .child(TextView::new("Name:"))
    .child(EditView::new().on_submit(|s, text| { /* ... */ }))
    .child(Button::new("OK", |s| s.quit()))
```

Extension traits provide chainable decoration:
- `.with_name("my_view")` (from `Nameable`) — enables `find_name::<ViewType>("my_view")`
- `.scrollable()` (from `Scrollable`) — wraps in ScrollView
- `.resized(SizeConstraint::Full, SizeConstraint::Fixed(5))` (from `Resizable`)

There is also `ViewWrapper` — a trait for creating thin wrappers around existing views, delegating most methods to the inner view.

### Event Handling

Cursive is **callback-driven**. Events flow:
1. Raw input is captured from the terminal backend
2. Forwarded to the currently focused view via `on_event()`
3. View returns `EventResult::Consumed(callback)` or `EventResult::Ignored`
4. If ignored, parent container tries to handle it (e.g., `LinearLayout` cycles focus on Tab)

Focus management:
- Only one view is focused at a time
- Tab key cycles focus through focusable views
- `take_focus()` determines if a view is willing to accept focus
- View groups handle Tab by shifting focus among children

The `OnEventView` wrapper allows adding event handlers to any view without implementing a custom type:
```rust
OnEventView::new(my_view)
    .on_event(Key::F1, |s| { /* show help */ })
```

### State Management

Cursive has **no built-in reactive state**. State is managed imperatively:
- Callbacks capture `&mut Cursive` and call `find_name()` to get mutable view references
- Or state lives outside Cursive in a `Mutex<MyState>` captured in closures
- `siv.set_user_data(my_state)` / `siv.user_data::<MyState>()` for app-level state

### Key Takeaways

- Traditional retained-mode widget toolkit feel
- Good for dialog-heavy applications with fixed widget sets
- No reactivity — state changes require manually finding and updating views by name
- Strong focus/keyboard navigation model built in
- Not composable in a functional sense — you mutate the tree, not return new trees
- Familiarity for anyone who's used GTK or Qt

---

## 3. iocraft

**Repository**: https://github.com/ccbrown/iocraft
**Crates.io**: https://crates.io/crates/iocraft
**Maintenance**: Actively maintained. Latest release (v0.7.18) February 19, 2026, 53 total releases, 240 commits.

### Architecture Overview

iocraft is explicitly React-inspired, describing itself as having "a React-like declarative style, as opposed to Ratatui's immediate mode style." Inspired by both **Dioxus** and **Ink** (the Node.js React renderer for terminals). Uses **Taffy** for flexbox layout.

Key differentiators:
- Works for both interactive TUI apps *and* simple one-shot formatted output (like a rich `println!`)
- Components accept props by reference (avoiding cloning)
- Async-first: `use_future` for async side effects

### Component Definition

Components are Rust functions decorated with `#[component]`. They receive a `Hooks` parameter and return `impl Into<AnyElement<'static>>`:

```rust
#[component]
fn Counter(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut count = hooks.use_state(|| 0);

    hooks.use_future(async move {
        loop {
            smol::Timer::after(Duration::from_millis(100)).await;
            count += 1;  // State mutation triggers re-render
        }
    });

    element! {
        Text(color: Color::Blue, content: format!("counter: {}", count))
    }
}

fn main() {
    smol::block_on(element!(Counter).render_loop()).unwrap();
}
```

### The `element!` Macro

The primary composition tool. Syntax is component-name + named props + optional children block:

```rust
element! {
    View(
        border_style: BorderStyle::Round,
        border_color: Color::Blue,
        flex_direction: FlexDirection::Column,
    ) {
        Text(content: "Hello, world!")
        Text(color: Color::Red, content: "Goodbye!")
    }
}
```

Props use named argument syntax. The macro supports shorthand percentage values (e.g., `50pct`).

### Props System

Props are defined as structs with `#[derive(Default, Props)]`:

```rust
#[derive(Default, Props)]
struct FormFieldProps {
    label: String,
    value: Option<State<String>>,  // State<T> can be passed as prop for two-way binding
    has_focus: bool,
    multiline: bool,
}
```

Props are passed by value in the `element!` macro. Notably, `State<T>` (a wrapper around reactive state) can be passed as a prop, enabling two-way binding from parent to child.

### Hooks

State management through hooks called on the `Hooks` parameter:

| Hook | Purpose |
|------|---------|
| `hooks.use_state(|| initial)` | Reactive local state; mutation triggers re-render |
| `hooks.use_future(async { ... })` | Async side effects (timers, network, etc.) |
| `hooks.use_context::<T>()` | Access context provided by ancestor `ContextProvider` |

```rust
// State example
let mut count = hooks.use_state(|| 0);
count += 1;  // Triggers re-render
count.set(42);  // Alternative setter form

// Async future example
hooks.use_future(async move {
    loop {
        smol::Timer::after(Duration::from_millis(100)).await;
        count += 1;
    }
});

// Context example
let theme = hooks.use_context::<Theme>();
```

### Context System

Analogous to React Context. `ContextProvider` makes a value available to all descendants:

```rust
struct NumberOfTheDay(i32);

#[component]
fn MyContextConsumer(hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let number = hooks.use_context::<NumberOfTheDay>();
    element! {
        Text(content: format!("The number of the day is {}!", number.0))
    }
}

fn main() {
    element! {
        ContextProvider(value: Context::owned(NumberOfTheDay(42))) {
            MyContextConsumer
        }
    }.print();
}
```

### Layout Model

Flexbox via **Taffy**. Layout properties on `View` components mirror CSS flexbox:
- `flex_direction` (Row/Column)
- `justify_content`, `align_items`
- `padding`, `margin`
- `width`, `height` (with percentage support)

Taffy computes the layout tree, with a measure function closure computing leaf node sizes (text dimensions in terminal cells).

### Event Handling

The `use_input` hook enables keyboard input handling. The `weather.rs` example shows async data loading triggered by user input. The `calculator.rs` example shows clickable buttons.

### Output Modes

Two rendering modes:
- `.print()` — one-shot to stdout (no interaction, useful for rich formatted output)
- `.render_loop()` — interactive fullscreen TUI with event loop

### Key Takeaways

- Most complete and polished React-style TUI library in Rust
- Props-as-structs is idiomatic Rust (versus JSX-style attribute spreading)
- `State<T>` can be passed as props — enables two-way data flow without global state
- Async-native design with smol runtime
- Context avoids prop drilling
- Flexbox layout means rich layout without manual coordinate math
- No virtual DOM diffing mentioned — unclear if it does tree reconciliation or rerenders fully
- Works as both an interactive TUI library and a formatting/output library

---

## 4. tui-realm

**Repository**: https://github.com/veeso/tui-realm
**Maintenance**: Actively maintained ("Realm" name = React + Elm)

### Architecture Overview

tui-realm is a **framework built on top of Ratatui** that adds structure via a React/Elm hybrid architecture. It does not change how widgets render (still Ratatui's immediate-mode rendering), but wraps it in:

- A component lifecycle system
- Elm-style message passing
- A central Application that manages the tick loop
- A View that manages component mounting/focus/queries

The lifecycle: **Event → Cmd → MockComponent → CmdResult → Msg → Model.update() → View re-renders**

### Two-Tier Component System

**MockComponent** (reusable, library-distributable):
```rust
pub trait MockComponent {
    fn view(&mut self, frame: &mut Frame, area: Rect);
    fn query(&self, attr: Attribute) -> Option<AttrValue>;
    fn attr(&mut self, attr: Attribute, value: AttrValue);
    fn state(&self) -> State;
    fn perform(&mut self, cmd: Cmd) -> CmdResult;
}
```

**Component** (application-specific, wraps MockComponent):
```rust
pub trait Component<Msg, UserEvent>: MockComponent {
    fn on(&mut self, ev: Event<UserEvent>) -> Option<Msg>;
}
```

The key design insight: `MockComponent` handles *rendering and UI state* independent of application logic. `Component` translates *terminal events* into *application messages* — this translation layer is the application-specific part.

### Example Component

```rust
// Reusable input widget
struct TextInput {
    props: Props,
    text: String,
    cursor: usize,
}

impl MockComponent for TextInput {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
        // Render using Ratatui primitives
    }

    fn perform(&mut self, cmd: Cmd) -> CmdResult {
        match cmd {
            Cmd::Type(ch) => {
                self.text.insert(self.cursor, ch);
                self.cursor += 1;
                CmdResult::Changed(State::One(StateValue::String(self.text.clone())))
            }
            _ => CmdResult::None,
        }
    }
    // ...
}

// App-specific wrapper
#[derive(MockComponent)]
struct MyInput {
    component: TextInput,
}

impl Component<Msg, NoUserEvent> for MyInput {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<Msg> {
        match ev {
            Event::Keyboard(KeyEvent { code: Key::Char(ch), .. }) => {
                match self.perform(Cmd::Type(ch)) {
                    CmdResult::Changed(State::One(StateValue::String(s))) =>
                        Some(Msg::InputChanged(s)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
```

### Model-Update-View Cycle

The `Model` struct implements the `Update` trait:

```rust
pub trait Update<ComponentId, Msg, UserEvent> {
    fn update(
        &mut self,
        view: &mut View<ComponentId, Msg, UserEvent>,
        msg: Option<Msg>,
    ) -> Option<Msg>;
}
```

The `Application::tick()` method drives the loop:

```rust
fn main() {
    let mut model = Model::default();

    while !model.quit {
        if let Ok(messages) = model.app.tick(PollStrategy::Once) {
            for msg in messages {
                let mut msg = Some(msg);
                while msg.is_some() {
                    msg = model.update(msg);  // Chain messages
                }
            }
            model.redraw = true;
        }

        if model.redraw {
            model.view();   // Re-render the entire UI
            model.redraw = false;
        }
    }
}
```

### Events vs. Commands Distinction

| Events | Commands |
|--------|----------|
| Hardware-bound (keyboard, mouse, resize) | Application-logic independent |
| App-specific keybinding decisions | Reusable across apps |
| Example: `Event::Keyboard(KeyEvent { code: Key::Enter, .. })` | Example: `Cmd::Submit`, `Cmd::Type(ch)` |

This separation is the key to making `MockComponent` reusable across different applications with different keybindings.

### Properties vs. States

- **Properties**: Static configuration (colors, styles, dimensions) — set at mount time
- **States**: Dynamic runtime data (selected item, cursor position, text content) — can be queried

### Subscriptions

Components can subscribe to events even when not focused, enabling background event handling.

### Key Takeaways

- Adds significant structure to Ratatui apps without replacing Ratatui
- Elm-style message passing makes state transitions explicit and testable
- The Events/Commands split enables component reuse across apps with different keybindings
- More boilerplate than iocraft, but clearer separation of concerns
- No layout engine — layout is still handled manually via Ratatui `Rect` splitting
- Components are uniquely identified and mounted/unmounted, not freely composable trees

---

## 5. bevy_ratatui

**Repository**: https://github.com/ratatui/bevy_ratatui
**Maintenance**: Actively maintained (under ratatui org, originally by joshka/cxreiff)

### Architecture Overview

bevy_ratatui is not a new UI framework — it's an **integration layer** that lets you use Ratatui widgets inside a Bevy ECS application. The appeal: Bevy's ECS provides a structured, systems-based architecture for complex applications; bevy_ratatui handles the terminal I/O plumbing.

```rust
use bevy::prelude::*;
use bevy::app::ScheduleRunnerPlugin;
use bevy_ratatui::{RatatuiContext, RatatuiPlugins};

fn main() {
    let frame_time = std::time::Duration::from_secs_f32(1. / 60.);

    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(frame_time)),
            RatatuiPlugins::default(),
        ))
        .add_systems(Update, draw_system)
        .run();
}

fn draw_system(mut context: ResMut<RatatuiContext>) -> Result<()> {
    context.draw(|frame| {
        let text = ratatui::text::Text::raw("hello world");
        frame.render_widget(text, frame.area());
    })?;
    Ok(())
}
```

### What bevy_ratatui Provides

- `RatatuiPlugins` — sets up the Bevy/terminal integration
- `RatatuiContext` — a Bevy Resource wrapping the Ratatui terminal, callable in any Bevy system
- Event forwarding: keyboard/mouse events from crossterm become Bevy events (`KeyMessage`, or standard Bevy input events via `enable_input_forwarding`)

### Design Philosophy

This is **ECS as the architecture layer**, with Ratatui as the rendering backend. State lives in Bevy components/resources; systems read ECS state and call Ratatui drawing primitives. It's closer to an application framework than a UI framework.

### Key Takeaways

- Only useful if you're already invested in the Bevy ecosystem or want ECS-style state management
- Ratatui rendering is still immediate-mode; no new declarative layer
- Bevy's system scheduling can be useful for complex TUI apps with many concurrent concerns
- Heavy dependency (all of Bevy) for terminal UI

---

## 6. Other Notable Libraries

### rxtui (zerocore-ai/rxtui)

Early-stage (~319 stars, 50 commits, explicitly "early development"). Component model uses `#[derive(Component)]` with `#[update]` and `#[view]` proc-macro methods. Uses a `node!` macro DSL. Automatic diffing and dirty tracking. Feels closest to a React-without-JSX approach.

```rust
#[derive(Component)]
struct Counter;

impl Counter {
    #[update]
    fn update(&self, _ctx: &Context, msg: &str, mut count: i32) -> Action {
        match msg {
            "inc" => Action::update(count + 1),
            "dec" => Action::update(count - 1),
            _ => Action::exit(),
        }
    }

    #[view]
    fn view(&self, ctx: &Context, count: i32) -> Node {
        node! {
            div(pad: 2, align: center, w_frac: 1.0, gap: 1,
                @key(up): ctx.handler("inc"),
                @key(down): ctx.handler("dec"),
            ) [
                text(format!("Count: {count}"), color: white, bold)
            ]
        }
    }
}
```

### revue (hawk90/revue)

Vue-inspired signals/reactivity system for TUI. Primitives: `Signal<T>`, `Computed<T>`, `Effect`. `View` trait with a `render()` method. State changes auto-trigger re-renders. 557 commits suggesting more substantial development. Most "reactive" in the SolidJS/Vue sense.

```rust
struct Counter {
    count: Signal<i32>,
}

impl View for Counter {
    fn render(&self, ctx: &mut RenderContext) {
        let count = self.count.get();
        vstack()
            .child(Text::new(format!("Count: {}", count)).bold())
            .render(ctx);
    }
}
```

### hack-tui (SunKing2/hack-tui)

Very early vision project (6 commits) aiming for "one declarative description → multiple render targets" (terminal via Ratatui + web via WASM). Fluent builder API. Not production-ready.

### teatui

Lightweight Elm/BubbleTea-inspired framework on crates.io. Less prominent than tui-realm.

### r3bl_tui

Another TUI framework with its own component model. More focused on editor-style UI.

---

## 7. Broader Analysis: Declarative API Patterns for Terminal UIs

### The Fundamental Tension

Terminals are fundamentally different from web browsers:
1. **Character grid** — layout must snap to integer cell positions
2. **No paint API** — you write characters, not pixels
3. **ANSI escape codes** — the "rendering API" is a string of escape sequences
4. **No async layout** — everything must be computed before writing output
5. **Limited input events** — keyboards and mice, no touch, no pointer precision

These constraints shape which patterns work well:

### Pattern Analysis

#### React/Virtual DOM (iocraft, dioxus-tui, rxtui)

**How it works**: Component functions return element trees; framework manages the component lifecycle; state changes trigger re-renders of affected subtrees.

**Terminal fit**: Good. The component function model maps cleanly to "given state S, produce a cell grid G". Hooks provide a clean state API without global mutation.

**Challenges**:
- Virtual DOM diffing is overkill for terminal (cell grids diff trivially at render time — Ratatui already does this with its double-buffer)
- JSX/RSX macros for what is essentially a character grid feels over-engineered
- Flexbox layout (via Taffy) adds complexity but terminal layout is often simpler (split a rect)

**Verdict**: iocraft shows this works well in practice. The `element!` macro + `#[component]` + hooks is ergonomic. The React mental model (components, props, state, context) translates directly.

#### Elm/MVU (tui-realm, ratatui TEA pattern, rxtui's update fn)

**How it works**: Single model struct holds all state. Messages are enums. An `update(model, msg) -> model` function handles transitions. A `view(model) -> Widget` function renders.

**Terminal fit**: Excellent. The terminal redraw loop is a natural fit for MVU — you have a render loop anyway. Messages make state transitions explicit and easy to test.

**Challenges**:
- Verbose for simple cases (every interaction needs a message type)
- Layout is still manual (tui-realm doesn't add a layout engine)
- Nested components require message routing boilerplate (tui-realm's `on()` chain)

**Verdict**: The most production-battle-tested pattern for complex TUI apps. BubbleTea (Go) dominates this space and Rust's tui-realm provides similar capability. The downside is boilerplate for state that is inherently local to a widget.

#### Signals/Fine-Grained Reactivity (revue, Vue-inspired)

**How it works**: State wrapped in `Signal<T>`; reads create subscriptions; writes invalidate dependents; `Effect`s run automatically on dependency change.

**Terminal fit**: Interesting fit. The terminal redraw loop becomes: run all dirty effects, then redraw affected cells. Could enable very efficient partial redraws.

**Challenges**:
- Signals in Rust require careful lifetime management (typically needs `Arc<Mutex<T>>` or an arena)
- The terminal's double-buffer diff (Ratatui's approach) already handles partial redraw efficiently
- Complex ownership semantics conflict with fine-grained reactive graphs

**Verdict**: Theoretically appealing (SolidJS/Leptos proves fine-grained reactivity can be extremely efficient). In practice, Rust's ownership model makes building signal graphs harder than in JS/TypeScript. revue exists but is not mature.

#### Retained-Mode Widget Tree (Cursive)

**How it works**: Build a widget tree once; mutate it imperatively via callbacks.

**Terminal fit**: Good for dialog-heavy apps. Familiar to developers coming from GTK/Qt/TkInter. Built-in focus management.

**Challenges**:
- No reactivity — finding views by name and mutating them is error-prone
- Not composable in a functional sense
- Harder to reason about state when it's scattered across callbacks and view mutations

**Verdict**: Practical but not "modern". Best for traditional form-based TUI applications. Not the direction new frameworks are moving.

#### Immediate Mode (Ratatui baseline)

**How it works**: Each frame, call widget render methods with current state. No retained state in widgets.

**Terminal fit**: Excellent. Clean and simple — just describe what to draw. No framework overhead.

**Challenges**:
- Developer must manage all state
- No composition abstraction beyond ad-hoc functions
- Layout requires manual `Rect` splitting

**Verdict**: The right foundation. The best declarative frameworks (iocraft, tui-realm) sit *on top of* or *alongside* immediate-mode rendering, not replacing it.

### What a Good Declarative TUI API Might Look Like

Based on this survey, the most ergonomic patterns appear to be:

1. **Function components + hooks**: iocraft demonstrates this is ergonomic in Rust. `#[component]` + `element!` is clean. The React mental model translates well.

2. **Props as typed structs**: Much better than string-keyed props or CSS-string properties. Rust's type system makes props composable and checked at compile time.

3. **`State<T>` passable as props**: iocraft's ability to pass `State<T>` as a prop enables two-way binding without global state stores.

4. **Context for cross-cutting concerns**: Theme, configuration, focus management — context avoids prop drilling without global singletons.

5. **Flexbox layout via Taffy**: The most principled layout model. Eliminates `Rect`-splitting boilerplate while staying deterministic.

6. **Async-native state effects**: `use_future` / async effects are the right model for timers, network requests, and event streams.

7. **Elm MVU for complex apps**: When the app has complex cross-component state, a central `Model` + `update(msg)` is more maintainable than a forest of component-local state.

8. **Escape hatch to immediate-mode**: Any framework that lets you drop down to raw Ratatui/crossterm when needed will be more adoptable.

---

## Summary Table

| Library | Pattern | Layout | State | Maintained | Maturity |
|---------|---------|--------|-------|------------|---------|
| **iocraft** | React/Hooks | Flexbox (Taffy) | Hooks (`use_state`, `use_future`) | Yes (Feb 2026) | Production-ready |
| **tui-realm** | Elm/MVU | Manual (Ratatui Rect) | External model struct | Yes | Production-ready |
| **Cursive** | Retained widget tree | Negotiate (req_size/layout) | Imperative callbacks | Yes (Oct 2025) | Production-ready |
| **bevy_ratatui** | ECS + Immediate | Manual (Ratatui) | Bevy ECS resources | Yes | Production-ready |
| **dioxus-tui** | React/RSX | Flexbox (Taffy) | Dioxus hooks | No (discontinued) | Abandoned |
| **rxtui** | MVU + component macro | Unknown | External state | Early dev | Experimental |
| **revue** | Signals/Vue | Unknown | Signal<T> | Uncertain | Experimental |
| **Ratatui** | Immediate mode | Manual (Rect) | External (user-managed) | Yes | Dominant baseline |

---

## Sources

- [GitHub - DioxusLabs/rink: Build reactive terminal user interfaces using Rust and Dioxus](https://github.com/DioxusLabs/rink)
- [Document TUI status and roadmap on docsite · Issue #2620 · DioxusLabs/dioxus](https://github.com/DioxusLabs/dioxus/issues/2620)
- [dioxus-tui - crates.io](https://crates.io/crates/dioxus-tui)
- [GitHub - gyscos/cursive: A Text User Interface library for the Rust programming language](https://github.com/gyscos/cursive)
- [View in cursive - Rust (docs.rs)](https://docs.rs/cursive/latest/cursive/trait.View.html)
- [cursive::view - Rust (docs.rs)](https://docs.rs/cursive/latest/cursive/view/index.html)
- [GitHub - ccbrown/iocraft: A Rust crate for beautiful, artisanally crafted CLIs, TUIs, and text-based IO](https://github.com/ccbrown/iocraft)
- [iocraft - Rust (docs.rs)](https://docs.rs/iocraft/latest/iocraft/)
- [Iocraft - new TUI / CLI library with React-like style - The Rust Programming Language Forum](https://users.rust-lang.org/t/iocraft-new-tui-cli-library-with-react-like-style/119236)
- [GitHub - veeso/tui-realm: A ratatui framework to build stateful applications with a React/Elm inspired approach](https://github.com/veeso/tui-realm)
- [tui-realm/docs/en/get-started.md at main · veeso/tui-realm](https://github.com/veeso/tui-realm/blob/main/docs/en/get-started.md)
- [veeso/tui-realm | DeepWiki](https://deepwiki.com/veeso/tui-realm/1-overview)
- [The Elm Architecture (TEA) | Ratatui](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/)
- [Rendering | Ratatui](https://ratatui.rs/concepts/rendering/)
- [GitHub - ratatui/bevy_ratatui: A rust crate for using Ratatui in a Bevy application](https://github.com/ratatui/bevy_ratatui)
- [GitHub - hawk90/revue: Modern reactive TUI framework for Rust — signals, effects, and beautiful widgets](https://github.com/hawk90/revue)
- [GitHub - zerocore-ai/rxtui: reactive terminal interfaces for Rust](https://github.com/zerocore-ai/rxtui)
- [GitHub - SunKing2/hack-tui: Declarative UI for Rust, Ratatui, Web](https://github.com/SunKing2/hack-tui)
- [GitHub - ratatui/awesome-ratatui: A curated list of TUI apps and libraries built with ratatui](https://github.com/ratatui/awesome-ratatui)
- [DioxusLabs/taffy: A high performance rust-powered UI layout library](https://github.com/DioxusLabs/taffy)
