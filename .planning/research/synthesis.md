# Research Synthesis: eye_declare Architecture

## The Design Space

eye_declare is a **declarative rendering library** for **inline** (non-alt-screen) terminal UIs, targeting an AI-native shell proxy built on Atuin Hex. The library supports multiple rendering modes — from growing inline conversations to fixed status bars — while remaining host-agnostic so it works both as a standalone renderer and embedded within Hex's PTY proxy architecture.

The research covers two mature reference architectures (pi-tui, Codex tui2), one mature declarative API pattern (iocraft), the Ratatui modular crate ecosystem, and the Atuin Hex PTY proxy internals.

---

## Target Application Context

**Atuin Hex** is a PTY proxy that transparently wraps the user's shell, tracking prompts/commands/outputs via OSC 133 semantic zones. **Atuin AI** provides English-to-shell generation and Q&A. The vision is fusing these into an **AI-native shell proxy**: use the shell normally, switch into natural language conversations where the agent has full context.

**UI rendering needs span a spectrum:**

- **Growing inline regions** (primary, implemented): AI conversations that appear inline, grow as messages stream in, and become normal scrollback when done.
- **Fixed regions** (status bar, designed): A bottom bar showing agent status, updated independently of shell output. Uses terminal scroll regions to reserve space.
- **Floating overlays** (popups, designed): Config menus, autocomplete, agent status cards that appear over shell output temporarily.

**Deferred**: Full terminal multiplexing (side panes). Too complex, and carries the same scrollback/copy-paste issues as tmux.

---

## Reference Architectures

### pi-tui: Line-Level Retained Components
- Components return `string[]` (ANSI-encoded lines), own their caching
- Diff is line-level: find first changed line, rewrite from there to end
- Full clear on resize (destroys scrollback)
- Simple, ~1200 LOC for the core

### Codex tui2: Cell-Level App-Owned Viewport
- `HistoryCell` trait, two-layer cache, content-relative scroll anchors
- Display-time-only wrapping via byte-range span slicing
- Uses alt-screen — data model is portable but rendering context differs

### Ratatui Primitives (what we use)
- `ratatui-core`: Buffer, Cell, Rect, Layout, Style, Widget/StatefulWidget traits
- `ratatui-widgets` (with `unstable-rendered-line-info` feature): Paragraph with `wrap()` and `line_count()`
- `Buffer::diff()` as a standalone pure method
- `crossterm`: terminal I/O, event types (used directly, not via ratatui-crossterm)
- `unicode-width`: correct display width for CJK/emoji

---

## What's Built (as of Phase 4)

### Architecture Stack

```
┌─────────────────────────────────────────────────┐
│  Host Application                               │
│  (Hex PTY proxy, standalone CLI, etc.)          │
│  - Owns terminal I/O                            │
│  - Manages regions (scroll regions, overlays)   │
│  - Routes input events                          │
│  - Writes eye_declare output to terminal        │
├─────────────────────────────────────────────────┤
│  eye_declare: Terminal (optional convenience)   │
│  - Event loop with crossterm                    │
│  - Raw mode, resize handling, Ctrl+C exit       │
│  - Direct stdout writing                        │
├─────────────────────────────────────────────────┤
│  eye_declare: InlineRenderer                    │
│  - Growing-region mode (cursor mgmt, newline    │
│    claiming, scrollback)                        │
│  - Resize (clear screen + re-render)            │
│  - Cursor positioning from focused component    │
├─────────────────────────────────────────────────┤
│  eye_declare: Renderer                          │
│  - Component tree (NodeId arena, parent/child)  │
│  - Recursive measure + render                   │
│  - Dirty tracking (Tracked<S> auto-dirty)       │
│  - force_dirty on width change                  │
│  - Freeze lifecycle (cached buffers)            │
│  - Focus management + Tab cycling               │
│  - Event delivery with bubble-up propagation    │
├─────────────────────────────────────────────────┤
│  eye_declare: Frame / Diff / Escape             │
│  - Cell-level diffing (Buffer::diff)            │
│  - Height-mismatch padding for growing content  │
│  - Relative cursor movement (no absolute pos)   │
│  - DEC 2026 synchronized output wrapping        │
│  - SGR style diffing (minimal escape sequences) │
├─────────────────────────────────────────────────┤
│  Built-in Components                            │
│  - TextBlock: display-time word wrapping        │
│  - Spinner: animated with tick/complete         │
│  - Markdown: bold, italic, code, headings, lists│
│  - VStack: pure vertical container              │
├─────────────────────────────────────────────────┤
│  ratatui-core         │  crossterm              │
│  (Buffer, Cell, Rect, │  (escape sequences,     │
│   Layout, Style,      │   event types,          │
│   Widget trait)       │   terminal queries)     │
└─────────────────────────────────────────────────┘
```

