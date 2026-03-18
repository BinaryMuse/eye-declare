# OpenAI Codex TUI2 Architecture: Viewport, History, and Streaming

> **Branch**: `joshka/docs-tui2-viewport-history`
> **Crate**: `codex-rs/tui2`
> **Research date**: 2026-03-18
> **Primary sources**: Three design docs fetched from `raw.githubusercontent.com`; source files from the branch; GitHub issue #7601 (WIP PR description).

---

## Research Summary

The `tui2` crate is a ground-up redesign of the Codex terminal UI whose central insight is that **the app, not the terminal, must own the viewport**. Rather than cooperating with each terminal emulator's idiosyncratic scrollback behavior, TUI2 keeps an in-memory list of typed `HistoryCell` objects as the single source of truth, renders them to a dedicated screen region on every frame, and only ever appends to terminal scrollback as a one-way export (on suspend or exit). This eliminates the duplicate/missing-content bugs of the legacy TUI and enables deterministic, testable, terminal-agnostic rendering.

---

## Key Findings

1. **`HistoryCell` is the universal unit of conversation content.** Every logical event — user prompt, agent message, tool call, reasoning summary, session header — is a distinct `Arc<dyn HistoryCell>`. The trait surface is small but carefully layered: `display_lines` for on-screen rendering; `transcript_lines` / `transcript_lines_with_joiners` for scrollback export and clipboard copy; `desired_height` / `desired_transcript_height` for layout; `is_stream_continuation` for streaming merging.

2. **Viewport rendering is a flattening + caching pipeline.** On every frame, `render_transcript_cells` asks a `TranscriptViewCache` to flatten all cells into wrapped visual lines at the current terminal width. A scroll anchor (`TranscriptScroll`) maps to an offset into that flat list, and only the visible slice is drawn. The cache has two layers: per-width wrapped lines (invalidated on resize or transcript mutation) and pre-rasterized row cells (LRU-evicted).

3. **Scroll state anchors to content, not to pixel rows.** `TranscriptScroll` stores either "follow latest" or a `TranscriptLineMeta` content reference. On every render call, `resolve_top()` maps the anchor to an integer line offset in the current wrapped list, which may have shifted during resize or streaming growth. This eliminates the "jump" effect the legacy TUI suffered.

4. **History printing uses a cell-level high-water mark.** The app tracks how many cells at the front of the transcript have already been printed to scrollback. On suspend or exit, only the _suffix_ beyond that mark is rendered (at the current terminal width) and appended to stdout. The mark is an integer cell count, never a line count, so width changes cannot cause miscounting.

5. **Streaming tokens integrate via continuation cells.** The first chunk of an agent response creates an initial cell; subsequent chunks create "continuation" cells (`is_stream_continuation() → true`). From the transcript/scrollback perspective, each chunk is just another appended entry. The limitation is that streaming cells currently bake in the width at commit time; a follow-up is planned to make streaming use display-time-only wrapping like everything else.

6. **Display-time-only wrapping works through range-based span slicing.** The `wrapping.rs` module's `wrap_ranges_trim` returns byte-range indices into the original styled `Line` without copying text. `slice_line_spans` reconstructs correctly-styled output by mapping those ranges back to the original span boundaries. Source data is never mutated; the same underlying text can be reflowed to any width on every frame.

7. **Suspend/resume is an explicit TUI operation, not a pass-through.** On Ctrl+Z: leave alt screen, print unpublished cells to scrollback, advance high-water mark, then background the process. On `fg`: re-enter TUI modes, redraw from in-memory transcript. The terminal state is managed with an `alt_screen_nesting` counter that supports nested overlays without losing viewport state.

8. **Lessons from the legacy TUI** are explicitly enumerated in `tui_viewport_and_history.md`: terminal-dependent behavior (scroll regions, clears, resize semantics differ across emulators); layout churn on resize/focus-change causing duplicate or lost lines; "clear and rewrite everything" strategies still failing because terminals treat full-screen clears differently (Terminal.app leaves a blank page, iTerm2 requires user consent for scrollback clear); and line-count–based high-water mark logic that broke whenever the terminal width changed.

