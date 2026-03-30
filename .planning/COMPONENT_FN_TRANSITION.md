# Component Function Transition Plan

**Status**: In progress
**Updated**: 2026-03-29

## Background

eye_declare is transitioning from a struct + `impl Component` model to a `#[component]` fn decorator model. The `#[component]` and `#[props]` macros exist and work for the happy path, but the internals are still structured around the old model. All 9 built-in components still use the old model.

**Reference**: `iocraft` is a similar Rust TUI crate with a function-component syntax — may provide useful patterns.

## Current State

### Old Model (struct + impl Component)
```rust
#[derive(TypedBuilder)]
struct Spinner {
    label: String,
    done: bool,
}

impl Component for Spinner {
    type State = SpinnerState;
    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) { ... }
    fn lifecycle(&self, hooks: &mut Hooks<SpinnerState>, state: &SpinnerState) { ... }
}
```

### New Model (#[component] fn)
```rust
#[props]
struct CardProps {
    title: String,
    #[default(true)]
    visible: bool,
}

#[component(props = CardProps, children = Elements)]
fn card(props: &CardProps, children: Elements) -> Elements {
    element! { View(border: BorderType::Rounded) { #(children) } }
}
```

### What #[component] generates

1. `impl Component for PropsStruct` with `update()` override (combined lifecycle + view)
2. `impl_slot_children!` if `children = Elements`
3. The function body is called **once per cycle** via `update()` with real hooks and real children

---

## Friction Points

### F1. ~~Component trait carries legacy methods~~ (Resolved in Wave 3A/3C)

All legacy methods are now `#[doc(hidden)]`. The trait's public API is just `State` + `update()`. Users define components via `#[component]` and never see the trait methods.

### F2. ~~Hooks can't override everything~~ (Resolved by design)

All behavioral methods have hook equivalents. The remaining `render()` and `content_inset()` are primitive-only — kept on View and Canvas by design, not exposed to `#[component]` users.

### F3. ~~Function body runs twice per cycle~~ (Resolved in Wave 3B)

Solved by the `update()` trait method. `#[component]` now generates an `update()` override that calls the user function once with real hooks and real children. The default `update()` implementation chains `lifecycle()` then `view()` for backward compatibility with hand-written primitives.

### F4. Data children not supported

`#[component]` only supports `children = Elements`. Components like TextBlock that accept typed data children (`Line`/`Span`) must use manual `ChildCollector` + `DataChildren<T>`.

### F5. ~~Fragile parameter detection~~ (Resolved in Wave 1B)

Hooks parameter now detected by type `&mut Hooks<T>`.

### F6. Two parallel paths for slot children

Old: struct + `impl_slot_children!` macro. New: `#[component(children = Elements)]`.

---

## Roadmap

### Wave 1 — Low-risk enablers (current wave)

| # | Task | Effort | Status |
|---|------|--------|--------|
| 1A | Add `hooks.use_layout()` and `hooks.use_width_constraint()` | Low | Done |
| 1B | Detect hooks parameter by type (`&mut Hooks<T>`) instead of name | Low | Done |
| 1C | Support `initial_state` in `#[component]` (attribute or hook) | Low-Medium | Done |

### Wave 2 — Migrate built-ins to `#[component]` fn model

| # | Task | Effort | Status |
|---|------|--------|--------|
| 2A | Convert VStack/HStack/Column to `#[component]` | Low | Done |
| 2B | Convert Spinner to `#[component]` (returns Canvas element) | Medium | Done |
| 2C | Convert Markdown to `#[component]` (returns Canvas element) | Medium | Done |
| 2D | Keep View as hand-written primitive (fundamental building block) | N/A | Done — kept by design |
| 2E | Keep Canvas as hand-written primitive (fundamental building block) | N/A | Done — kept by design |

> **Design decision**: View and Canvas are hand-written `impl Component` primitives
> that `#[component]` functions compose *with*. They provide the border/inset and
> imperative-render escape hatches that all other components build on. There is no
> benefit to converting them — they are the foundation, not the target.

> **Blocked**: TextBlock/Line/Span require `children = DataChildren<T>` support in
> `#[component]` (Wave 4B). They use the data children pattern, which the macro
> does not yet support.

### Wave 3 — Structural simplification (after migration)

| # | Task | Effort | Status |
|---|------|--------|--------|
| 3A | Unify `render()` and `view()` into a single path | High | Done |
| 3B | Call function once, not twice — `update()` combines lifecycle + view | High | Done |
| 3C | Hide legacy trait methods behind `#[doc(hidden)]` | Medium | Done |
| 3D | Simplify `ChildCollector` / `DataChildren` / `ComponentWithSlot` hierarchy | High | Pending |
| 3E | Remove struct+impl path entirely; `#[component]` becomes the only way | High | Pending |

### Wave 4 — Future enhancements

| # | Task | Effort | Status |
|---|------|--------|--------|
| 4A | `hooks.use_height_hint(n)` for explicit height declarations | Low | Pending |
| 4B | `children = SomeType` support in `#[component]` (data children) | High | Pending |
| 4C | Typed event emission (`ctx.emit()`) | Medium | Pending |
| 4D | `use_ref` / imperative handles for parent-to-child state access | Medium | Pending |
| 4E | Effects / async in components | High | Pending |

---

## Key Files

| File | Role |
|------|------|
| `crates/eye_declare/src/component.rs` | Component trait, Tracked, EventResult, VStack/HStack/Column, impl_slot_children! |
| `crates/eye_declare/src/hooks.rs` | Hooks struct and all hook methods |
| `crates/eye_declare/src/node.rs` | Node, AnyComponent, AnyTrackedState, type erasure, effect system |
| `crates/eye_declare/src/element.rs` | Element trait, Elements, ElementEntry |
| `crates/eye_declare/src/children.rs` | ChildCollector, AddTo, DataChildren, ComponentWithSlot |
| `crates/eye_declare_macros/src/component.rs` | #[component] attribute macro implementation |
| `crates/eye_declare_macros/src/props.rs` | #[props] attribute macro implementation |
| `crates/eye_declare_macros/src/lib.rs` | Proc macro entry points |
| `crates/eye_declare/src/components/` | Built-in components (canvas, markdown, spinner, text, view) |
| `crates/eye_declare/src/inline.rs` | InlineRenderer |
| `crates/eye_declare/src/renderer.rs` | Renderer, reconciliation, layout, rendering pipeline |

## End State Vision

The `Component` trait becomes an internal implementation detail. Users only interact with:

```rust
#[props]
struct MyProps { ... }

#[component(props = MyProps, state = MyState, children = Elements)]
fn my_component(props: &MyProps, state: &MyState, hooks: &mut Hooks<MyState>, children: Elements) -> Elements {
    // Everything expressed through:
    // - Return value (element tree)
    // - Hooks (behavioral capabilities)
    // - Props (input)
    // - State (internal data)
}
```

The trait may be hidden entirely, with `#[component]` being the sole public API for defining components.