### Module Map

| Module | Purpose |
|--------|---------|
| `component.rs` | `Component` trait, `Tracked<S>`, `EventResult`, `VStack` |
| `node.rs` | `NodeId`, `Node`, type-erasure traits (`AnyComponent`, `AnyTrackedState`) |
| `renderer.rs` | Tree management, recursive measure/render, focus, Tab cycling, event delivery |
| `frame.rs` | `Frame` (owns Buffer), `Diff` (changed cells), height-mismatch handling |
| `escape.rs` | `CursorState`, relative cursor movement, SGR diffing, DEC 2026 wrapping |
| `inline.rs` | `InlineRenderer` — growing region, cursor tracking, resize |
| `terminal.rs` | `Terminal` — convenience event loop wrapper for standalone use |
| `wrap.rs` | `wrapped_line_count()`, `wrapping_paragraph()` utilities |
| `components/text.rs` | `TextBlock` — styled text with display-time wrapping |
| `components/spinner.rs` | `Spinner` — animated spinner with completion state |
| `components/markdown.rs` | `Markdown` — inline formatting, code blocks, headings, lists |

### Actual Component Trait (implemented)

```rust
pub trait Component: Send + Sync + 'static {
    type State: Send + Sync + 'static;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State);
    fn desired_height(&self, width: u16, state: &Self::State) -> u16;
    fn initial_state(&self) -> Self::State;

    // Optional with defaults:
    fn handle_event(&self, event: &Event, state: &mut Self::State) -> EventResult { Ignored }
    fn is_focusable(&self, state: &Self::State) -> bool { false }
    fn cursor_position(&self, area: Rect, state: &Self::State) -> Option<(u16, u16)> { None }
}
```

### Key Differences from Original Design

- `desired_height` returns `u16` not `Option<u16>` — fill-height deferred until flexible layout is needed
- No separate `Tree` handle — tree manipulation is directly on `Renderer` (`append_child`, `push`, `remove`, `children`)
- `Node.force_dirty` flag for framework-initiated re-renders (width change) alongside `Tracked<S>` auto-dirty
- Escape generation uses relative movement only (no absolute positioning) — critical for inline rendering
- `Terminal` wrapper uses synchronous crossterm events, not async

### Test Coverage: 73 tests

- `component.rs`: Tracked<S> dirty behavior (4 tests)
- `frame.rs`: Diff with identical, changed, growing, shrinking frames (5 tests)
- `escape.rs`: Sync wrapping, relative movement, style diffing (5 tests)
- `wrap.rs`: Line counting with wrapping, empty text, zero width (7 tests)
- `renderer.rs`: Flat rendering, tree nesting, dirty tracking, freeze, remove, events, focus cycling (24 tests)
- `inline.rs`: First render, no-change, growing content (4 tests)
- `components/text.rs`: Wrapping, height, styling, render (7 tests)
- `components/spinner.rs`: Height, render, completion, tick (4 tests)
- `components/markdown.rs`: All formatting types, wrapping, streaming (12 tests)
- Doc test in terminal.rs (1 test)

### Examples: 7