---

## Detailed Analysis

### 1. The `HistoryCell` Trait

**File**: `codex-rs/tui2/src/history_cell.rs`

```rust
pub(crate) trait HistoryCell: std::fmt::Debug + Send + Sync + Any {
    // Core rendering — returns styled ratatui Lines at the given viewport width.
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;

    // Layout — defaults to counting lines in a Paragraph with wrapping.
    fn desired_height(&self, width: u16) -> u16 {
        Paragraph::new(Text::from(self.display_lines(width)))
            .wrap(Wrap { trim: false })
            .line_count(width)
            .try_into()
            .unwrap_or(0)
    }

    // Scrollback export — by default same as display_lines.
    // Cells that need different scrollback representation can override.
    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.display_lines(width)
    }

    // Clipboard copy path — returns lines with soft-wrap joiner metadata.
    fn transcript_lines_with_joiners(&self, width: u16) -> TranscriptLinesWithJoiners {
        let lines = self.transcript_lines(width);
        TranscriptLinesWithJoiners {
            joiner_before: vec![None; lines.len()],
            lines,
        }
    }

    // Layout for the transcript export context (slightly different whitespace logic).
    fn desired_transcript_height(&self, width: u16) -> u16 { ... }

    // True for streaming continuation chunks — used by the transcript builder
    // to suppress the spacer row that normally separates cells.
    fn is_stream_continuation(&self) -> bool { false }
}
```

**Method responsibilities in detail:**

| Method | Purpose |
|---|---|
| `display_lines(width)` | On-screen rendering. Returns `Vec<Line<'static>>` at `width`. Called every frame from the render pipeline. |
| `desired_height(width)` | Layout hint used by the viewport to compute how many rows a cell occupies. Defaults to counting wrapped lines; cells with fixed sizes can override. |
| `transcript_lines(width)` | Scrollback export and transcript overlay rendering. Usually identical to `display_lines`, but cells may strip interactive decorations. |
| `transcript_lines_with_joiners(width)` | Clipboard copy path. Returns both the styled lines _and_ a parallel `joiner_before` vector indicating, for each continuation line, what whitespace was skipped at the wrap boundary (enabling prose joins on copy). |
| `desired_transcript_height(width)` | Same as `desired_height` but applies a special rule: an all-whitespace single line counts as height 1 (prevents empty spacer cells from inflating height counts). |
| `is_stream_continuation()` | Returns `true` for cells that are continuation chunks of the same streaming message. The transcript builder suppresses the inter-cell spacer row for continuations, keeping a streamed response visually unified. |

**Known concrete implementations:**

- `UserHistoryCell` — user prompt text with `"› "` prefix
- `AgentMessageCell` — agent response text with indentation and code-block detection
- `ReasoningSummaryCell` — dimmed italic reasoning content
- `McpToolCallCell` — MCP tool invocations with spinner/checkmark/cross status, result truncation, and image support
- `PrefixedWrappedHistoryCell` — generic wrapping with configurable initial and subsequent indents
- `SessionInfoCell` — session header, help text, and tooltips

**`TranscriptLinesWithJoiners` invariants:**

```rust
pub(crate) struct TranscriptLinesWithJoiners {
    pub(crate) lines: Vec<Line<'static>>,
    // len == lines.len()
    // joiner_before[0] is always None (first line has no predecessor)
    // None  => hard line break (between input lines)
    // Some(s) => soft-wrap continuation; s is the skipped whitespace
    pub(crate) joiner_before: Vec<Option<String>>,
}
```

When copying, the clipboard reconstruction code uses `None` to insert a real newline, and `Some(joiner)` to join with the skipped whitespace (or to drop it for prose joins), preserving code-block indentation while collapsing soft-wrapped prose back to a single logical line.

---

### 2. Viewport Rendering Pipeline

**Files**: `codex-rs/tui2/src/app.rs`, `src/transcript_render.rs`, `src/transcript_view_cache.rs`, `src/wrapping.rs`

The pipeline has five stages:

#### Stage 1 — Define the transcript region

```
transcript_area = full frame area minus chat_height rows at the bottom
```

