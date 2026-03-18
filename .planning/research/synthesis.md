# Research Synthesis: eye_declare Architecture

## The Design Space

We're building a **declarative rendering library** for **inline** (non-alt-screen) terminal UIs, targeting an AI-native shell proxy built on Atuin Hex. The library must support multiple rendering modes — from growing inline conversations to fixed status bars — while remaining host-agnostic so it works both as a standalone renderer and as a component within Hex's PTY proxy architecture.

The research covers two mature reference architectures (pi-tui, Codex tui2), one mature declarative API pattern (iocraft), the Ratatui modular crate ecosystem, and the Atuin Hex PTY proxy internals.

---

## Target Application Context

**Atuin Hex** is a PTY proxy that transparently wraps the user's shell, tracking prompts/commands/outputs via OSC 133 semantic zones. **Atuin AI** provides English-to-shell generation and Q&A. The vision is fusing these into an **AI-native shell proxy**: use the shell normally, switch into natural language conversations where the agent has full context.

**UI rendering needs span a spectrum:**

- **Growing inline regions** (primary): AI conversations that appear inline, grow as messages stream in, and become normal scrollback when done. The coding agent pattern.
- **Fixed regions** (status bar): A bottom bar showing agent status, updated independently of shell output. Uses terminal scroll regions to reserve space.
- **Floating overlays** (popups): Config menus, autocomplete, agent status cards that appear over shell output temporarily.

**Deferred**: Full terminal multiplexing (side panes). Too complex, and carries the same scrollback/copy-paste issues as tmux.

---

## Reference Architectures

### pi-tui: Line-Level Retained Components
- Components are persistent objects returning `string[]` (ANSI-encoded lines)
- Components own their caching — `render(width)` returns cached output unless `invalidate()` was called
- Diff is line-level: find first changed line, rewrite from there to end
- Scrollback is implicit — content scrolls off the top naturally
- Full clear on resize (destroys scrollback)
- Simple, ~1200 LOC for the core

### Codex tui2: Cell-Level App-Owned Viewport
- `HistoryCell` trait as unit of content — returns `Vec<Line<'static>>`
- Two-layer cache: wrapped transcript lines + rasterized cell buffers
- Content-relative scroll anchors (`TranscriptLineMeta`) survive resizes
- Cell-level high-water mark for explicit scrollback printing
- Display-time-only wrapping via byte-range span slicing
- Uses alt-screen (not inline) — but the data model is portable

### Ratatui Primitives Available
- `ratatui-core`: Buffer, Cell, Rect, Layout, Style, Widget/StatefulWidget traits
- `ratatui-widgets`: All 16 built-in widgets render into `&mut Buffer` with zero backend deps
- `Buffer::diff()` is a standalone pure method — we can use it directly
- Layout is pure Rect math (Cassowary solver, cached)
- We do NOT need Terminal, Frame, or any backend crate

### Atuin Hex PTY Proxy
- Transparent PTY proxy with 4 threads: stdin→PTY, PTY→stdout, shadow vt100 parser, SIGWINCH handler
- OSC 133 zone tracking: knows when shell is in Prompt/Input/Output/Unknown states
- Unix socket API: child processes connect to request screen snapshots
- Pop-over mechanism: child process takes terminal, renders UI, restores screen from saved state
- No direct UI injection — child processes own the terminal during pop-overs

---

## Architectural Decisions

### Decision 1: Host Integration Model — Render-to-Buffer (Model C)

eye_declare is a **pure rendering engine** that does not own the terminal. It renders component trees into buffers and produces escape sequence output. The host application decides when and where to write that output.

**Layered API:**

```rust
// Layer 1: Component tree → Buffer (pure, no I/O)
let frame = renderer.render(width, height);

// Layer 2: Buffer → Diff (pure)
let diff = frame.diff(&previous_frame);

// Layer 3: Diff → escape sequences (pure, produces bytes)
let output: Vec<u8> = diff.to_escape_sequences(&mut cursor_state);
// Wrapped in DEC 2026 synchronized output

// Layer 4: Write to terminal (caller's responsibility)
// Standalone: stdout.write_all(&output)?;
// Hex: hex.inject_output(&output);
```

**Rationale:**
- Hex needs to composite multiple eye_declare render targets onto one terminal (inline UI, status bar, overlays)
- eye_declare stays testable without a real terminal
- The same library works in standalone mode (atuin ai chat) and embedded in Hex
- A convenience `Terminal` wrapper for the standalone/pop-over case is trivial sugar on top

