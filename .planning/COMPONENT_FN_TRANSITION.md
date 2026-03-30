# Component Function Transition Plan

**Status**: Waves 1–3 complete, wave 4 next
**Updated**: 2026-03-29

## Background

eye_declare transitioned from a struct + `impl Component` model to a `#[component]` fn decorator model. The `#[component]` and `#[props]` macros are the primary API. All built-in components except TextBlock (blocked on data children) and the two primitives (View, Canvas — by design) use `#[component]`.

**Reference**: `iocraft` is a similar Rust TUI crate with a function-component syntax — informed the single-call `update()` design.

## Current State

### Primary API (#[component] fn)
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

### Primitives (hand-written impl Component)

View and Canvas are the two framework primitives that implement `Component` directly. They provide the border/inset and imperative-render escape hatches that `#[component]` functions compose with. TextBlock also uses manual impl (blocked on data children support in `#[component]`).

---

## Friction Points

### F1. ~~Component trait carries legacy methods~~ (Resolved in Wave 3A/3C)

All legacy methods are now `#[doc(hidden)]`. The trait's public API is just `State` + `update()`. Users define components via `#[component]` and never see the trait methods.

### F2. ~~Hooks can't override everything~~ (Resolved by design)

All behavioral methods have hook equivalents. The remaining `render()` and `content_inset()` are primitive-only — kept on View and Canvas by design, not exposed to `#[component]` users.

### F3. ~~Function body runs twice per cycle~~ (Resolved in Wave 3B)

Solved by the `update()` trait method. `#[component]` now generates an `update()` override that calls the user function once with real hooks and real children. The default `update()` implementation chains `lifecycle()` then `view()` for backward compatibility with hand-written primitives.

### F4. ~~Data children not supported~~ (Resolved in Wave 4B)

`#[component]` now supports `children = DataChildren<T>` (or any collector type). The macro generates a hidden wrapper struct + ChildCollector impl. The function receives data children by reference.

### F5. ~~Fragile parameter detection~~ (Resolved in Wave 1B)

Hooks parameter now detected by type `&mut Hooks<T>`.

### F6. Two parallel paths for slot children

Old: struct + `impl_slot_children!` macro. New: `#[component(children = Elements)]`. Both work; the old path is needed for View (primitive) and TextBlock (data children).

---

## Roadmap

### Wave 1 — Low-risk enablers ✅

| # | Task | Status |
|---|------|--------|
| 1A | Add `hooks.use_layout()` and `hooks.use_width_constraint()` | Done |
| 1B | Detect hooks parameter by type (`&mut Hooks<T>`) instead of name | Done |
| 1C | Support `initial_state` in `#[component]` (attribute or hook) | Done |

### Wave 2 — Migrate built-ins to `#[component]` fn model ✅

| # | Task | Status |
|---|------|--------|
| 2A | Convert VStack/HStack/Column to `#[component]` | Done |
| 2B | Convert Spinner to `#[component]` (returns Canvas element) | Done |
| 2C | Convert Markdown to `#[component]` (returns Canvas element) | Done |
| 2D | Keep View as hand-written primitive (fundamental building block) | Done — kept by design |
| 2E | Keep Canvas as hand-written primitive (fundamental building block) | Done — kept by design |

> **Unblocked**: TextBlock/Line/Span can now be migrated to `#[component]` using
> `children = DataChildren<T>` (Wave 4B complete).

### Wave 3 — Structural simplification ✅ (3D, 3E deferred)

| # | Task | Status |
|---|------|--------|
| 3A | Unify `render()` and `view()` — formalize the primitive/component split | Done |
| 3B | Single-call `update()` — function runs once per cycle, not twice | Done |
| 3C | Hide legacy trait methods behind `#[doc(hidden)]` | Done |
| 3D | Simplify `ChildCollector` / `DataChildren` / `ComponentWithSlot` hierarchy | Deferred to 4B |
| 3E | Make Component trait `#[doc(hidden)]` | Parked — marginal value with 3C done |

**Wave 3 also fixed two bugs from wave 2:**
- Spinner animation: dirty `#[component]` containers now re-reconcile before render, and Application tick sets dirty when effects fire
- Spinner builder styles: TypedBuilder field defaults aligned with struct Default (DarkGray/Green)

### Wave 4 — Future enhancements

| # | Task | Effort | Status |
|---|------|--------|--------|
| 4A | `hooks.use_height_hint(n)` for explicit height declarations | Low | Done |
| 4B | `children = SomeType` support in `#[component]` (data children) | High | Done |
| 4C | Typed event emission (`ctx.emit()`) | Medium | Pending |
| 4D | `use_ref` / imperative handles for parent-to-child state access | Medium | Pending |
| 4E | Effects / async in components | High | Pending |

**Dependencies:**
- 4B unblocks: TextBlock migration to `#[component]`, 3D (children hierarchy simplification)
- 4C unblocks: cleaner component communication (replaces context-channel pattern)
- 4E depends on: runtime model decisions (per-component vs Application-level async)

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

The `Component` trait is an internal implementation detail. Users interact with:

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

The trait is hidden behind `#[doc(hidden)]` methods (wave 3C). View, Canvas, and TextBlock are the only manual implementors — they are framework primitives, not user-facing patterns.