If either dimension is zero, scroll state is reset and rendering is skipped.

#### Stage 2 — Ensure the cache is up to date (`TranscriptViewCache::ensure_wrapped`)

`TranscriptViewCache` holds two sub-caches:

- **`WrappedTranscriptCache`** — memoizes the full flattened wrapped transcript for a given `(cells, width)` pair. Invalidation strategy:
  - If `width` changed → full rebuild
  - If the transcript pointer identity changed or length decreased → full rebuild (detects cell replacement / backtrack truncation)
  - If cells were _appended_ at the same width → incremental append using `append_wrapped_transcript_cell`
  - Otherwise → no-op

- **`TranscriptRasterCache`** — caches pre-rasterized ratatui `Cell` buffers per `(line_index, is_user_row)`. Uses approximate LRU eviction with a monotonic clock stamp and a deque-based access log.

Raster cache is keyed as `(line_index as u64) << 1 | u64::from(is_user_row)` — a single 64-bit integer packed from the two discriminants.

#### Stage 3 — Resolve scroll position to a top-line offset

```rust
let (scroll_state, top_offset) = {
    let line_meta = self.transcript_view_cache.line_meta();
    self.transcript_scroll.resolve_top(line_meta, max_start)
};
self.transcript_scroll = scroll_state;
self.transcript_view_top = top_offset;
```

`resolve_top` maps the `TranscriptScroll` anchor (either "follow latest" or a `TranscriptLineMeta` content reference) to an integer `top_offset` into the wrapped line list. If the scroll mode is "follow latest", `top_offset = max_start` (always shows the bottom). If it is anchored, the metadata is looked up in the cache's `line_meta()` table to find the new offset (which may have shifted due to wrapping changes).

#### Stage 4 — Draw the visible slice

The visible slice is `lines[top_offset .. top_offset + max_visible]`. Each row is drawn via `TranscriptViewCache::render_row_index_into`, which either copies pre-cached rasterized cells or rasterizes the line into the destination buffer (then caches it). User-message rows get full-width background painting.

#### Stage 5 — Apply overlays

After the base transcript is painted, `apply_transcript_selection` overlays selection highlighting on top. Selection coordinates are stored in transcript-space (`TranscriptSelection`), so they remain stable across scrolling.

**`build_transcript_lines` (the flattening step, in `transcript_render.rs`):**

```
For each cell in transcript_cells:
  1. Call cell.transcript_lines_with_joiners(width)
  2. If cell is NOT a stream_continuation, insert a spacer row before it
     (spacer has a TranscriptLineMeta pointing to the cell boundary)
  3. Append the cell's visual lines with their metadata and joiners
```

The result is three parallel vectors: `lines`, `meta` (one `TranscriptLineMeta` per visual line), and `joiner_before`.

**`TranscriptLineMeta`** records which cell index and which line within that cell each visual line came from. This is the anchor structure used by `TranscriptScroll::resolve_top`.

---

### 3. Scroll State

**File**: `codex-rs/tui2/src/app.rs` (inline scroll fields), `transcript_scroll.rs` (not publicly found, but behavior is documented)

The `App` struct tracks:

```rust
transcript_scroll: TranscriptScroll,    // content-anchored position
transcript_view_top: usize,             // resolved integer offset (per-frame)
transcript_total_lines: usize,          // total wrapped lines (per-frame)
scroll_config: ScrollConfig,            // lines-per-tick, acceleration settings
scroll_state: MouseScrollState,         // stream-based event accumulator
```

**`TranscriptScroll` modes:**

- **Follow Latest** (default): `top_offset` always resolves to `max_start` (the bottom of the transcript). New content causes the view to advance automatically.
- **Anchored at `TranscriptLineMeta`**: records a cell index + intra-cell line number. On each frame, `resolve_top` searches the current line metadata table to find the current integer offset for that anchor. Resize or streaming growth shifts line numbers but the anchor survives because it references _content_, not a raw row number.
- **Spacer row anchor**: a special anchor variant that points to the spacer before a cell, preventing boundary-stickiness when a cell grows during streaming.

**User scroll → scrolled away from bottom:**