### Decision 2: Rendering Modes

eye_declare supports three rendering modes, all built on the same "render into a rectangle" core:

**Mode 1 — Growing inline region** (primary use case):
- Component tree renders at full width, grows vertically as content accumulates
- Old content scrolls into terminal scrollback naturally (implicit scrollback, pi-tui model)
- Cursor position tracked by the renderer
- Frozen components stop contributing diffs, preventing "change above viewport" full-clears
- This is where streaming content, display-time wrapping, and the freeze lifecycle apply

**Mode 2 — Fixed region** (status bar, reserved space):
- Component tree renders into a fixed-size Rect that doesn't grow
- Content updates in place via cell-level diffing
- Host (Hex) manages terminal scroll regions to reserve space and handles placement
- eye_declare just renders into the Rect it's given

**Mode 3 — Floating overlay** (popups, config menus):
- Identical to Mode 2 from eye_declare's perspective (fixed Rect, diff in place)
- Host (Hex) manages save/restore of underlying content and overlay placement
- eye_declare doesn't know it's an overlay

**From eye_declare's perspective**, Mode 2 and 3 are the same — render into a fixed Rect. Mode 1 is the complex one with unique scrollback/growth/freeze semantics.

### Decision 3: Diff Granularity — Cell-Level Viewport Diffing

