# Component Function Transition Plan

**Status**: Core transition complete (waves 1–4A/B + Text redesign)
**Updated**: 2026-03-30

## Background

eye_declare transitioned from a struct + `impl Component` model to a `#[component]` fn decorator model. The transition is complete: all built-in components use `#[component]` except the two framework primitives (View, Canvas) which implement `Component` directly by design.

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

**For slot children (`children = Elements`):**
1. `impl Component for Props` with `update()` override
2. `impl ChildCollector` (inline, replaces `impl_slot_children!`)
3. Function called once per cycle with real hooks and real children

**For data children (`children = DataChildren<T>`):**
1. `impl Component for Props` (no-children usage, passes empty data)
2. Hidden wrapper struct holding props + collected data
3. `impl Component for Wrapper` (with-children usage, passes real data)
4. `impl ChildCollector for Props` → output is the wrapper

### Primitives (hand-written impl Component)

View and Canvas are the two framework primitives that implement `Component` directly. They provide the border/inset and imperative-render escape hatches that `#[component]` functions compose with.

### Text model

`Text` replaces the old `TextBlock`/`Line` hierarchy. It's a `#[component]` with `children = DataChildren<TextChild>` that accepts `Span` and string children directly:

```rust
// Simple
"hello"                    // string literal sugar → Text
Text { "hello" }           // explicit

// Styled
Text(style: dim) { "hello" }
Text { Span(text: "hello", style: bold) }

// Mixed spans
Text { "Name: " Span(text: name, style: green) }

// Multi-line: View handles stacking
View { Text { "Line one" } Text { "Line two" } }
```

---

## Friction Points (all resolved)

### F1. ~~Component trait carries legacy methods~~ (Wave 3A/3C)
All legacy methods are `#[doc(hidden)]`. The trait's public API is just `State` + `update()`.

### F2. ~~Hooks can't override everything~~ (By design)
All behavioral methods have hook equivalents. `render()` and `content_inset()` are primitive-only.

### F3. ~~Function body runs twice per cycle~~ (Wave 3B)
Solved by `update()` trait method. Function runs once with real hooks and real children.

### F4. ~~Data children not supported~~ (Wave 4B)
`#[component]` supports `children = DataChildren<T>`. Hidden wrapper + ChildCollector generated.

### F5. ~~Fragile parameter detection~~ (Wave 1B)
Hooks parameter detected by type `&mut Hooks<T>`.

### F6. ~~Two parallel paths for slot children~~ (Wave 4B)
`#[component]` generates ChildCollector inline. `impl_slot_children!` only used by View (primitive).

---

## Roadmap

### Wave 1 — Low-risk enablers ✅

| # | Task | Status |
|---|------|--------|
| 1A | `hooks.use_layout()` and `hooks.use_width_constraint()` | Done |
| 1B | Detect hooks parameter by type | Done |
| 1C | `initial_state` attribute on `#[component]` | Done |

### Wave 2 — Migrate built-ins to `#[component]` ✅

| # | Task | Status |
|---|------|--------|
| 2A | VStack/HStack/Column → `#[component]` | Done |
| 2B | Spinner → `#[component]` (returns Canvas) | Done |
| 2C | Markdown → `#[component]` (returns Canvas) | Done |
| 2D | View — kept as hand-written primitive | Done — by design |
| 2E | Canvas — kept as hand-written primitive | Done — by design |

### Wave 3 — Structural simplification ✅

| # | Task | Status |
|---|------|--------|
| 3A | Formalize the primitive/component split | Done |
| 3B | Single-call `update()` | Done |
| 3C | Hide legacy trait methods | Done |
| 3D | Simplify children hierarchy | Done — resolved by 4B (macro handles complexity) |
| 3E | Make Component trait `#[doc(hidden)]` | Parked — marginal value with 3C done |

**Bug fixes from wave 3:**
- Dirty `#[component]` containers re-reconcile before render
- Application tick sets dirty when effects fire
- TypedBuilder field defaults aligned with struct Default

### Wave 4 — Enhancements ✅ (4A/B done, 4C-E future)

| # | Task | Status |
|---|------|--------|
| 4A | `hooks.use_height_hint(n)` | Done |
| 4B | Data children in `#[component]` | Done |
| 4C | Typed event emission (`ctx.emit()`) | Future |
| 4D | `use_ref` / imperative handles | Future |
| 4E | Effects / async in components | Future |

### Text redesign ✅

| Task | Status |
|------|--------|
| Replace TextBlock/Line/Span with Text/Span | Done |
| String literal sugar produces Text | Done |
| `AddTo<Elements> for String` context-aware dispatch | Done |
| `Text::unstyled()` / `Text::styled()` constructors | Done |
| All docs, examples, README updated | Done |

---

## Architecture Summary

```
User API                        Framework internals
─────────                       ──────────────────
#[props] struct Props { }       Component trait (#[doc(hidden)] methods)
#[component(props, state,       AnyComponent (type erasure)
  children)]                    Node, NodeArena
fn my_comp(...) -> Elements     Renderer (reconciliation, layout, render)
                                InlineRenderer (ANSI output, scrollback)
element! { ... }                Application (async event loop)

Built-in components:
  Text      — #[component], data children (Span/String)
  Spinner   — #[component], Canvas child, use_interval
  Markdown  — #[component], Canvas child
  VStack    — #[component], passthrough
  HStack    — #[component], use_layout(Horizontal)
  Column    — #[component], use_width_constraint
  View      — hand-written primitive (borders, padding, layout)
  Canvas    — hand-written primitive (raw buffer rendering)
```

## Key Files

| File | Role |
|------|------|
| `crates/eye_declare/src/component.rs` | Component trait, Tracked, EventResult, VStack/HStack/Column |
| `crates/eye_declare/src/hooks.rs` | Hooks struct, HooksOutput, all hook methods |
| `crates/eye_declare/src/node.rs` | Node, AnyComponent, type erasure, effect system |
| `crates/eye_declare/src/element.rs` | Element trait, Elements, ElementEntry |
| `crates/eye_declare/src/children.rs` | ChildCollector, AddTo, DataChildren, ComponentWithSlot |
| `crates/eye_declare_macros/src/component.rs` | #[component] macro (slot + data children codegen) |
| `crates/eye_declare_macros/src/props.rs` | #[props] macro |
| `crates/eye_declare/src/components/` | Built-in components (text, canvas, markdown, spinner, view) |
| `crates/eye_declare/src/renderer.rs` | Renderer, reconciliation, layout, dirty refresh |
| `crates/eye_declare/src/inline.rs` | InlineRenderer |
| `crates/eye_declare/src/app.rs` | Application, Handle, interactive loop |

## Future Work (4C–4E)

**4C — Typed event emission:** `event = MyEvent` on `#[component]`, `ctx.emit()` in hooks. Framework routes events upward through the tree. Replaces context-channel pattern.

**4D — Imperative refs:** `use_ref::<HandleTrait>()` for parent-to-child state access. Useful for wrapping external widgets (tui-textarea, etc.).

**4E — Async in components:** `use_future()` for per-component async tied to component lifetime. Cancelled on unmount. Depends on runtime model decisions.

These are independent features with no blockers from the core transition work.