```
ScrollDirection event
  → mouse_scroll_update() → ScrollUpdate { lines, next_tick_in }
  → apply_scroll_update()
     → scroll_transcript(delta, visible_lines, width)
        → if currently following: switch to Anchored at current top_offset
        → offset += delta (clamped to [0, max_start])
        → if offset == max_start: switch back to Follow Latest
  → if next_tick_in: schedule deferred tick for velocity/acceleration
```

A `ScrollUpdate` with `next_tick_in` starts a scroll tick timer for kinetic scrolling. On `handle_scroll_tick`, `scroll_state.on_tick()` produces the next velocity-decayed `ScrollUpdate`.

**New content arrives while scrolled:**

Because scroll state is anchored to a `TranscriptLineMeta` (not an integer), when new cells are appended the cache is extended but the anchor retains its meaning. On the next frame, `resolve_top` finds the same logical line at its new offset. The viewport _stays_ where the user left it; the new content appears below the visible area. This is the "follow latest stops while selection is active" behavior — selection locking is implemented as the same mechanism.

**Keyboard scrolling (PgUp/PgDn/Home/End):**

These use `scroll_transcript` directly with fixed delta values (PgUp/PgDn use `visible_lines` as the delta; Home sets `top_offset = 0`; End sets `top_offset = max_start` and restores Follow Latest).

---

### 4. High-Water Mark History Printing

**Design doc**: `tui_viewport_and_history.md` §5

The high-water mark is a simple integer: **"how many cells at the front of the transcript have already been sent to scrollback."** It tracks logical cell count, not wrapped line count.

**Why cell-level, not line-level?**

Line counts change whenever the terminal is resized (wrapping changes) or when streaming adds content. A line-based mark would need to recompute "how many of the old lines match the new layout" — which is exactly the bug the legacy TUI had. A cell-level mark is unambiguous: cells are never reordered or modified in place; they are only appended.

**Print-to-scrollback flow (suspend or exit):**

```
1. high_water = current high-water mark (e.g., 5 cells printed so far)
2. suffix = transcript_cells[high_water..]
3. Render suffix using build_transcript_lines(suffix, current_terminal_width)
4. Convert to ANSI strings via render_lines_to_ansi()
5. Write to stdout
6. high_water += suffix.len()
```

`render_lines_to_ansi` merges line-level styles, pads user-authored row backgrounds to terminal width, and emits ANSI escape sequences via a shared vt100 writer.

**Scrollback is append-only.** Previously printed cells remain in scrollback at whatever width they were printed. No attempt is made to reflow them. The design document explicitly accepts this tradeoff: logical correctness (each cell appears exactly once) is prioritized over visual consistency across width changes.

**Configuration:**

A `print_on_suspend` config flag controls whether the high-water mark advances on Ctrl+Z or only on exit. The WIP PR describes `print_on_suspend = true` as the default. The design anticipates a future "streaming cell tail" mode to always include the latest chunk of a currently-streaming cell, with explicit acknowledgment that this would allow a small intentional duplication at the streaming boundary.

---

### 5. Streaming Content Integration

**Files**: `codex-rs/tui2/src/streaming/`, `history_cell.rs`, `tui_viewport_and_history.md` §6

**How streaming tokens become cells:**

The streaming pipeline lives in `streaming/markdown_stream.rs` and `chatwidget.rs`. The model:

1. The first token of an agent response creates an initial `AgentMessageCell` (or equivalent) and appends it to `transcript_cells`.
2. Each subsequent token produces a new "continuation" cell and appends it.
3. Each continuation cell returns `is_stream_continuation() → true`, which suppresses the inter-cell spacer row so the streamed response appears as a single visual block.
4. `TranscriptViewCache::ensure_wrapped` handles continuations via the **append** path (no full rebuild needed), so rendering stays fast during streaming.

**The current limitation — commit-time wrapping:**

The design doc (`tui_viewport_and_history.md` §6.1, `streaming_wrapping_design.md`) candidly describes a known gap:

> "Today, streaming rendering still 'bakes in' some width at the time chunks are committed: line breaks for the streaming path are computed using the width that was active at the time, and stored in the intermediate representation."