Components render into `Buffer` (Ratatui's cell grid). `Buffer::diff()` produces minimal cell-level changes within the viewport region.

**Rationale:**
- **Compatible with all existing Ratatui widgets** (Paragraph, List, Table, Block, tui-textarea, etc.)
- Sub-line precision for minimal terminal writes
- Performance bounded by viewport size: ~55μs for 200×50 terminal (kruci benchmarks)
- For Mode 1 (growing inline), only the viewport-sized buffer is diffed — not the full content history

### Decision 4: Component Model — Retained Components with Dirty Tracking

Foundation layer: retained components implementing a trait. Components are persistent objects that the framework calls `render()` on each frame, with dirty tracking to skip clean components.

**Designed for future declarative layer**: The retained component tree is structured to support a React-style declarative API (component functions + hooks + element macro) layered on top later. The declarative layer would produce and manage the retained component tree.

### Decision 5: Layout — Intrinsic Sizing with Two-Pass Measure/Layout

Components declare `desired_height(width)`. The framework queries components and allocates space in two passes: measure, then layout.

**Rationale:**
- Chat-style UIs need variable-height components (a message wraps to N lines at width W)
- Ratatui's Layout splits space without querying component sizes — insufficient
- Taffy/flexbox is overkill for primarily-vertical layouts
- Layout abstraction is pluggable — if complex layouts are needed later, Taffy can be added as an alternative layout strategy without changing the component model

### Decision 6: Scrollback Commit — Implicit with Freeze Points

Content scrolls into terminal scrollback naturally (pi-tui model). Components can be "frozen" — the framework stops re-rendering them, they contribute no diffs, and they never trigger full-clear paths.

**Freeze lifecycle:**
1. Component is live — rendered each frame, participates in diffing
2. Component content is complete (e.g., AI message finished streaming)
3. Component is frozen — cached output is returned, framework skips re-rendering
4. Content scrolls above viewport — becomes immutable terminal scrollback
5. On resize: only need to redraw from first non-frozen component

### Decision 7: Streaming Content — Display-Time-Only Wrapping

Components store logical content (not pre-wrapped). Wrapping is computed at render time based on current viewport width. Token-by-token updates invalidate only the streaming component. Cell-level diff handles the minimal update.

### Decision 8: Event/Input Handling — Minimal, Host-Delivered

eye_declare does NOT own the event loop. The host delivers events:

```rust
renderer.handle_event(event) -> EventResult
```

The framework routes events to the focused component with bubble-up propagation. Focus management is built in. The host (Hex or standalone wrapper) decides which events to deliver vs handle itself (e.g., Hex routes some keys to the shell, some to eye_declare).

A convenience `Terminal` wrapper for standalone use provides event loop integration with crossterm.

---

## Proposed Architecture Stack

```
┌─────────────────────────────────────────────────┐
│  Host Application                               │
│  (Hex PTY proxy, standalone CLI, etc.)          │
│  - Owns terminal I/O                            │
│  - Manages regions (scroll regions, overlays)   │
│  - Routes input events                          │
│  - Writes eye_declare output to terminal        │
├─────────────────────────────────────────────────┤
│  eye_declare: Convenience Layer (optional)      │
│  - Terminal wrapper for standalone use          │
│  - Event loop integration with crossterm        │
│  - Direct stdout writing                        │
├─────────────────────────────────────────────────┤
│  eye_declare: Component Framework               │
│  - Component trait (render, measure, event,     │
│    freeze, dirty tracking)                      │
│  - Component tree management                    │
│  - Intrinsic layout (measure/layout passes)     │
│  - Focus management & event routing             │
│  - Future: declarative API layer (hooks, macros)│
├─────────────────────────────────────────────────┤
│  eye_declare: Rendering Engine                  │
│  - Viewport Buffer management                   │
│  - Cell-level diffing (Buffer::diff)            │
│  - Escape sequence generation                   │
│  - Cursor state tracking                        │
│  - Synchronized output (DEC 2026) wrapping      │
│  - Growing-region mode (cursor mgmt, scrollback)│
│  - Fixed-region mode (in-place updates)         │
├─────────────────────────────────────────────────┤
│  ratatui-core         │  crossterm              │
│  (Buffer, Cell, Rect, │  (escape sequences,     │
│   Layout, Style,      │   event types,          │
│   Widget trait)       │   terminal queries)     │
└─────────────────────────────────────────────────┘
```

## Resolved Design Questions

### Q1: Component Tree Ownership — External (Framework-Managed)

The framework owns the tree structure. Components don't know about children — they only know how to render themselves and measure themselves. Users create components and hand them to the framework; the framework manages parent/child relationships via `NodeId` handles.

```rust
let message = AgentMessage::new("Hello world");
let id = renderer.root().append_child(Box::new(message));
```

**Rationale:** Enables the future React-style declarative layer (which needs a reconciler that manages the tree), avoids components needing to implement tree traversal, and makes dynamic tree manipulation straightforward.

### Q2: State Flow — External with Automatic Dirty Tracking

Component state is a separate associated type, owned and wrapped by the framework in `Tracked<S>`. Any `&mut` access to state automatically marks the component content-dirty via `DerefMut`.

```rust
pub struct Tracked<S> {
    inner: S,
    dirty: bool,
}

impl<S> DerefMut for Tracked<S> {
    fn deref_mut(&mut self) -> &mut S {
        self.dirty = true;
        &mut self.inner
    }
}
```

**Rationale:** Component authors never manually manage dirty flags. The framework detects mutations automatically. Small API cost (separate `State` type) for a big correctness win — no forgotten `set_dirty()` calls.

### Q3: Dirty Propagation — Two-Level (Content vs Layout)

Two distinct dirty flags per node:

- **Content dirty**: "my rendered output changed." Triggers re-render of this component's buffer region. Does NOT propagate upward. Common (streaming tokens, animations).
- **Layout dirty**: "my size changed." Triggers parent re-layout, which may cascade upward. Rare (content wrapping to a new line count).

Layout-dirty is detected by caching the last `desired_height` result and comparing after state mutation. Only layout changes propagate upward. This keeps propagation infrequent and O(tree depth) — negligible for the shallow trees in chat-style UIs.

### Q4: Resize Strategy — Re-render from Viewport Top, Preserve Scrollback

On terminal resize:
1. All components marked layout-dirty (width changed, wrapping changes)
2. Re-render from the topmost **visible** component downward
3. Write a full viewport update (entire viewport is stale after resize)
4. Frozen content already in scrollback stays at old wrapping — visual discontinuity at the boundary, accepted tradeoff (same as pi-tui and Codex tui2)

No `\x1b[3J` scrollback clear. Old scrollback stays intact with old wrapping. New content flows at new width.

### Q5: Frozen Component Buffers — Retain with Expiry

Keep a `HashMap<NodeId, Buffer>` for frozen components. Enables resize reflow when the buffer is available. Evict oldest entries when count/memory exceeds a threshold. If a frozen component's buffer has been evicted and is needed (e.g., it scrolls back into the viewport during resize), re-render it on demand — "frozen" means content won't change, not that the component is gone.

---

## Core API Sketch

```rust
/// The result of handling an event.
pub enum EventResult {
    /// Event was consumed by this component.
    Consumed,
    /// Event was not handled; propagate to parent.
    Ignored,
}

/// A component that can render itself into a terminal region.
/// Components are stateless renderers — state lives in the
/// associated State type, managed by the framework.
pub trait Component: Send + Sync {
    /// State type for this component. Framework wraps it in
    /// Tracked<S> for automatic dirty detection.
    type State: Send + Sync + 'static;

    /// Render into the given buffer region using current state.
    /// Can use any ratatui Widget internally.
    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State);

    /// How tall at the given width? None = fill available space.
    fn desired_height(&self, width: u16, state: &Self::State) -> Option<u16>;

    /// Handle an input event, potentially mutating state.
    fn handle_event(
        &self,
        event: &Event,
        state: &mut Self::State,
    ) -> EventResult {
        EventResult::Ignored
    }

    /// Create the initial state for this component.
    fn initial_state(&self) -> Self::State;
}

// --- Framework-managed tree node (internal) ---

struct Node {
    component: Box<dyn AnyComponent>,   // type-erased Component
    state: Box<dyn AnyTrackedState>,    // Tracked<S>, type-erased
    children: Vec<NodeId>,
    parent: Option<NodeId>,
    cached_buffer: Option<Buffer>,
    frozen: bool,
    content_dirty: bool,
    layout_dirty: bool,
    last_height: Option<u16>,           // cached desired_height result
    layout_rect: Option<Rect>,          // assigned by parent/framework
}

// --- Public renderer API ---

/// Manages a component tree and produces frame output.
/// The host creates one Renderer per rendering region.
pub struct Renderer { /* ... */ }

impl Renderer {
    /// Create a new renderer with a root component.
    pub fn new(root: impl Component + 'static) -> Self { ... }

    /// Access the tree for structural manipulation.
    pub fn tree(&mut self) -> &mut Tree { ... }

    /// Render the component tree at the given dimensions.
    /// Returns ready-to-use frame output.
    pub fn render(&mut self, width: u16, height: u16) -> Frame { ... }

    /// Deliver an event to the focused component.
    pub fn handle_event(&mut self, event: &Event) -> EventResult { ... }

    /// Freeze a component (content finalized, skip future re-renders).
    pub fn freeze(&mut self, id: NodeId) { ... }

    /// Notify that terminal was resized. Marks all components
    /// layout-dirty and triggers full viewport re-render on next
    /// render() call.
    pub fn resize(&mut self) { ... }
}

/// Tree manipulation handle.
pub struct Tree { /* ... */ }

impl Tree {
    pub fn root(&self) -> NodeId { ... }
    pub fn append_child(
        &mut self,
        parent: NodeId,
        component: impl Component + 'static,
    ) -> NodeId { ... }
    pub fn remove(&mut self, id: NodeId) { ... }
    pub fn insert_before(
        &mut self,
        sibling: NodeId,
        component: impl Component + 'static,
    ) -> NodeId { ... }
}

/// Output of a render pass.
pub struct Frame { /* buffer contents */ }

impl Frame {
    /// Diff against previous frame.
    pub fn diff(&self, previous: &Frame) -> Diff { ... }
}

/// Changes between two frames.
pub struct Diff { /* changed cells */ }

impl Diff {
    /// Produce ready-to-write terminal escape sequences,
    /// wrapped in DEC 2026 synchronized output.
    pub fn to_escape_sequences(
        &self,
        cursor: &mut CursorState,
    ) -> Vec<u8> { ... }

    /// Whether any cells actually changed.
    pub fn is_empty(&self) -> bool { ... }
}

// --- Convenience layer for standalone use ---

/// Wraps a Renderer with a crossterm event loop and direct
/// stdout writing. For use outside of Hex / PTY proxy contexts.
pub struct Terminal { /* ... */ }

impl Terminal {
    pub fn new(renderer: Renderer) -> Self { ... }

    /// Run the event loop. Handles rendering, input delivery,
    /// resize events, and DEC 2026 synchronized writes to stdout.
    pub async fn run(&mut self) -> io::Result<()> { ... }
}
```

## Research Index

| Document | Contents |
|----------|----------|
| `prelim-research.md` | Landscape overview: Ratatui internals, Ink/Claude Code rendering, terminal escape sequences, pi-tui, Codex, prior art survey |
| `ratatui-modular-architecture.md` | Ratatui v0.30 crate split, Widget/Buffer/Layout APIs, standalone usage without Terminal |
| `codex-tui2-architecture.md` | HistoryCell trait, viewport pipeline, scroll anchors, high-water mark, streaming, display-time wrapping |
| `declarative-tui-patterns.md` | iocraft, tui-realm, cursive, dioxus-tui, bevy_ratatui, rxtui — API patterns and tradeoffs |
| `pi-tui-architecture.md` | Retained component model, computeDiffRange, writePartialRender, scrollback policy, DEC 2026 integration |
| `synthesis.md` | This document — consolidated decisions, architecture stack, core API sketch |
