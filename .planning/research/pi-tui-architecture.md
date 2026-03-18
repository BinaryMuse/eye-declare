# pi-tui Rendering Architecture: Deep Technical Reference

**Sources:** Mario Zechner's blog post ([mariozechner.at](https://mariozechner.at/posts/2025-11-30-pi-coding-agent/)), the [badlogic/pi-mono](https://github.com/badlogic/pi-mono) TypeScript source, [steipete/TauTUI](https://github.com/steipete/TauTUI) (Swift port with porting spec, sync logs, and actual source code), and Peter Steinberger's [signature-flicker post](https://steipete.me/posts/2025/signature-flicker).

---

## Overview

pi-tui (`@mariozechner/pi-tui`) is a TypeScript terminal UI library embedded in the `badlogic/pi-mono` monorepo at `packages/tui/`. It is described by Peter Steinberger as the "gold standard for differential rendering" among coding agent TUIs. It is not full-screen/alt-screen — it renders inline, preserving native terminal scrollback. The Swift port TauTUI (`steipete/TauTUI`) is a direct feature-parity reimplementation that documents every architectural decision explicitly.

The library's distinguishing characteristics:

- **Inline rendering only** (no alternate screen)
- **Retained-mode component tree** (not immediate-mode like Ratatui)
- **Line-string backbuffer** compared against previous render to compute diffs
- **Three rendering strategies** (first render, full clear, partial delta)
- **DEC 2026 synchronized output** wrapping every write
- **Native terminal scrollback preservation** as an explicit design goal

---

## 1. Retained-Mode Component Model

### Component Protocol

Every UI element implements a simple interface:

```typescript
// TypeScript (pi-tui)
interface Component {
  render(width: number): string[];
  handleInput?(data: string): void;
  invalidate?(): void;
  wantsKeyRelease?: boolean;
}
```

```swift
// Swift (TauTUI)
public protocol Component: AnyObject {
  func render(width: Int) -> [String]
  func handle(input: TerminalInput)
  func invalidate()
  func apply(theme: ThemePalette)
}
```

Key contract: `render(width)` returns **one string per terminal line**, where each string may contain ANSI escape codes but must not exceed `width` visible columns. The TUI enforces this with a `VisibleWidth.measure()` precondition that will crash/error on overflow.

The TUI appends `\x1b[0m\x1b]8;;\x07` (SGR reset + OSC 8 hyperlink reset) after each line to prevent style bleed across lines. Multi-line styled content must re-emit color codes on each new line.

### Container Composition

`Container` is the simplest `Component` implementation — it holds an ordered list of children and concatenates their `render()` output:

```typescript
class Container implements Component {
  children: Component[];
  render(width: number): string[] {
    return this.children.flatMap(c => c.render(width));
  }
  invalidate(): void {
    this.children.forEach(c => c.invalidate?.());
  }
}
```

The `TUI` class itself subclasses/extends `Container`, so the entire interface is a component tree that produces a flat `string[]` when rendered.

### Per-Component Output Caching

Components are expected to cache their rendered output. A fully-streamed assistant message avoids re-parsing markdown and re-rendering ANSI sequences on every render call — it stores the previously rendered `string[]` and returns it if neither content nor width has changed. The `invalidate()` method clears this cache, forcing fresh computation on the next `render()` call. Theme changes propagate via `apply(theme:)` and call `invalidate()` on children.

The caching is purely component-level — there is no framework-managed dirty flag mechanism. Each component is responsible for its own freshness tracking.

---

## 2. Differential Rendering Algorithm

### The Three Rendering Strategies

The `performRender()` / `doRender()` method evaluates three cases in priority order on every render cycle:

#### Strategy 1: First Render (no previous state)
```swift
if self.previousLines.isEmpty {
    self.writeFullRender(newLines)
    self.previousLines = newLines
    self.previousWidth = width
    self.cursorRow = newLines.count - 1
    return
}
```
On the very first render, emit all lines to the terminal without clearing scrollback. The cursor ends at the last line of the rendered output.

#### Strategy 2: Full Clear + Redraw (width change OR change above viewport)
```swift
if self.previousWidth != width {
    self.writeFullRender(newLines, clear: true)
    // ... update state
    return
}

guard let diffRange = computeDiffRange(old: previousLines, new: newLines) else { return }
let viewportTop = self.cursorRow - height + 1
if diffRange.lowerBound < viewportTop {
    self.writeFullRender(newLines, clear: true)
    // ... update state
    return
}
```
Two separate triggers cause a full clear-and-redraw:
- **Width change**: Because line wrapping is computed against the terminal width, any resize invalidates every line's layout. There is no partial fix — full redraw is mandatory.
- **Change above viewport**: If the first differing line is scrolled above the visible viewport, the terminal cannot scroll back to modify it. Full clear is the only option.

The full clear emits `\x1b[2J\x1b[H\x1b[3J` (clear screen + home cursor + clear scrollback), then outputs all lines fresh.

#### Strategy 3: Partial Delta (the normal case)
```swift
self.writePartialRender(lines: newLines, from: diffRange.lowerBound)
```
When changes are within the visible viewport, only the changed tail is redrawn.

### The `computeDiffRange` Function

This is the core of the differential algorithm. It finds the first and last differing lines between the previous and current render:

```swift
private func computeDiffRange(old: [String], new: [String]) -> Range<Int>? {
    let maxCount = max(old.count, new.count)
    var firstChanged: Int?
    var lastChanged: Int?

    for index in 0..<maxCount {
        let oldLine = index < old.count ? old[index] : ""
        let newLine = index < new.count ? new[index] : ""
        if oldLine != newLine {
            if firstChanged == nil { firstChanged = index }
            lastChanged = index
        }
    }

    guard let start = firstChanged, let end = lastChanged else {
        return nil  // no changes → skip render entirely
    }
    return start..<(end + 1)
}
```

Important properties:
- Returns `nil` if there are no changes at all — render is skipped entirely.
- Lines beyond `old.count` or `new.count` are treated as empty strings `""` for comparison. This means adding a new line at the end produces a diff from `new.count - 1` to `new.count - 1`.
- The range is from `firstChanged` (inclusive) to `lastChanged + 1` (exclusive). But `writePartialRender` always rewrites **from `firstChanged` to the end of `newLines`** — not just the changed range. This means adding one line at the bottom rewrites only that one line. Modifying a line in the middle rewrites from that line to the end.

### The `writePartialRender` Function

```swift
private func writePartialRender(lines: [String], from start: Int) {
    var buffer = ANSI.syncStart  // \x1b[?2026h

    // Move cursor from current position (cursorRow) to target line (start)
    let lineDiff = start - self.cursorRow
    if lineDiff > 0 {
        buffer += ANSI.cursorDown(lineDiff)   // \x1b[{n}B
    } else if lineDiff < 0 {
        buffer += ANSI.cursorUp(-lineDiff)    // \x1b[{n}A
    }
    buffer += ANSI.carriageReturn             // \r (move to column 0)

    // Rewrite from start to end of new content
    for index in start..<lines.count {
        if index > start { buffer += "\r\n" }
        buffer += ANSI.clearLine              // \x1b[2K (erase entire line)
        buffer += lines[index]
    }

    // Clear any extra lines from previous render that are now gone
    if self.previousLines.count > lines.count {
        let extraLines = self.previousLines.count - lines.count
        for _ in 0..<extraLines {
            buffer += "\r\n" + ANSI.clearLine
        }
        buffer += ANSI.cursorUp(extraLines)   // move cursor back up
    }

    buffer += ANSI.syncEnd                    // \x1b[?2026l
    self.terminal.write(buffer)
}
```

Key mechanics:
- **Cursor is always at `cursorRow`** at the start of a render (maintained as invariant). `cursorRow` is updated to `newLines.count - 1` after each render.
- Cursor movement is relative: `\x1b[{n}A` (up) or `\x1b[{n}B` (down) from the current position, followed by `\r` to return to column 0.
- Each line is cleared with `\x1b[2K` before being rewritten. This handles the case where a new line is shorter than the previous one — no stale characters are left behind.
- **Trailing lines cleanup**: If the component tree shrank (fewer lines than last render), extra lines are erased with `\r\n` + `\x1b[2K`, then the cursor moves back up.
- The entire buffer is assembled as a single string and written in one `terminal.write()` call.

### Comparison Semantics

Line comparison is **exact string equality** on the raw ANSI-containing strings. There is no ANSI-stripping for comparison. This means:
- A line that produces the same visible text via different ANSI sequences (e.g., different reset sequences) will be treated as changed.
- Component caching is important to ensure identical output produces identical strings, not just identical visible text.

---

## 3. Streaming Content (Token-by-Token LLM Output)

pi-tui does not have a special streaming API — streaming is handled by the event-driven architecture of the coding agent:

1. The LLM token stream emits delta events.
2. Each delta calls a method on the relevant component (e.g., appending to a `Markdown` or `Text` component's internal buffer).
3. The component calls `tui.requestRender()` or the TUI polls on a schedule.
4. On the next render, `render(width)` is called on all components. The streaming component returns its updated lines.
5. `computeDiffRange` detects that only the last N lines changed (the newly appended tokens).
6. `writePartialRender` rewrites only from the first changed line to the end.

Because token streaming typically appends to the end of content, the diff range is typically `[lastLine, lastLine]` — only the most recently modified line and any newly added lines are rewritten. For a chat interface receiving tokens at 50-100 tokens/second, this means only 1-3 lines are rewritten per render cycle.

**Component-level caching interaction**: When a streaming component receives a new token, it calls `invalidate()` on itself to clear its cached render output. On the next frame, it re-renders from scratch (re-parsing markdown etc.). For completed messages, `invalidate()` is not called, so they return their cached `string[]` instantly, and `computeDiffRange` produces no diff for those lines.

**Render scheduling**: `requestRender()` coalesces via `process.nextTick` (TypeScript) / `DispatchQueue.main.async` (Swift). Multiple token arrivals within a single event loop tick are batched into one render cycle.

---

## 4. Terminal Resize Handling

Width changes trigger the full-clear strategy without exception. The rationale is fundamental: soft word wrapping in terminal content is computed at render time against the known width. When width changes, every line's content potentially changes its layout. There is no partial workaround.

The resize sequence:
1. `SIGWINCH` signal (Unix) or a resize detection mechanism fires.
2. `onResize` callback calls `requestRender()`.
3. On next render cycle, `terminal.columns` returns the new width.
4. `performRender()` detects `previousWidth != width`.
5. `writeFullRender(newLines, clear: true)` is called:
   - Emits `\x1b[?2026h` (sync start)
   - Emits `\x1b[2J\x1b[H\x1b[3J` (clear screen + home + clear scrollback)
   - Emits all lines joined with `\r\n`
   - Emits `\x1b[?2026l` (sync end)
6. All state (`previousLines`, `previousWidth`, `cursorRow`) is reset.

**The destructive side effect**: Clear scrollback (`\x1b[3J`) is emitted on every resize. Any content the user had scrolled up to see in scrollback is erased. This is an unavoidable consequence of the full-clear strategy — the renderer cannot rewrite a subset of scrollback to reflow it, so it wipes and restarts.

**Height changes** (terminal height change without width change): The TUI recalculates `viewportTop = cursorRow - height + 1`. If previous content now falls above the new, smaller viewport, the next render that touches those lines will trigger a full clear. Height-only changes without any content change may proceed without redraw.

---

## 5. Scrollback Commitment Policy

### How Content Becomes "Permanent"

pi-tui does not have an explicit "commit to scrollback" API like Ratatui's `insert_before()`. Instead, scrollback commitment is implicit and continuous:

1. On the very first render, all lines are output to the terminal with no clearing. Lines that scroll off the top of the viewport as new content is added naturally become part of the terminal's native scrollback buffer.
2. There is no mechanism to mark a component as "done" and push it into immutable scrollback. All rendered components remain in `previousLines` and are potentially redrawn on full-clear events.
3. The "backbuffer" (`previousLines`) grows monotonically over a session. After a multi-hour coding session, `previousLines` might contain thousands of lines representing the entire conversation history.

### The Scrollback Immutability Constraint

This is the central architectural tension:

> "If the first changed line is above the visible viewport (the user scrolled up), we have to do a full clear and re-render."

Terminals do not provide any API to write to the scrollback buffer. Once content scrolls above the visible viewport, it is owned by the terminal emulator and is read-only from the application's perspective. The application can only:

1. **Clear scrollback** (`\x1b[3J`) — destructive, loses everything
2. **Do nothing** — accept that scrollback content is stale if the component produced different output

pi-tui's strategy: if a change occurs above the viewport, issue a full clear and redraw everything from scratch. The consequence is that the user's scroll position is reset to the bottom of the output. This happens rarely in practice because: (a) completed components cache their output and never change, so they produce no diffs, and (b) the agent is typically running and the user is watching the bottom.

### Memory Characteristics of `previousLines`

`previousLines` is a `string[]` containing every line that has ever been rendered, including all ANSI escape codes. For a typical coding agent session:

- Each line is ~80-200 bytes (content + ANSI codes)
- 1000 lines of session history ≈ 100-200 KB
- 10,000 lines ≈ 1-2 MB

From the blog post: *"on computers younger than 25 years, this is not a big deal, both in terms of performance and memory use (a few hundred kilobytes for very large sessions)"*

The string comparison during `computeDiffRange` iterates the entire `previousLines` array on every render. For a 10,000-line session at 60 fps, this is 600,000 string comparisons per second. Most comparisons are cache hits (completed components return identical strings), so modern JS engines optimize this well via string interning.

The TypeScript version also tracks `maxLinesRendered` as a high-water mark and `hardwareCursorRow` to track the actual terminal cursor position (which can diverge from `cursorRow` due to overlays and IME cursor placement).

---

## 6. DEC 2026 Synchronized Output Integration

### The Protocol

Synchronized output (DEC private mode 2026) tells the terminal to buffer rendering until the end marker is received, then display the entire update atomically:

```
\x1b[?2026h    CSI ?2026h  — Begin synchronized output (start buffering)
[content]
\x1b[?2026l    CSI ?2026l  — End synchronized output (flush buffer)
```

### Integration with the Render Loop

Every write to the terminal — both `writeFullRender` and `writePartialRender` — is wrapped in sync start/end. The entire assembled buffer (cursor movements + line clears + content) is sent in a single `terminal.write()` call between the sync markers.

```swift
private func writeFullRender(_ lines: [String], clear: Bool = false) {
    var buffer = ANSI.syncStart        // \x1b[?2026h
    if clear {
        buffer += ANSI.clearScrollbackAndScreen  // \x1b[2J\x1b[H\x1b[3J
    }
    buffer += lines.joined(separator: "\r\n")
    buffer += ANSI.syncEnd             // \x1b[?2026l
    self.terminal.write(buffer)
}
```

The sync markers are not negotiated or capability-checked — they are always emitted. Terminals that do not support DEC 2026 ignore the sequences without harm; terminals that do support it (Ghostty, iTerm2, most modern emulators) buffer and display atomically. VS Code's terminal has partial support and may still flicker under certain timing conditions.

### Interaction with Streaming

For token streaming at high frequency, each token arrival triggers a `requestRender()`. The coalescing mechanism (`process.nextTick` / `DispatchQueue.main.async`) ensures only one sync-wrapped write per event loop tick, regardless of how many tokens arrived in that tick. This is important — if every token triggered an immediate `terminal.write()`, the sync markers would bracket single-token updates and provide little benefit. Coalescing means each sync block contains a meaningful multi-token diff.

---

## 7. Backbuffer Memory Architecture

### State Variables

The TUI class maintains this minimal state:

```typescript
// TypeScript (pi-tui tui.ts)
previousLines: string[]      // entire rendered output from last frame
previousWidth: number        // terminal width at last render
previousHeight: number       // terminal height at last render
maxLinesRendered: number     // high-water mark of total lines ever rendered
hardwareCursorRow: number    // actual terminal cursor position (for IME)
cursorRow: number            // logical end-of-content row
```

```swift
// Swift (TauTUI)
private var previousLines: [String] = []
private var previousWidth: Int = 0
private var cursorRow: Int = 0
private var renderRequested = false
```

### No Cell-Based Buffer

This is a critical distinction from Ratatui's architecture. pi-tui does not maintain a 2D cell grid. The backbuffer is a 1D array of strings (one per line). This has consequences:

- **Pro**: No cell allocation overhead. Memory is proportional to actual rendered content, not terminal area × history depth.
- **Pro**: Streaming appends to `previousLines` as they appear; no frame-boundary synchronization needed.
- **Con**: Cannot do sub-line diffing. If one character changes in the middle of a line, the entire line is rewritten (with `\x1b[2K` + content).
- **Con**: ANSI-aware column width must be computed separately (`VisibleWidth` utility) for overflow checks; it is not embedded in the buffer.

### No Scrollback Buffer Duplication

`previousLines` is the only copy of rendered state. There is no separate "scrollback buffer" in the application. The terminal emulator's scrollback is the authoritative source for content above the viewport. The application only knows about `previousLines[0..cursorRow]`.

---

## 8. Handling the "Scrollback Above Viewport Cannot Be Modified" Limitation

This is the central unsolved problem in all inline TUI renderers. pi-tui's approach:

### The Strategy: Accept the Limitation with Graceful Fallback

1. **Avoid changes to completed content**: Completed components (fully-streamed messages, finished tool calls) return cached output. Their lines never change. `computeDiffRange` produces no diff for them. They never trigger the above-viewport full-clear path.

2. **Detect the violation**: On every render, `viewportTop = cursorRow - terminalHeight + 1` is computed. If `diffRange.lowerBound < viewportTop`, the changed line is above the viewport. Fall back to full clear.

3. **Full clear as fallback**: Issue `\x1b[2J\x1b[H\x1b[3J` and redraw everything. This resets the user's scroll position to the bottom. In practice, this should only happen if the user has scrolled up *and* an in-progress component (currently visible above the viewport) has been updated.

### Why This Approach Works for Coding Agents

The coding agent workload has favorable characteristics:
- **Completed content is truly immutable**: A finished assistant message or tool result never changes. Component caching makes this free.
- **Active content is at the bottom**: The LLM is streaming tokens into the current message, which is at the bottom of the visible output. The diff is always in the visible region.
- **User scrolling is infrequent during active generation**: Users typically watch the bottom while the agent is running.

### The Alternative (Not Chosen)

The alternative would be to maintain a viewport of fixed height and commit completed content to scrollback via `insert_before()`-style scrolling. This is what Ratatui's `insert_before()` API does. pi-tui explicitly rejects this approach because:
- It requires knowing line heights upfront (for the `height` parameter)
- It loses the ability to update in-progress content once committed
- It requires managing the split between "committed scrollback" and "live viewport"

---

## 9. Overlay System

The TUI supports floating overlays rendered on top of the base component tree. Overlays are positioned via:
- **Anchor-based**: nine positions (center, corners, edges)
- **Percentage-based**: `"50%"` syntax for relative placement
- **Absolute coordinates**

Overlay compositing works at the line-string level: after both base and overlay lines are rendered, overlay content is spliced into base lines at the computed column position using a `compositeLineAt()` function that is ANSI-aware (it must account for escape sequences when counting columns). The overlay does not interfere with the differential rendering algorithm — composite lines are compared against their composite previous-frame equivalents in `previousLines`.

---

## 10. IME / Hardware Cursor Positioning

For input components (Editor, Input), the framework needs to position the terminal's hardware cursor at the text insertion point so that IME (Input Method Editor) candidate windows appear in the right place.

The mechanism:
1. Focusable components embed a special marker in their rendered output: `\x1b_pi:c\x07` (an APC escape sequence used as an application-private marker).
2. After all lines are rendered, the TUI scans for this marker.
3. The marker's line/column position is calculated using `VisibleWidth` (ANSI-aware column counter).
4. The marker is stripped from the output before it reaches the terminal.
5. The hardware cursor is repositioned to the calculated location with cursor movement sequences.

This is separate from `cursorRow` (which tracks the end of all content) — the hardware cursor may be placed in the middle of the content area for input components.

---

## 11. Built-In Components

| Component | Description |
|-----------|-------------|
| `Container` | Groups children; flattens render output |
| `Box` | Applies padding and background color to child |
| `Text` | Multi-line word-wrapped text; caches render |
| `TruncatedText` | Single line with ellipsis; pads to full width |
| `Markdown` | Renders markdown (headings, lists, tables, code fences, links, bold/italic) via AST |
| `Input` | Single-line text input with horizontal scroll; Ctrl+A/E/W, word navigation |
| `Editor` | Multi-line input with autocomplete overlay, bracketed paste markers, large paste substitution |
| `Loader` / `CancellableLoader` | Animated spinner at 80ms tick |
| `SelectList` | Scrollable selection list with keyboard navigation |
| `SettingsList` | Configuration UI |
| `Spacer` | Fixed-height blank space |
| `Image` | Inline image via Kitty protocol or iTerm2 protocol |

---

## 12. Terminal Image Support

As of v0.29.0 (2025-12-25), pi-tui added inline image rendering. This required a special accommodation in the rendering engine:

- Image lines contain binary/escape data that is not width-measurable by `VisibleWidth.measure()`.
- The width precondition (`precondition(VisibleWidth.measure(line) <= terminal.columns)`) is skipped for lines containing image escape sequences.
- The TUI queries terminal cell pixel size via `CSI 16t` → response `CSI 6;height;widtht` to determine image dimensions in cells.

---

## 13. Architectural Tradeoffs vs. Alternatives

| Approach | Scrollback | Flicker | Resize | Complexity |
|----------|-----------|---------|--------|------------|
| **pi-tui (inline differential)** | Native terminal | Near-zero (DEC 2026) | Full clear | Low |
| **Ratatui inline (insert_before)** | Native terminal | Moderate | Fixed viewport | Medium |
| **Alt-screen (Amp, OpenCode)** | None | Low | Full redraw | Medium |
| **Ratatui full-screen** | None | Low (double-buffer diff) | Full redraw | Low-Medium |
| **Ink (erase-and-rewrite)** | Native terminal | Visible | Full clear | Low |

pi-tui's design philosophy is: *"you can kill flicker without giving up the terminal's muscle memory"* (Steinberger). The cost is:
- Full clears on resize (and scrollback erasure on resize)
- Full clears when user scrolls up during active generation
- O(n) string comparison per render frame where n = total lines ever rendered

---

## 14. Debug Tooling

The TypeScript implementation supports:
- `PI_DEBUG_REDRAW` env variable for debugging render cycles
- `PI_TUI_DEBUG` env variable for additional debug output
- Width overflow crashes with detailed error logging (annotated as "CRITICAL" in source)

TauTUI adds:
- `VirtualTerminal` test harness: records viewport + scrollback buffers, exposes `sendInput`, `resize`, `flush`, `viewportLines` for fully deterministic test scenarios
- `TTYSampler` and `TTYReplayer` utilities for recording and replaying terminal sessions to compare Swift vs. TypeScript rendering output

---

## 15. Key Implementation Files

**pi-tui (TypeScript, `badlogic/pi-mono`):**
- `packages/tui/src/tui.ts` — Core TUI class, Container, Component interface, `doRender()`/differential algorithm, overlay system, IME cursor (1212 lines)
- `packages/tui/src/terminal.ts` — `ProcessTerminal`, raw mode, Kitty keyboard protocol, stdin buffer
- `packages/tui/src/components/` — Built-in components
- `packages/tui/src/utils.ts` — `wrapTextWithAnsi`, `VisibleWidth`, ANSI helpers

**TauTUI (Swift, `steipete/TauTUI`):**
- `Sources/TauTUI/Core/TUI.swift` — Swift port of TUI runtime, `computeDiffRange`, `performRender`, `writeFullRender`, `writePartialRender`
- `Sources/TauTUI/Core/Component.swift` — `Component` protocol, `Container` class
- `Sources/TauTUI/Terminal/Terminal.swift` — Terminal protocol, `ProcessTerminal`, ANSI constants
- `Sources/TauTUI/Terminal/VirtualTerminal.swift` — Test harness
- `docs/spec.md` — Full porting specification (authoritative architecture doc)
- `docs/pitui-sync.md` — Sync logs from 2025-11-15 through 2025-12-27 tracking upstream changes

---

## 16. Summary of Direct Answers to Research Questions

**Q1: Retained-mode component model / caching**
Components are persistent objects with `render(width) -> string[]`. Each component caches its output internally and returns the same `string[]` until `invalidate()` is called. The TUI never manages component lifecycles — components are added/removed explicitly.

**Q2: Differential algorithm in detail**
`computeDiffRange` does a linear scan to find `firstChanged` and `lastChanged` line indices (treating absent lines as `""`). `writePartialRender` moves the cursor to `firstChanged`, then rewrites every line from there to the end (`lines.count - 1`), clearing each line with `\x1b[2K` before writing. Removed lines are cleared and cursor repositioned.

**Q3: Streaming content**
No special streaming API. Components accumulate tokens, call `invalidate()`, trigger `requestRender()`. The diff is typically the last few lines only. Render coalescing batches multiple tokens per frame.

**Q4: Terminal resize**
Width change → always full clear + redraw (scrollback erased). No partial resize strategy exists or is possible given line-wrapping semantics.

**Q5: Scrollback commitment policy**
Implicit. Content scrolls into scrollback as new output pushes it up. No explicit "commit" API. Completed components cache output so they are never modified in scrollback. Full clear as fallback if scrollback content needs to change.

**Q6: DEC 2026 synchronized output**
Always wrapped around every write. Both full and partial renders use `\x1b[?2026h` ... `\x1b[?2026l`. No capability negotiation — unsupported terminals silently ignore.

**Q7: Backbuffer memory**
A `string[]` of all rendered lines ever. ~100-200 KB for a 1000-line session. No cell grid. String equality comparison per line on every render.

**Q8: Scrollback immutability**
Detected by comparing `diffRange.lowerBound` against `viewportTop = cursorRow - terminalHeight + 1`. If change is above viewport, full clear is issued. Design philosophy relies on completed content being truly immutable (via caching) to make this case rare.