This means if the user resizes the terminal while a response is streaming, the streaming cells will retain the old width's line breaks, while non-streaming cells (which go through `display_lines(current_width)` on every frame) reflow correctly. The three proposed remediation paths are:

1. **Document current behavior** (conservative, no change)
2. **Width-agnostic streaming cells** — store raw token text and wrap only at render time (same model as non-streaming cells)
3. **Visual line count model** — re-render streaming cells at the current width each frame

Option 2 is the intended target ("wrap on display" model), deferred while other viewport features stabilize.

**Selection locking during streaming:**

While a `TranscriptSelection` is active, the app does _not_ advance to Follow Latest mode even when new streaming cells arrive. This keeps the selection stable. Follow Latest resumes when the selection is cleared.

---

### 6. Display-Time-Only Wrapping

**File**: `codex-rs/tui2/src/wrapping.rs`

Display-time-only wrapping is the principle that **the source `Line` objects in `HistoryCell::display_lines` are never pre-broken at a fixed width**. Instead, `wrapping.rs` computes wrap points at render time and returns new `Line` objects that reference spans from the originals.

**Implementation mechanism:**

```
word_wrap_lines_borrowed(lines, width_or_options):
  For each input Line:
    1. Compute wrap ranges via wrap_ranges_trim(text, width)
       → returns Vec<(start_byte, end_byte)> — indices into original text
    2. For each range, call slice_line_spans(line, start, end)
       → reconstructs a styled Line by mapping byte ranges back to spans
       → preserves style attributes without copying underlying strings
    3. Tag continuation lines with joiner metadata (the skipped whitespace)
  Return flat Vec<Line> of wrapped output lines
```

Because only byte-range indices are computed (not new string allocations), the wrapping step is cheap enough to run on every frame for the visible slice. The `TranscriptViewCache` wraps the full transcript once per width (memoized), and the per-frame hot path uses the raster cache.

**`RtOptions` (wrapping options):**

The wrapping function accepts an `RtOptions` that encodes:

- `initial_indent`: prefix for the first line (e.g. `"› "` for user prompts)
- `subsequent_indent`: prefix for continuation lines (e.g. `"  "` for visual alignment)
- `width`: maximum column count

This means the indent is applied at _display time_, consistently with the wrapping, rather than being baked into the stored text.

**Soft-wrap joiners (copy semantics):**

`word_wrap_line_with_joiners` returns two parallel vectors:

- The wrapped lines
- `joiner_before`: `None` for hard breaks, `Some(skipped_whitespace)` for soft wraps

During clipboard copy, `None` → newline in output; `Some(joiner)` → join prose lines with the original whitespace (or drop it for natural prose), but _preserve_ the joiner for code blocks to maintain indentation.

---

### 7. Suspend/Resume Lifecycle

**Files**: `codex-rs/tui2/src/tui.rs`, `tui_viewport_and_history.md` §5.3

The `Tui` struct manages terminal modes with:

- `alt_screen_nesting: u32` — reference-counted nesting depth for overlays
- `alt_screen_active: bool` — tracks the current state
- Inline viewport dimensions (saved on alt-screen enter, restored on leave)

**Terminal modes set on startup (`set_modes`):**

- Bracketed paste mode
- Raw mode (individual key events, no line buffering)
- Keyboard enhancement flags: `DISAMBIGUATE_ESCAPE_CODES`, `REPORT_EVENT_TYPES`, `REPORT_ALTERNATE_KEYS` (with graceful fallback on unsupported terminals via `enhanced_keys_supported` flag)
- Mouse capture for scroll event delivery
- Focus change notifications

**Alt screen nesting:**

`enter_alt_screen()` increments `alt_screen_nesting`. Only the first call (nesting = 1 → 2 or 0 → 1) actually executes `EnterAlternateScreen`. `leave_alt_screen()` decrements; only when it reaches 0 does `LeaveAlternateScreen` execute and inline viewport dimensions restore. This allows pager overlays to layer on top of the main alt-screen without accidentally leaving it.

**Suspend flow (Ctrl+Z on Unix):**