| Example | Demonstrates |
|---------|-------------|
| `growing` | Basic growing inline text |
| `agent_sim` | Animated agent conversation with spinners + streaming |
| `nested` | Multi-turn conversation with tree composition |
| `wrapping` | Live resize reflow with crossterm events |
| `interactive` | Text input with wrapping + cursor positioning |
| `terminal_demo` | Terminal wrapper with Tab cycling between two inputs |
| `markdown_demo` | Streamed markdown response with full formatting |

---

## Resolved Design Decisions

### Rendering
- **Cell-level diffing** via `Buffer::diff()` — compatible with all ratatui widgets
- **Relative cursor movement only** — no absolute positioning, correct for inline rendering
- **DEC 2026 synchronized output** — atomic frame display, always emitted (no feature detection)
- **Cursor hidden during writes**, shown only at focused component's cursor position after render

### Component Model
- **External state** with `Tracked<S>` auto-dirty via `DerefMut`
- **Framework-managed tree** — components don't know about children
- **Intrinsic sizing** — `desired_height(width)` with vertical stacking
- **Freeze lifecycle** — frozen components skip re-render, use cached buffers
- **Type erasure** via `AnyComponent`/`AnyTrackedState` with `Any` downcasting

### Event Handling
- **Host delivers events** — eye_declare doesn't own the event loop
- **Focus + bubble-up** — events go to focused component, Ignored bubbles to parent
- **Tab cycling** — framework walks tree DFS for focusable components
- **Cursor positioning** — `cursor_position()` on Component, framework positions hardware cursor

### Scrollback & Resize
- **Implicit scrollback** — content scrolls off naturally (pi-tui model)
- **Standalone resize** — clear visible screen + re-render (scrollback preserved at old width)
- **Hex-assisted resize** (designed, not built) — Hex's shadow vt100 parser diffs against fresh render for smooth reflow without clearing

---

## What's Next

### Near-term: Declarative Layer (the big piece)

React-style component-level composition where components return element trees instead of rendering into buffers:

```rust
fn render(&self, state: &Self::State) -> Element {
    element! {
        VStack {
            for msg in &state.messages {
                TextBlock { text: msg.text, style: msg.style }
            }
            Spinner { label: "Thinking..." }
        }
    }
}
```

The framework reconciles this description against the existing tree, creating/updating/removing nodes as needed. This is the reconciler pattern from React — the piece that makes the library truly declarative.

**Key design questions:**
- Element representation (enum vs trait object vs generic)
- Reconciliation algorithm (keyed children for stable identity)
- Hooks system (use_state, use_effect) vs current external state model
- Whether to keep the current retained-component API as a lower-level escape hatch

### Medium-term

- **Focus scopes** — independent focus cycles for modals/popups (gpui-inspired)
- **Bordered containers** — components that provide padding/border and pass inner area to children
- **Hex integration** — experimental rendering within Hex's PTY proxy
- **Scroll regions** — reserved terminal areas for fixed UI elements (status bar)

### Longer-term

- **Layout dirty propagation** — child height change cascades to parent re-layout
- **Frozen buffer eviction** — LRU for long sessions
- **Non-vertical layout** — horizontal splits, flexible layout strategies
- **Accessibility** — screen reader support via terminal accessibility APIs

---

## Research Index

| Document | Contents |
|----------|----------|
| `prelim-research.md` | Landscape overview: Ratatui internals, Ink/Claude Code rendering, terminal escape sequences, pi-tui, Codex, prior art survey |
| `ratatui-modular-architecture.md` | Ratatui v0.30 crate split, Widget/Buffer/Layout APIs, standalone usage without Terminal |
| `codex-tui2-architecture.md` | HistoryCell trait, viewport pipeline, scroll anchors, high-water mark, streaming, display-time wrapping |
| `declarative-tui-patterns.md` | iocraft, tui-realm, cursive, dioxus-tui, bevy_ratatui, rxtui — API patterns and tradeoffs |
| `pi-tui-architecture.md` | Retained component model, computeDiffRange, writePartialRender, scrollback policy, DEC 2026 integration |
| `synthesis.md` | This document — current architecture, implementation status, and roadmap |
