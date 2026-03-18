# Declarative inline TUI rendering: a technical reference

Building a declarative rendering model on top of Ratatui for inline (non-alt-screen) terminal UIs requires navigating a specific set of architectural tradeoffs. **The industry is converging on inline rendering with cell-based differential updates** as the correct approach for agent-style TUI applications — Claude Code rewrote its renderer around this, OpenAI Codex owns viewport state in-memory, and Google's Gemini CLI rolled back an alt-screen TUI within a week of launch after user backlash. The core tension: Ratatui's immediate-mode double-buffer diffing is simple and correct but can consume **50% of frame time** on large buffers, while Ink's erase-and-rewrite approach causes visible flicker. The path forward combines retained-mode dirty tracking, synchronized output (DEC mode 2026), and viewport-only diffing.

---

## 1. Ratatui internals

### Inline viewport mode (`Viewport::Inline`) and its sharp edges

**Link:** [Ratatui Viewport docs](https://docs.rs/ratatui/latest/ratatui/enum.Viewport.html) · [Inline example](https://github.com/ratatui/ratatui/blob/main/examples/inline.rs)

`Viewport::Inline(height)` allocates a fixed-height rectangular region at the cursor's current position when the terminal is initialized, drawn inline with the rest of the terminal output rather than in alternate screen mode. Width matches the terminal; height is specified in lines. On a 200×50 terminal starting at line 2 with `Inline(10)`, the renderable `Rect` is `{ x: 0, y: 2, width: 200, height: 10 }`.

The viewport is a purely positional abstraction. There is **no support for viewports larger than the terminal** — `Rect` uses `u16` for all coordinates, making it impossible to express negative or off-screen positions. Once content scrolls into the terminal's scrollback buffer, it is outside the application's control entirely.

Known issues that directly affect inline renderer design:

- **Resize bugs** ([#2086](https://github.com/ratatui/ratatui/issues/2086)): Inline viewport resizing doesn't handle horizontal terminal resizes correctly. Even calling `terminal.autoresize()` on `Event::Resize` doesn't reposition the viewport properly.
- **Flickering at high throughput** ([#584](https://github.com/ratatui/ratatui/issues/584)): Rapid `insert_before()` calls cause visible flicker. The `scrolling-regions` feature flag mitigates this by using terminal scroll regions instead of clear-and-redraw.
- **No scrollback modification**: Content pushed above the viewport via `insert_before()` cannot be modified or removed. There is no `delete_before()` API. This is confirmed in [PR #1964](https://github.com/ratatui/ratatui/pull/1964).
- **Viewport height adjustment**: PR #1964 added `set_viewport_height` for inline terminals since the existing `resize` method required x, y, and width parameters that are difficult to reliably obtain in inline mode.

**Key takeaway:** Inline viewport is a thin abstraction over cursor positioning. For a declarative renderer, you either accept its constraints (fixed rectangle, no scrollback modification) or, like Codex, own the viewport entirely and treat the terminal as a dumb output target.

### The `insert_before()` API

**Link:** [Terminal::insert_before docs](https://docs.rs/ratatui/latest/ratatui/struct.Terminal.html)

```rust
pub fn insert_before<F>(&mut self, height: u16, draw_fn: F) -> io::Result<()>
where F: FnOnce(&mut Buffer)
```

This method inserts rendered content above the inline viewport. The closure receives a `Buffer` sized to `(terminal_width, height)`. If the viewport hasn't reached the bottom of the screen, inserted lines push it downward. Once at the bottom, inserted lines scroll content upward into scrollback.

The API enables "append-only log" style UIs with native terminal scrollback, but has critical limitations for agent-style applications:

- **Fixed height required** ([#1365](https://github.com/ratatui/ratatui/issues/1365)): You must specify the exact line count upfront. Content from `Paragraph::wrap()` that exceeds the specified height is silently truncated. The workaround is to pre-calculate wrapped line count before calling `insert_before`, which is fragile.
- **No terminal-native wrapping** ([#1426](https://github.com/ratatui/ratatui/issues/1426)): Content renders into a width-constrained buffer, so overlong lines cannot be output as raw text for the terminal to wrap and re-wrap on resize. A proposed `insert_lines_before` method would let the terminal handle wrapping natively.
- **One-directional**: Content in scrollback is immutable from the application's perspective. This makes it impossible to update "not fully baked" content (spinners, in-progress tool calls) once pushed above the viewport, as the [Codex team identified](https://github.com/openai/codex/issues/1247).

**Key takeaway:** `insert_before()` is the only mechanism for populating scrollback in inline mode. Its fixed-height requirement and immutability make it unsuitable for dynamic content. A declarative renderer must decide early whether scrollback content is "committed" (never updated) or "live" (requires owning the viewport).

### Double-buffer diffing architecture

**Link:** [Rendering under the hood](https://ratatui.rs/concepts/rendering/under-the-hood/) · [kruci post-mortem](https://pwy.io/posts/kruci-post-mortem/)

Ratatui maintains two `Buffer` instances: `current` (being drawn to) and `previous` (what was last flushed). The rendering cycle works as follows:

1. The current buffer is wiped clean (filled with default cells)
2. `terminal.draw(|frame| ...)` runs — all widgets render into the current buffer
3. `Terminal::flush()` calls `Buffer::diff()` comparing current vs previous
4. Only changed cells are written to the terminal via the backend
5. Buffers are swapped; the now-previous buffer becomes the template for next frame's diff

The diff implementation is a linear O(width × height) scan:

```rust
pub fn diff(&self, other: &Self) -> Vec<(u16, u16, &Cell)> {
    let mut updates = vec![];
    for (i, (current, previous)) in next_buffer.iter().zip(previous_buffer.iter()).enumerate() {
        if current != previous {
            let (x, y) = self.pos_of(i);
            updates.push((x, y, &next_buffer[i]));
        }
    }
    updates
}
```

The `Cell` type stores a `CompactString` (inline-buffer-optimized string for Unicode grapheme clusters), foreground/background/underline colors, and modifier flags. Cell comparison via `PartialEq` checks all fields.

**Performance characteristics** from the kruci post-mortem: `Buffer::diff()` can consume **~50% of frame time** on some views. The primary bottleneck is `CompactString` comparison (pointer chasing for Unicode) and calling `.width()` on each symbol. [PR #1339](https://github.com/ratatui/ratatui/pull/1339) cached symbol width in Cell, yielding a **~55% speedup of the diff itself** (from ~120μs to ~55μs on a 200×50 terminal). Dropping Unicode support entirely halved overall frame time — though this is unacceptable for most applications.

The diff output `Vec<(u16, u16, &Cell)>` is consumed by the backend (e.g., CrosstermBackend), which moves the cursor via escape sequences, sets style attributes, and writes cell symbols. Consecutive changed cells on the same line are batched into a single write to avoid redundant cursor moves.

**Key takeaway:** The double-buffer diff is correct and simple but makes inline rendering expensive for large viewports. A declarative renderer should consider dirty-flag-based incremental updates to avoid full-buffer diffing. The kruci author explored a retained-mode widget tree where only dirty widgets repaint, avoiding the diff entirely — but abandoned it due to complexity. The fundamental insight: "we need to diff because every frame we get a brand new buffer. What if the UI just knew how to update the terminal incrementally?"

### GitHub discussions on architectural limitations

**Discussion #552: "Problems using ratatui"**
**Link:** [ratatui/discussions/552](https://github.com/ratatui/ratatui/discussions/552)

This October 2023 discussion by arxanas is the most thorough articulation of Ratatui's architectural limitations for building complex UIs. The five core issues identified:

1. **Relative positioning**: Cannot render widget Y below widget X without knowing X's size in advance. Layout doesn't account for rendered sizes at layout time.
2. **Signed coordinates**: `Rect` uses unsigned `u16`, making negative coordinates (needed for scrolling) impossible. arxanas implemented a virtual canvas internally and copies the visible portion to the terminal — the same approach `tui-scrollview` later formalized.
3. **Masking/compositing**: No built-in support for partial widget visibility (clipped scrolling views).
4. **Hit testing**: After drawing, there's no framework-level way to map screen coordinates to widgets. Requires manual bookkeeping.
5. **Event handling/focus**: Requires widget identity and a retained drawing hierarchy — "all retained mode concepts, seriously missing in immediate mode."

Joshka's response acknowledged all points and proposed a `Viewport` widget providing a virtual buffer for widgets to render into at 0,0 coordinates, with coordinate remapping to screen space. This concept became `tui-scrollview`.

**RFC #174: Design of scrollable widgets**
**Link:** [ratatui/issues/174](https://github.com/ratatui/ratatui/issues/174)

Joshka's May 2023 RFC examined four approaches to adding scrolling: a `ScrollableWidget` wrapper, a `Scrollable` trait, state-implements-scrollable, and external `ScrollState`. The RFC led to `tui-scrollview` as an external crate and the `Scrollbar` widget being added to Ratatui core.

### The `tui-scrollview` crate

**Link:** [joshka/tui-scrollview](https://github.com/joshka/tui-scrollview) · [docs.rs](https://docs.rs/tui-scrollview/latest/tui_scrollview/) · Now part of [ratatui/tui-widgets](https://github.com/ratatui/tui-widgets)

Created by joshka, `tui-scrollview` implements the virtual canvas approach directly. A `ScrollView::new(content_size)` creates a buffer larger than the visible area. Widgets render into it at logical coordinates. `ScrollViewState` tracks the scroll offset, and the `render` method copies the visible window to the screen buffer.

```rust
let mut scroll_view = ScrollView::new(Size::new(100, 300));
scroll_view.render_widget(Paragraph::new(content), Rect::new(0, 0, 100, 300));
scroll_view.render(area, buf, &mut state);  // copies visible portion
```

The tradeoff: **all content is rendered regardless of visibility** (no culling). The entire virtual buffer must fit in memory. For a 100×10,000 cell content area, that's 1 million `Cell` allocations. There's no lazy or virtualized rendering. For a declarative inline renderer handling long agent transcripts, this pattern works but needs augmentation with viewport-only rendering for performance.

### OpenAI Codex's fork: the `tui2` architecture

**Link:** [openai/codex](https://github.com/openai/codex) · [PR #7601](https://github.com/openai/codex/pull/7601)

The Codex repository contains two TUI implementations: `codex-rs/tui/` (legacy, currently shipping) and `codex-rs/tui2/` (experimental redesign by joshka-oai). PR #7601, opened December 2025 as a draft, represents the most significant architectural rework.

**The core shift** is from "cooperating" with terminal scrollback to treating the **in-memory transcript as the single source of truth**. The legacy TUI's cooperation with scrollback caused terminal-dependent behavior, resize failures, and content loss during transitions.

The new model's key principles:

The transcript is a list of **cells** (user prompts, agent messages, system rows, streaming segments), each implementing a `HistoryCell` trait:

```rust
pub(crate) trait HistoryCell: Debug + Send + Sync + Any {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;
    fn desired_height(&self, width: u16) -> u16;
    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>>;
    fn is_stream_continuation(&self) -> bool { false }
}
```

Each frame computes the transcript region (terminal minus input area), flattens all cells into visual lines with the current viewport width, and renders the visible slice based on scroll state. **Terminal scrollback is never part of the live layout algorithm** — it's only written to on suspend or exit.

**Display-time-only wrapping** fixed a critical streaming bug: the old TUI wrapped streamed markdown at a fixed width at commit time, causing permanent line splits that couldn't un-wrap on resize. The new approach buffers markdown source, commits at newline boundaries, and wraps only during rendering at the current viewport width. Streamed responses reflow correctly on terminal resize.

**History printing** uses a cell-based high-water mark (`printed_history_cells`). On suspend (Ctrl+Z), unprinted cells are rendered to scrollback, the high-water mark advances, and on resume the TUI redraws from the in-memory transcript. Each logical cell is printed to scrollback **at most once**. The tradeoff: older printed cells may have different wrapping than newer ones.

**Design documents** (`tui_viewport_and_history.md`, `streaming_wrapping_design.md`) are in the `codex-rs/tui2/docs/` directory and formalize these principles.

### Josh McKinney (joshka) and the bridge between Ratatui and Codex

**Link:** [github.com/joshka](https://github.com/joshka) · [github.com/joshka-oai](https://github.com/joshka-oai)

Josh McKinney is the primary maintainer of Ratatui who joined OpenAI in October 2025. He led Ratatui's modularization in v0.30.0 (splitting into ratatui-core, ratatui-crossterm, etc.), created `tui-markdown` (used by Codex for markdown rendering), `tui-scrollview`, and `bevy_ratatui` (an ECS-based terminal UI experiment). OpenAI officially funded Ratatui as an open-source grant recipient in May 2025.

His key insight on inline rendering limitations, from [issue #1964](https://github.com/ratatui/ratatui/pull/1964): "To the best of my knowledge, there's no way a viewport larger than the terminal can work in Ratatui. The abstraction is purely that an inline viewport is an allocation of a fixed rectangular area positioned on the screen." This limitation is what drove Codex's decision to own the viewport entirely.

In a Ratatui forum discussion on redesigns (November 2024), joshka noted: "If I'd redesign Ratatui entirely based on current knowledge: Widgets would implement multiple methods for layout, handling events, rendering."

---

## 2. Ink and React-terminal rendering approaches

### Boris Cherny's rendering rewrite announcement

**Link:** [threads.com/@boris_cherny/post/DSZbZatiIvJ/](https://www.threads.com/@boris_cherny/post/DSZbZatiIvJ/) (December 18, 2025)

Boris Cherny, Head of Claude Code at Anthropic (formerly Principal Engineer at Meta, author of O'Reilly's *Programming TypeScript*), posted a detailed thread describing Claude Code's rendering rewrite. The key framing: terminals have two regions — the **viewport** at the bottom and the **scrollback buffer** above it. When content exceeds viewport height, the top row gets pushed into scrollback and rendering happens offscreen. Unlike CLI tools that print and exit, Claude Code is a long-running interactive UI that redraws dozens of times per second. When rendering offscreen or during terminal resizes, clearing the scrollback each time creates flicker.

The solution: **"We now diff each cell and emit the minimal escape sequences needed to update what changed."** This achieved an **~85% reduction in flickering**. The rewrite was verified using property-based testing that generated thousands of random UI states — different widths, content lengths, Unicode edge cases — confirming the new renderer matched the old one exactly. Boris noted that Claude Code itself was used to build the fix, with the property-based tests serving as the automated verification loop.

### Ink's rendering architecture and why it flickers

**Link:** [vadimdemedes/ink](https://github.com/vadimdemedes/ink) · [Ink flickering analysis](https://github.com/atxtechbro/test-ink-flickering/blob/main/INK-ANALYSIS.md)

Ink is a React renderer for terminals using Yoga for Flexbox layouts. Its rendering pipeline on every React state change:

1. React reconciler triggers `rootNode.onRender()` after every commit
2. `renderNodeToOutput()` performs a recursive full-tree traversal of all nodes
3. A complete 2D buffer is built and an output string generated
4. `ansiEscapes.eraseLines(previousLineCount)` erases all previous lines
5. The complete new output is written

The critical code in Ink's `log-update.ts`:

```javascript
const render = (str) => {
  stream.write(ansiEscapes.eraseLines(previousLineCount) + output);
  previousLineCount = output.split('\n').length;
};
```

This is a fundamental architectural limitation. Even when only a timer updates from "0.1s" to "0.2s", Ink traverses all nodes, builds a complete buffer, erases all previous lines, and rewrites everything. The visible erase-redraw cycle is the source of flicker. As conversation history grows, the number of erased and rewritten lines increases proportionally. Ink's `<Static>` component renders items permanently to stdout once (never re-rendered) — the closest thing to "committed" scrollback — but it's too rigid for agent-style applications where content may still be streaming.

### The cell-based diffing architecture in depth

**Link:** [DEV.to: "Do Androids Dream of O(n) Diffs?"](https://dev.to/vmitro/i-profiled-claude-code-some-more-part-2-do-androids-dream-of-on-diffs-2kp6) (January 21, 2026, by vmitro)

This profiling article reveals the implementation details and performance characteristics of Claude Code's cell-based diffing. Two functions consumed **~45% of CPU time**: `Qt1` (damage region calculation / cell diffing) and `get` (screen buffer rebuilding). The `get()` method allocates a **fresh buffer every frame** of size `yogaNode.getComputedHeight()` × width — critically, this is the full flexbox-laid-out content height (entire conversation), not just the viewport.

At scale: **500 messages × 80 columns = 40,000 cells diffed per frame**, with no list virtualization — every message is a React component walked on every reconciliation pass. The article documents a fundamental pendulum:

| Version | Diff + Buffer CPU | Visual result |
|---|---|---|
| 2.1.12 (late session) | ~48% combined | No flicker, but progressively slower |
| 2.1.14 (partial revert) | ~22% combined | Flicker returns |

The cell-based diffing eliminated flicker but caused sessions to become **progressively slower** as conversation history grew (memory climbing at 29 MB/min, typing lag at 500 MB). The author's verdict: the I/O fixes are trivial single-line patches, but the render architecture is "the load-bearing wall. You don't patch around it, you rebuild the house."

The author built [bukowski](https://github.com/vmitro/bukowski), a tool that captures Claude Code's output, composites frames externally, and emits them with DEC 2026 synchronized update sequences — proving that synchronized output alone can eliminate flicker without architectural changes, though it doesn't address the underlying performance scaling issue.

### Claude Code GitHub issues on rendering

**Link:** [#769](https://github.com/anthropics/claude-code/issues/769) · [#28077](https://github.com/anthropics/claude-code/issues/28077) · [#34794](https://github.com/anthropics/claude-code/issues/34794)

Issue #769 ("In-progress Call causes Screen Flickering," April 2025) is the canonical rendering bug, assigned to Chris Lloyd at Anthropic and referenced by Boris Cherny's post. Issue #28077 reveals that Claude Code uses the **alternate screen buffer** — meaning Ghostty's scrollback settings have no effect and users cannot scroll back through conversation history. This is the architectural tension: alt-screen avoids scrollback pollution but eliminates scroll-back capability. Issue #34794 contains an excellent technical breakdown of Ink's rendering, showing how `eraseLines(this.height)` moves the cursor far up in the terminal buffer on large outputs, causing the viewport to jump.

Other notable issues: #29937 (tmux rendering corruption from position drift), #18084 (VS Code/xterm.js severe flickering), #17547 (subagent swarm causes flicker-thrashing when TUI exceeds screen buffer height), and #10619 — a desperate user plea that captures the user impact.

### Peter Steinberger's "The Signature Flicker"

**Link:** [steipete.me/posts/2025/signature-flicker](https://steipete.me/posts/2025/signature-flicker) (December 17, 2025)

This post is the definitive landscape overview of terminal rendering for coding agents. Steinberger frames the choice as a false dichotomy: alt-screen mode eliminates flicker entirely but breaks native text selection, scrolling, and search. Inline/differential rendering preserves terminal-native behavior but requires careful implementation. His survey of the ecosystem:

- **Claude Code**: Rewrote renderer on top of Ink, inline mode, eliminated most flicker
- **Codex (OpenAI)**: Stays in primary screen buffer — "behaves like a terminal"
- **pi (Mario Zechner)**: "Gold standard for differential rendering" — synchronized output, inline images, all modern terminal features while staying inline
- **Amp**: Switched from Ink to alt-screen (September 2025). `find` fails unless text is on-screen; no native selection
- **Gemini CLI**: Launched alt-screen, rolled back within a week
- **OpenCode**: Built `opentui` (Zig), renders SolidJS/React in alt-screen. Doesn't work in macOS Terminal (pre-macOS 26) or GNOME Terminal

Quote from Thariq (Anthropic) referenced in the post: "This is kind of like if a website were to do their own text rendering, highlighting, mouse movement, context menu — it would not feel like your browser. We value this native experience a lot."

Steinberger also referenced the Codex design doc `tui_viewport_and_history.md` and ported pi-tui to Swift as [TauTUI](https://github.com/steipete/TauTUI).

### Mario Zechner's pi-tui: the gold standard

**Link:** [mariozechner.at/posts/2025-11-30-pi-coding-agent/](https://mariozechner.at/posts/2025-11-30-pi-coding-agent/) (November 30, 2025)

Zechner's write-up contains the most detailed technical explanation of differential inline rendering. His architecture uses a retained-mode component model where components cache their output — a fully-streamed assistant message doesn't re-render each frame. The differential algorithm:

1. **First render**: Output all lines
2. **Width changed**: Full clear and re-render (soft wrapping changes invalidate everything)
3. **Normal update**: Find the first line that differs from the backbuffer → move cursor there → re-render from that point to end
4. **Scrollback limitation**: If the first changed line is above the visible viewport (user scrolled up), a full clear and re-render is required since terminals don't allow writing to scrollback above the viewport

All rendering wraps in **synchronized output sequences** (`CSI ?2026h` / `CSI ?2026l`), producing atomic frame displays. Memory for the backbuffer is "a few hundred kilobytes for very large sessions" — negligible. Zechner rejected existing frameworks: Ink because "I definitely don't want to write my TUI like a React app," Blessed as unmaintained, and OpenTUI as not production-ready.

**Key takeaway:** Pi-tui demonstrates that retained-mode components with line-level caching plus synchronized output produces excellent results with minimal complexity. The "find first changed line" algorithm is dramatically simpler than cell-level diffing while being adequate for chat-style UIs.

### Amp's switch to alt-screen

**Link:** [ampcode.com/news/look-ma-no-flicker](https://ampcode.com/news/look-ma-no-flicker) (September 2, 2025)

Amp initially used Ink and suffered the same flicker problems. Their September 2025 switch to a custom fullscreen TUI framework in alt-screen mode eliminated flicker entirely and enabled smooth scrolling, mouse interactions, overlays, and popups. However, as Steinberger documented, this broke terminal-native `find`, text selection, and context menu flow. They maintain an Ink-based fallback CLI (`pnpm -C cli cli:ink`), suggesting awareness of the tradeoffs.

### Google Gemini CLI: the cautionary tale

**Link:** [Blog announcement](https://developers.googleblog.com/making-the-terminal-beautiful-one-pixel-at-a-time/) (Nov 13, 2025) · [Issue #13074](https://github.com/google-gemini/gemini-cli/issues/13074) · [Issue #13161](https://github.com/google-gemini/gemini-cli/issues/13161)

Google published "Making the terminal beautiful one pixel at a time" announcing a major TUI overhaul with alt-screen rendering, mouse navigation, embedded scrollbar, and no flicker. Within days, users filed priority-1 bugs: **copy/paste completely broken** (could not select or copy text from responses), required Ctrl+S to enter selection mode, scrollback unavailable, and some terminals hung on initialization.

Google **rolled back the TUI within approximately one week**. The changelog notes: "We've temporarily rolled back our updated UI to give it more time to bake. This means for a time you won't have embedded scrolling or mouse support. You can re-enable with /settings → Use Alternate Screen." The alt-screen mode remains available as an opt-in setting, not the default.

This is the strongest evidence that **alt-screen is the wrong default for agent-style applications**. The fundamental terminal workflow of copy/paste and scrollback is non-negotiable for most users.

---

## 3. Terminal escape sequence optimization

### DEC mode 2026: synchronized output

**Link:** [Specification](https://github.com/contour-terminal/vt-extensions/blob/master/synchronized-output.md) · [Original gist](https://gist.github.com/christianparpart/d8a62cc1ab659194337d73e399004036)

Synchronized output is the single most impactful optimization for preventing visual tearing. The mechanism:

- **BSU** (Begin Synchronized Update): `\x1b[?2026h` — the terminal continues processing text and escape sequences internally but defers rendering, keeping the last fully-rendered state visible
- **ESU** (End Synchronized Update): `\x1b[?2026l` — the terminal atomically renders the latest grid buffer state

Feature detection uses DECRQM: send `\x1b[?2026$p` and parse the response mode value (0 = not supported, 2 = supported and inactive, 4 = permanently disabled). However, the practical recommendation is to **just send the sequences without detection** — unsupported escape codes are ignored by definition, and detection adds round-trip latency.

Timeout behavior varies: Windows Terminal uses 100ms, tmux and xterm.js use 1 second. The specification notes that a too-short timeout on slow connections is still no worse than having no synchronized output at all.

### Terminal emulator support matrix

Support is now nearly universal across modern terminals:

| Terminal | Status | Notes |
|---|---|---|
| **Ghostty** | ✅ | Supported since 1.0.0 (Dec 2024) |
| **iTerm2** | ✅ | Original proposer; migrated from DCS to Mode 2026 syntax |
| **WezTerm** | ✅ | Tracked via wezterm#882 |
| **Kitty** | ✅ | Since commit 5768c54c |
| **Alacritty** | ✅ | Since v0.13.0 |
| **Windows Terminal** | ✅ | Since v1.23 (Jan 2026), 100ms timeout |
| **VSCode terminal** | ✅ | Via [xterm.js PR #5453](https://github.com/xtermjs/xterm.js/pull/5453) by Chris Lloyd (Anthropic) |
| **tmux** | ✅ | Via [PR #4744](https://github.com/tmux/tmux/pull/4744) by Chris Lloyd, 1s timeout |
| **Zellij** | ✅ | PR zellij-org/zellij#2977 |
| **foot** | ✅ | Codeberg.org/dnkl/foot#459 |
| **Warp** | ✅ | Since v0.2025.01.15 |
| **Contour** | ✅ | Specification host |
| **GNOME Terminal/VTE** | ❌ | Responds with DECRPM value 4 (permanently disabled) |
| **Apple Terminal.app** | ❌ | Does not support DECRQM at all |

Both the xterm.js and tmux PRs were authored by Chris Lloyd from Anthropic's Claude Code team and were generated with Claude Code. As noted in a Hacker News comment: "We've been working upstream to add synchronized output / DEC mode 2026 support to environments where CC runs and have had patches accepted to VSCode's terminal and tmux."

### The OpenTUI project

**Link:** [anomalyco/opentui](https://github.com/anomalyco/opentui) (Zig original) · [Dicklesworthstone/opentui_rust](https://github.com/Dicklesworthstone/opentui_rust) (Rust port)

OpenTUI is a native terminal UI core written in Zig with TypeScript bindings, powering OpenCode in production. The Rust port is a **rendering engine, not a framework** — it provides double-buffered rendering with diff detection but no widget tree, layout system, or event loop.

Key differences from Ratatui's rendering approach:

| Aspect | Ratatui | OpenTUI Rust |
|---|---|---|
| Rendering model | Immediate-mode with full-frame diff | Double-buffered with cell diff + compositing |
| Cell storage | `CompactString` (Unicode grapheme clusters) | Grapheme pool (reference-counted) |
| Alpha/compositing | None | Real RGBA Porter-Duff "over" compositing |
| Clipping | Manual via `Rect` bounds | Scissor clip stack (push/pop rectangles) |
| Opacity | None | Opacity stack (push/pop modifiers) |
| Sync output | Optional | Enabled by default (`\x1b[?2026h`) |

OpenTUI Rust's grapheme pool approach to cell storage — using reference-counted grapheme clusters rather than per-cell inline strings — could reduce comparison overhead versus Ratatui's `CompactString`. Its scissor clipping and opacity stack are relevant for building composited views with overlapping regions.

### Damage tracking and cursor movement optimization

The goal of damage tracking is to minimize the bytes sent to the terminal on each frame. The approaches seen across projects, ordered by sophistication:

**Line-level diffing** (pi-tui): Find the first line that differs from the backbuffer, rewrite from that point to the end. Simple, O(lines) comparison, adequate for chat-style UIs where changes typically occur at the bottom.

**Cell-level diffing** (Ratatui, Claude Code, OpenTUI): Compare each cell against the previous frame, emit cursor moves and writes only for changed cells. O(width × height) comparison but produces minimal output. Ratatui's implementation outputs a sorted `Vec<(x, y, &Cell)>` consumed by the backend.

**Retained-mode dirty tracking** (kruci experiment): Widgets track their own dirty state. Only dirty widgets repaint to the buffer. Avoids the full-buffer diff entirely but requires explicit state management. The kruci post-mortem explored this and abandoned it due to complexity, but noted it would eliminate the diff bottleneck.

Cursor movement optimization techniques:

- **Batch consecutive changes**: Write adjacent changed cells as a continuous string with one cursor position rather than individual cell writes
- **Use relative moves when cheaper**: `\x1b[C` (forward 1 column, ~3 bytes) versus `\x1b[5;10H` (absolute position, ~7 bytes)
- **Hide cursor during rendering**: `\x1b[?25l` before frame, `\x1b[?25h` after, to avoid cursor flicker
- **Track current style state**: Only emit SGR codes when style actually changes. Combine parameters (`\x1b[1;31;42m` instead of three separate sequences). Use the shortest color representation (standard 3-bit > 256-color > truecolor)
- **Buffer all writes**: Wrap stdout in `BufWriter`; accumulate the entire frame's output and flush once. This reduces system calls from potentially thousands to one

### The bukowski tool

**Link:** [vmitro/bukowski](https://github.com/vmitro/bukowski)

Built by the author of the DEV.to profiling article, bukowski is a PTY proxy that captures an application's terminal output, composites frames, and emits them wrapped with DEC 2026 synchronized update sequences. It proved that synchronized output alone can eliminate flicker without modifying the application — though it doesn't address underlying performance scaling issues. A similar tool, [claude-chill](https://github.com/davidbeesley/claude-chill), takes the same approach as a purpose-built wrapper for Claude Code.

---

## Key architectural decisions

### The viewport ownership question

The fundamental architectural decision is who owns the viewport state. Three models have emerged:

**Terminal-cooperative** (Ratatui inline viewport): The application allocates a fixed region and coordinates with the terminal's scrollback. Simple but fragile — terminal-dependent behavior, resize failures, and no way to modify committed scrollback content.

**Application-owned** (Codex tui2): The in-memory transcript is the single source of truth. Scrollback is an append-only output target written to on suspend/exit, never part of the live layout. Trades scrollback fidelity (older cells may have different wrapping) for complete rendering control and resize resilience.

**Hybrid** (Claude Code, pi-tui): Application manages the viewport region with differential rendering, using the terminal's native scrollback as-is for committed content. Preserves terminal-native scrolling and selection but accepts that the application cannot modify scrollback content.

### The diff granularity spectrum

The right diff granularity depends on the application's update pattern:

**Full-frame erase+rewrite** (Ink): Zero implementation complexity, maximum flicker. Only acceptable with synchronized output on terminals that support it, and even then wastes bandwidth.

**Line-level diff** (pi-tui): Find first changed line, rewrite from there. O(lines) comparison, trivially simple, adequate for chat-style UIs. Combined with component-level caching, this produces excellent performance with minimal implementation effort.

**Cell-level diff** (Ratatui, Claude Code): O(width × height) comparison, produces minimal escape sequences. The correct choice when multiple parts of the viewport change independently (e.g., a spinner in one area and streaming text in another). **But the cost scales with total diffed area** — viewport-only diffing is essential to avoid the progressive slowdown Claude Code experienced.

**Dirty-region tracking** (kruci experiment, retained-mode): Widgets declare when they've changed; only changed regions are re-rendered and diffed. Minimal overhead but highest implementation complexity. The right choice for a declarative framework where the component tree provides natural dirty-tracking boundaries.

### What synchronized output does and doesn't solve

DEC mode 2026 prevents tearing — the terminal displays frames atomically rather than progressively. This eliminates the visual flicker from partial rendering. However, it does **not** reduce the computational cost of building or diffing frames, does **not** reduce the bytes sent to the terminal, and does **not** help with the progressive slowdown from growing buffer sizes. It is necessary but not sufficient: every serious inline renderer uses it, but none relies on it as the sole optimization.

### The streaming content problem

Agent-style applications have a unique challenge: content streams in token-by-token and must be displayed incrementally. The Codex team's solution — display-time-only wrapping with newline-boundary commits — is the most robust approach discovered. Store logical lines, wrap only during rendering at the current viewport width. Never commit pre-wrapped visual lines. This ensures correct reflow on resize without retroactively modifying committed content.

### Practical synthesis for a Rust declarative renderer

For building a declarative rendering model on top of Ratatui for inline agent-style TUIs, the evidence points to this architecture:

1. **Own the viewport** rather than cooperating with Ratatui's inline mode. Maintain an in-memory model of all content and render the visible slice each frame. Use `insert_before()` or raw terminal writes only for committing finalized content to scrollback.
2. **Implement a component tree with dirty tracking**. Each component caches its rendered output and declares when it needs re-rendering. This avoids both full-frame rebuilds and full-buffer diffs.
3. **Diff at the cell level but only within the viewport**. Don't diff scrollback content. The viewport is bounded by terminal dimensions, so the diff cost stays constant regardless of conversation length.
4. **Wrap synchronized output around every frame**. `\x1b[?2026h` at frame start, `\x1b[?2026l` at frame end. Don't bother with feature detection — just send the sequences.
5. **Implement display-time-only wrapping** for streaming content. Buffer logical lines, wrap at render time based on current width. Reflow automatically on resize.
6. **Buffer all terminal writes** through a `BufWriter`, track current cursor position and style state to minimize escape sequence overhead, and combine SGR parameters.

The winning pattern across all successful implementations — pi-tui, Codex tui2, and Claude Code's rewrite — is the same: **retained component state, viewport-only rendering, minimal terminal writes, atomic frame display**. The declarative model adds a component tree on top, but the rendering pipeline underneath follows these same principles.