```
1. Event loop detects Ctrl+Z
2. suspend_context.suspend(&alt_screen_active):
   a. leave_alt_screen() if active (writes LeaveAlternateScreen)
   b. restore() — disables raw mode, mouse capture, bracketed paste
   c. Print unpublished cells to scrollback (high-water mark advance)
   d. SIGSTOP the process (or raise(SIGTSTP)) → process goes to background
3. Shell runs; user eventually types `fg`
4. Process receives SIGCONT
5. prepared_resume.apply(&mut terminal):
   a. Re-enable raw mode, mouse capture, etc.
   b. Re-enter alt screen if it was active
   c. Synchronized update wraps the re-entry to prevent visual glitches
6. Cursor is repositioned via suspend_context.set_cursor_y(inline_area_bottom)
7. App::render() is called → full redraw from in-memory transcript
```

**On exit:**

`App::run()` returns `AppExitInfo { session_lines, ... }` where `session_lines` contains the ANSI-rendered suffix of the transcript (cells beyond the high-water mark). The CLI calling code:

```
1. tui.terminal.clear()        // clear TUI
2. leave_alt_screen()          // return to normal screen
3. print session_lines         // append unpublished history to scrollback
4. print blank line + token usage summary
```

Result: exit always prints each logical cell exactly once regardless of how many Ctrl+Z suspensions occurred during the session.

---

### 8. Lessons from the Legacy TUI

**Source**: `tui_viewport_and_history.md` §1, §8; `tui2_viewport_history_architecture.md`

The design doc enumerates these failure modes explicitly, with architectural responses:

| Legacy failure | Legacy cause | TUI2 response |
|---|---|---|
| Lost/duplicated lines on different terminals | Relied on scroll regions, partial clears, and re-writes whose ANSI behavior varies across emulators | Never cooperate with scrollback during live rendering; only append to it on explicit export |
| Lines dropped/duplicated on resize | Viewport coordinates shifted when size changed; line-count high-water mark misaligned | Cell-level high-water mark; `TranscriptLineMeta` content anchors survive resize |
| "Clear and rewrite everything" didn't work | Terminal.app leaves cleared screen as a scrollback page; iTerm2 gates `clear scrollback` behind user consent | Alt screen is a temporary render target; in-memory transcript is the only authoritative copy |
| Scrollback out of sync with TUI state | Legacy tried to keep scrollback "aligned" with in-memory history in real time | Scrollback is treated as append-only; the live TUI never reads from it |
| Inconsistent behavior across suspends | Suspend/resume was treated as transparent pass-through; state could drift | Suspend is an explicit TUI operation: leave alt screen, print history, SIGSTOP, on resume full redraw |
| Selection / copy copied gutter/margin characters | Used terminal's native selection which operates on raw buffer rows | Selection is implemented in transcript-coordinate space; gutter excluded by design |
| Streaming reflow on resize | Streaming cells baked in line breaks at commit time | Planned migration to display-time-only wrapping for all cell types |

**Explicit tradeoffs accepted by the new design:**

- Scrollback is append-only and will not reflow when terminal width changes. Older printed cells will remain at whatever wrapping they had when printed.
- The TUI must implement _all_ of its own scrolling, selection, and copy behavior. No delegation to the terminal's native selection.
- Terminal interaction is more complex: explicit alt-screen entry/exit, mouse event delivery, clipboard paste management. Users with terminals that conflict with TUI assumptions (remapped Ctrl+Z, disabled alt screen) will have reduced functionality.
- Suspend and resume are now explicit TUI operations rather than transparent pass-throughs.

**The design justification:**

> "Because cells are always re-rendered live from the transcript, per-cell interactions can become richer over time. Instead of treating the transcript as 'dead text', we can make individual entries interactive after they are rendered: expanding or contracting tool calls, diffs, or reasoning summaries in place, jumping back to a particular point in the conversation, and offering context-sensitive actions."

---

## Module Map

| Module | Role |
|---|---|
| `src/history_cell.rs` | `HistoryCell` trait + all concrete cell implementations (~2000 lines) |
| `src/transcript_render.rs` | `build_transcript_lines`, `render_lines_to_ansi`, `TranscriptLineMeta`, `TranscriptLines` |
| `src/transcript_view_cache.rs` | `TranscriptViewCache`, `WrappedTranscriptCache`, `TranscriptRasterCache` (two-layer cache) |
| `src/wrapping.rs` | `word_wrap_lines_borrowed`, `word_wrap_line_with_joiners`, `wrap_ranges_trim`, `slice_line_spans`, `RtOptions` |
| `src/app.rs` | `App` struct, `render_transcript_cells`, `scroll_transcript`, `handle_mouse_event`, `AppExitInfo`, `run()` |
| `src/tui.rs` | `Tui` struct, `enter_alt_screen`, `leave_alt_screen`, `suspend_context`, terminal modes |
| `src/streaming/markdown_stream.rs` | Streaming token collection, animation timing, commit logic |
| `src/transcript_selection.rs` | `TranscriptSelection` — content-coordinate selection state |
| `src/clipboard_copy.rs` | Copy reconstruction from selection + joiners |
| `src/bottom_pane/footer.rs` | Footer text assembly including scroll position and copy hints |
| `docs/tui2_viewport_history_architecture.md` | Top-level architecture doc (current) |
| `docs/tui_viewport_and_history.md` | Design rationale doc (historical, but very detailed) |
| `docs/streaming_wrapping_design.md` | Streaming wrapping limitation and remediation paths |
| `docs/scroll_input_model.md` | Scroll normalization model and per-terminal defaults |

---

## Research Gaps and Limitations

- `transcript_scroll.rs` returned a 404 — the `TranscriptScroll` struct and `resolve_top` implementation details come from docstrings and the architecture doc rather than direct code inspection.
- `streaming/markdown_stream.rs` also returned 404 — streaming integration details reconstructed from the design docs and the `history_cell.rs` summary.
- The exact `ScrollConfig` and `MouseScrollState` types were not directly observed; behavior described from `app.rs` method names and the scroll_input_model reference.
- The `docs/scroll_input_model.md` document was not fetched; scroll normalization details are partially covered.
- Implementation of `suspend_context` (the `prepared_resume.apply` step) was summarized by the WebFetch tool rather than shown as raw code.

---

## Contradictions and Disputes

- The design docs describe streaming wrapping as a "known limitation" in two places with slightly different framing: `tui_viewport_and_history.md` calls it "a known limitation… a follow-up change will make streaming behavior match the rest of the transcript more closely," while `streaming_wrapping_design.md` describes it as "intentionally deferred while prioritizing viewport functionality." These are consistent in intent but the second framing is more conservative about timeline.
- The WIP PR description (from gitmemories) says `print_on_suspend = true` is the default, while the design doc says "that switch has not been implemented yet." The PR appears to have implemented it after the design doc was written — the design doc is described as "historical" and the architecture doc supersedes it.

---

## Search Methodology

- Searches performed: 3 web searches + 10 WebFetch calls
- Most productive sources: raw.githubusercontent.com direct document fetches; gitmemories.com for PR description
- Primary source domains: github.com/openai/codex (raw branch), gitmemories.com

---

## Sources

- [TUI2 Viewport + History Architecture Doc (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/docs/tui2_viewport_history_architecture.md)
- [TUI Viewport and History Design Notes (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/docs/tui_viewport_and_history.md)
- [Streaming Wrapping Design Doc (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/docs/streaming_wrapping_design.md)
- [history_cell.rs source (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/src/history_cell.rs)
- [app.rs source (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/src/app.rs)
- [transcript_render.rs source (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/src/transcript_render.rs)
- [transcript_view_cache.rs source (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/src/transcript_view_cache.rs)
- [wrapping.rs source (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/src/wrapping.rs)
- [tui.rs source (raw)](https://raw.githubusercontent.com/openai/codex/joshka/docs-tui2-viewport-history/codex-rs/tui2/src/tui.rs)
- [WIP PR: Rework TUI viewport, history printing, and selection/copy](https://gitmemories.com/openai/codex/issues/7601)
- [GitHub issue #8344: Don't mess with the native TUI](https://github.com/openai/codex/issues/8344)
