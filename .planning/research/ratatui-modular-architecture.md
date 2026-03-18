# Ratatui v0.30+ Modular Architecture Research

## Research Summary

Ratatui v0.30.0 (released December 2025) split the monolithic crate into a modular workspace. The key insight for our purposes is: **`ratatui-core` provides `Buffer`, `Cell`, `Rect`, `Layout`, `Style`, and the `Widget`/`StatefulWidget` traits independently from any terminal backend. All existing widgets render into an arbitrary `Buffer` you own, without requiring `Terminal::draw()`.** We can use `ratatui-core` + `ratatui-widgets` with raw crossterm for a fully custom inline rendering pipeline.

---

## Q1: Crate Split — What Each Crate Contains

The modular workspace consists of these crates:

### `ratatui-core` (v0.1.0)
The foundational layer. Widget library authors should depend on this directly.

Modules:
- `backend` — The `Backend` trait (abstraction over terminal libraries)
- `buffer` — `Buffer` and `Cell` types
- `layout` — `Layout`, `Constraint`, `Direction`, `Rect`, `Position`, `Size`
- `style` — `Color`, `Style`, `Modifier`/`TextModifier`, `Stylize`
- `symbols` — Drawing symbols and markers (box-drawing, braille, etc.)
- `terminal` — `Terminal<B>` and `Frame` types
- `text` — `Text`, `Line`, `Span` styled text primitives
- `widgets` — `Widget` and `StatefulWidget` traits only (no implementations)

Notable: `ratatui-core` does NOT contain any built-in widget implementations. It only has the traits.

Feature flags: `std` (default), `layout-cache`, `anstyle`, `palette`, `portable-atomic`, `underline-color`, `scrolling-regions`, `serde`

### `ratatui-widgets`
All 16 built-in widget implementations, extracted from the old monolithic crate. Depends on `ratatui-core ^0.1.0`. Has **no backend dependencies**.

Widgets: `BarChart`, `Block`, `Calendar` (monthly), `Canvas`, `Chart`, `Clear`, `Gauge`, `LineGauge`, `List`, `Paragraph`, `RatatuiLogo`, `RatatuiMascot`, `Scrollbar`, `Sparkline`, `Table`, `Tabs`

### `ratatui-crossterm`
Crossterm backend. Provides:
- `CrosstermBackend<W: Write>` — implements the `Backend` trait using crossterm
- `FromCrossterm` / `IntoCrossterm` — conversion traits between crossterm and ratatui types
- Re-exports the selected crossterm version as `ratatui_crossterm::crossterm`

Supports crossterm v0.28 and v0.29 via feature flags.

### `ratatui-termion` / `ratatui-termwiz`
Termion and Termwiz backends respectively, following the same pattern as `ratatui-crossterm`.

### `ratatui-macros`
Macro utilities (convenience macros for building UI components).

### `ratatui` (main crate)
The "batteries included" façade. Re-exports everything from the above crates. Application developers should use this. Widget library authors should prefer `ratatui-core`.

**Import migration example:**
```rust
// Old (0.29.x and prior) - for widget library authors:
use ratatui::{widgets::{Widget, StatefulWidget}, buffer::Buffer, layout::Rect};

// New (0.30.0+) - preferred for widget library authors:
use ratatui_core::{widgets::{Widget, StatefulWidget}, buffer::Buffer, layout::Rect};
```

---

## Q2: Using ratatui-core WITHOUT Terminal or its Rendering Loop

**Yes, fully supported.** The architecture is explicitly designed for this.

Key design facts:
- `Buffer`, `Cell`, `Rect`, `Layout`, `Style`, `Widget`, `StatefulWidget` are all in `ratatui-core` and have zero dependencies on any backend or terminal library
- The `Widget` trait only needs a `&mut Buffer` — it does not touch a terminal, backend, or `Terminal` struct
- `Terminal<B>` is a convenience wrapper that owns two `Buffer`s and manages diffing/flushing; it is entirely optional
- `ratatui-core` has full `no_std` support, meaning these types are designed to work with no system I/O at all

For a custom inline renderer, you:
1. Create a `Buffer::empty(area)` with a `Rect` matching your desired render area
2. Call `widget.render(area, &mut buffer)` directly
3. Use `Buffer::diff()` against a previous buffer to find changed cells
4. Write those cells to the terminal yourself using raw crossterm

---

## Q3: Widget Trait Signatures

### `Widget` trait (from `ratatui_core::widgets`)

```rust
pub trait Widget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where Self: Sized;
}
```

- `render` takes ownership of `self` (the widget is consumed after rendering)
- `area: Rect` defines where within the buffer the widget should render
- `buf: &mut Buffer` is the target buffer being written to
- This is the ONLY required method

**Reference-based variant (feature-gated, `unstable-widget-ref`):**
```rust
pub trait WidgetRef {
    fn render_ref(&self, area: Rect, buf: &mut Buffer);
}
```
Allows storing widgets and rendering them by reference (for trait object collections).

**Primitive types implementing `Widget` directly:**
`&str`, `String`, `Span`, `Line<'_>`, `Text<'_>` all implement `Widget`.

### `StatefulWidget` trait

```rust
pub trait StatefulWidget {
    type State: ?Sized;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State);
}
```

- Adds an associated `State` type that persists between renders
- The mutable `state` reference allows the widget to update state (e.g., scroll offset)
- Enables inter-frame persistence without storing state inside the widget itself
- Implemented by: `List` (with `ListState`), `Table` (with `TableState`), `Scrollbar` (with `ScrollbarState`)

**Example stateful widget usage:**
```rust
// Your state persists across frames
let mut list_state = ListState::default();
list_state.select(Some(0));

// Each render: widget renders into buffer AND updates state
let list = List::new(items);
list.render(area, &mut buffer, &mut list_state);  // StatefulWidget::render
```

### How widgets render into a Buffer

Widgets call `Buffer` methods internally. Example custom widget:

```rust
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::text::Line;
use ratatui_core::widgets::Widget;

struct MyWidget;

impl Widget for MyWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Write a styled line to the buffer at the given area
        Line::raw("Hello").render(area, buf);
        // Or use lower-level buffer methods:
        buf.set_string(area.x, area.y, "Hello", Style::default());
    }
}
```

Widgets compose freely — a widget's `render` method can instantiate and call `render` on other widgets.

---

## Q4: Existing Widgets (Paragraph, List, Table, Block, etc.) into an Arbitrary Buffer

**Yes, unconditionally.** All widgets in `ratatui-widgets` implement `Widget::render(self, area, buf)` or `StatefulWidget::render(self, area, buf, state)`. They only require a `&mut Buffer` — they have no knowledge of `Terminal`, `Frame`, or any backend.

`Terminal::draw()` is purely a lifecycle wrapper:
1. It calls your closure with a `Frame`
2. `Frame::render_widget()` calls `widget.render(area, buf)` on the terminal's internal buffer
3. After the closure, Terminal diffs and flushes

You can replicate step 2 directly:

```rust
use ratatui_core::{buffer::Buffer, layout::Rect};
use ratatui_widgets::widgets::{Paragraph, Block, Borders};
use ratatui_core::text::Text;

// Create an arbitrary buffer we own
let area = Rect::new(0, 0, 80, 10);
let mut buffer = Buffer::empty(area);

// Render directly into our buffer — no Terminal needed
let block = Block::default().borders(Borders::ALL).title("My Widget");
block.render(area, &mut buffer);

let para = Paragraph::new(Text::raw("Hello, world!"));
let inner = area.inner(Margin { horizontal: 1, vertical: 1 });
para.render(inner, &mut buffer);
```

The `ratatui-widgets` crate depends only on `ratatui-core` — it has no backend dependencies. There is no path through the widget rendering code that touches a terminal or I/O.

---

## Q5: ratatui-crossterm vs. Raw Crossterm

### What ratatui-crossterm provides

`ratatui-crossterm` is a thin adapter crate:
- `CrosstermBackend<W: Write>` — implements `ratatui_core::backend::Backend` using crossterm commands
- `FromCrossterm` / `IntoCrossterm` — type conversion between crossterm and ratatui types
- Re-exports crossterm as `ratatui_crossterm::crossterm`

`CrosstermBackend` implements the `Backend` trait, which has these key methods:
```rust
fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where I: Iterator<Item = (u16, u16, &'a Cell)>;
fn flush(&mut self) -> Result<(), Self::Error>;
fn size(&self) -> Result<Size, Self::Error>;
fn hide_cursor(&mut self) -> Result<(), Self::Error>;
fn show_cursor(&mut self) -> Result<(), Self::Error>;
fn get_cursor_position(&mut self) -> Result<Position, Self::Error>;
fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error>;
fn clear(&mut self) -> Result<(), Self::Error>;
fn clear_region(&mut self, region: ClearType) -> Result<(), Self::Error>;
fn window_size(&mut self) -> Result<WindowSize, Self::Error>;
fn scroll_region_up(&mut self, region: Range<u16>, amount: u16) -> Result<(), Self::Error>;
fn scroll_region_down(&mut self, region: Range<u16>, amount: u16) -> Result<(), Self::Error>;
```

### Using raw crossterm directly (recommended for custom inline renderer)

For a custom inline renderer, you do NOT need `ratatui-crossterm` at all. You can use raw crossterm to write the diff output from `Buffer::diff()`:

```rust
use crossterm::{
    cursor::MoveTo,
    style::{Color, Print, ResetColor, SetForegroundColor, SetBackgroundColor, SetAttribute, Attribute},
    QueueableCommand,
};
use ratatui_core::buffer::Buffer;
use std::io::{stdout, Write};

// After computing diff between previous and current buffers:
let prev_buffer = Buffer::empty(area);
let new_buffer = /* ... render widgets into this ... */;

let diff = prev_buffer.diff(&new_buffer);
// diff returns Vec<(u16, u16, &Cell)>

let mut stdout = stdout();
for (x, y, cell) in diff {
    stdout.queue(MoveTo(x, y))?;
    // Apply cell styles via crossterm commands
    // Write cell.symbol()
    stdout.queue(Print(cell.symbol()))?;
}
stdout.flush()?;
```

**Summary:** Use `ratatui-core` + `ratatui-widgets` for widget/buffer abstraction, and raw `crossterm` for terminal I/O. Skip `ratatui-crossterm` entirely — it's only needed if you want the full `Terminal<CrosstermBackend>` pipeline.

---

## Q6: Buffer::diff() in Detail

### Signature

```rust
pub fn diff<'a>(&self, other: &'a Self) -> Vec<(u16, u16, &'a Cell)>
```

- `self` is the **previous** (old) buffer
- `other` is the **current** (new) buffer
- Returns a `Vec<(x, y, &Cell)>` — the minimal set of cells that need to be redrawn

### How it works

`diff()` compares the two buffers cell by cell. It returns only cells that have changed — this is the core of Ratatui's efficient rendering strategy. Terminal uses this to avoid writing the entire frame every draw cycle.

**Key behavior details:**
- Multi-width characters are handled specially: if a wide character's trailing cells were previously wide (and are being replaced), blank spacer cells may be emitted to prevent display artifacts
- The `Cell::skip` flag allows marking cells to be excluded from diffing (used by terminal graphics protocols like Sixel/Kitty that manage those pixels directly)

### Using diff() without Terminal

Since `diff()` is a pure method on two `Buffer` instances, it has no connection to `Terminal`. You maintain the double-buffer yourself:

```rust
struct InlineRenderer {
    prev_buffer: Buffer,
    area: Rect,
}

impl InlineRenderer {
    fn render(&mut self) {
        let mut new_buffer = Buffer::empty(self.area);

        // Render widgets into new_buffer
        MyWidget.render(self.area, &mut new_buffer);

        // Compute minimal diff
        let changes = self.prev_buffer.diff(&new_buffer);

        // Write changes to terminal via crossterm
        apply_diff_to_terminal(changes);

        // Swap buffers
        self.prev_buffer = new_buffer;
    }
}
```

### Buffer construction

```rust
// Create a buffer for a given screen region
let area = Rect::new(x, y, width, height);
let buffer = Buffer::empty(area);        // all cells default (space, default style)
let buffer = Buffer::filled(area, cell); // all cells set to a specific Cell value

// Build from text (useful for testing)
let buffer = Buffer::with_lines(["line 1", "line 2"]);
```

Buffer's `area` field specifies global terminal coordinates. `Rect::new(0, 5, 80, 10)` means an 80-wide, 10-tall region starting at row 5. Widgets respect this — they write to the buffer's cells at the appropriate absolute offsets.

---

## Q7: Layout System — Standalone Usage

### Core types

All in `ratatui_core::layout`:
- `Layout` — constraint solver for dividing a `Rect` into sub-regions
- `Constraint` — describes how space is allocated
- `Direction` — `Horizontal` or `Vertical`
- `Rect` — a `{x, y, width, height: u16}` rectangle
- `Flex` — space distribution strategy (alignment for extra space)
- `Position` — `{x, y: u16}`
- `Size` — `{width, height: u16}`
- `Margin` — `{horizontal, vertical: u16}`

### Constraint variants

```rust
pub enum Constraint {
    Length(u16),        // Fixed size in terminal columns/rows
    Percentage(u16),    // % of available space (0–100)
    Ratio(u32, u32),    // Fractional: e.g., Ratio(1, 3) = 1/3
    Fill(u16),          // Flexible; weight for proportional filling
    Min(u16),           // At least this many cells
    Max(u16),           // At most this many cells
}
```

### Layout construction and splitting

```rust
use ratatui_core::layout::{Layout, Constraint, Direction, Rect};

let area = Rect::new(0, 0, 80, 24);

// Vertical split: fixed header, flexible body, fixed footer
let [header, body, footer] = Layout::vertical([
    Constraint::Length(3),
    Constraint::Fill(1),
    Constraint::Length(1),
]).areas(area);

// Horizontal split: sidebar + main
let [sidebar, main] = Layout::horizontal([
    Constraint::Length(20),
    Constraint::Fill(1),
]).areas(area);
```

**Key methods:**
```rust
// Type-safe compile-time count (returns [Rect; N])
let areas: [Rect; 3] = layout.areas(area);

// Dynamic count (returns Rc<[Rect]>)
let areas: Rc<[Rect]> = layout.split(area);

// Returns areas plus spacer rects between them
let (areas, spacers) = layout.split_with_spacers(area);
```

**Configuration builder:**
```rust
Layout::vertical(constraints)
    .direction(Direction::Vertical)   // can also set direction explicitly
    .margin(2)                        // uniform margin inside the area
    .horizontal_margin(1)
    .vertical_margin(1)
    .flex(Flex::Start)                // where to place extra space
    .spacing(1)                       // gap between segments
```

### Standalone usage

**Layout is completely standalone.** It uses a Cassowary-based linear constraint solver (internally called "kasuari"). Results are cached in thread-local LRU storage (default 500 entries, configurable via `Layout::init_cache(n)`).

Layout requires only a `Rect` as input — no terminal, no backend, no I/O. You can use it to calculate sub-regions for any rendering purpose:

```rust
// Completely standalone — just a math operation on Rect values
let area = Rect::new(0, 0, 80, 10);
let [top, bottom] = Layout::vertical([
    Constraint::Percentage(30),
    Constraint::Fill(1),
]).areas(area);
// top = Rect { x: 0, y: 0, width: 80, height: 3 }
// bottom = Rect { x: 0, y: 3, width: 80, height: 7 }
```

---

## Architecture Diagram

```
ratatui-core
├── buffer::{Buffer, Cell}          ← we own and manage these
├── layout::{Layout, Rect, Constraint, Direction}
├── style::{Style, Color, Modifier}
├── text::{Text, Line, Span}
├── widgets::{Widget, StatefulWidget}  ← traits only
├── backend::Backend               ← trait only (no impl)
└── terminal::{Terminal, Frame}    ← optional, requires a Backend

ratatui-widgets  (depends on ratatui-core only)
└── All built-in widgets           ← implement Widget/StatefulWidget
    (Paragraph, List, Table, Block, etc.)

ratatui-crossterm  (depends on ratatui-core + crossterm)
└── CrosstermBackend               ← implements Backend trait

Our custom inline renderer:
├── Depends on: ratatui-core + ratatui-widgets + crossterm (raw)
├── Owns: two Buffer instances (current + previous)
├── Flow: fill new_buffer via widget.render() → diff() → write to terminal
└── Does NOT need: ratatui-crossterm, Terminal struct, Frame, Terminal::draw()
```

---

## Dependency Setup (Cargo.toml)

```toml
[dependencies]
ratatui-core = "0.1"
ratatui-widgets = "0.1"       # for Paragraph, List, Table, Block, etc.
crossterm = "0.29"            # for raw terminal I/O
```

Alternatively, use the main `ratatui` crate (which re-exports everything) plus crossterm directly:

```toml
[dependencies]
ratatui = { version = "0.30", default-features = false, features = ["crossterm"] }
# ratatui re-exports ratatui_core and ratatui_widgets types
```

---

## Key Takeaways

1. **Widget rendering is backend-agnostic.** `Widget::render(self, area, &mut Buffer)` is the entire contract. No terminal I/O occurs.

2. **Buffer is a plain owned data structure.** `Buffer::empty(area)` gives you a grid of `Cell`s you own completely. No global state, no singletons.

3. **Buffer::diff() is a pure method.** Call it on any two buffers with matching areas. Terminal uses it internally but you can use it directly in a custom pipeline.

4. **Layout is pure math.** `Layout::split(area)` / `Layout::areas(area)` returns `Rect` values. No I/O required.

5. **ratatui-widgets has no backend dependencies.** It depends only on `ratatui-core`. All 16 widgets (Paragraph, List, Table, Block, etc.) render purely into a Buffer.

6. **ratatui-crossterm is optional.** It's only needed for the `Terminal<CrosstermBackend>` path. For a custom renderer, use raw crossterm for I/O and `ratatui-core` for the widget/buffer abstraction.

7. **Terminal struct is optional.** It provides a convenient double-buffer lifecycle (render → diff → flush → swap). You can reimplement this yourself in ~20 lines to get inline rendering behavior.

---

## Sources

- [v0.30.0 | Ratatui](https://ratatui.rs/highlights/v030/)
- [ratatui_core - Rust (docs.rs)](https://docs.rs/ratatui-core/latest/ratatui_core/)
- [Widget in ratatui::widgets - Rust](https://docs.rs/ratatui/latest/ratatui/widgets/trait.Widget.html)
- [StatefulWidget in ratatui::widgets - Rust](https://docs.rs/ratatui/latest/ratatui/widgets/trait.StatefulWidget.html)
- [Buffer in ratatui-core - Rust](https://docs.rs/ratatui-core/latest/ratatui_core/buffer/struct.Buffer.html)
- [Cell in ratatui::buffer - Rust](https://docs.rs/ratatui/latest/ratatui/buffer/struct.Cell.html)
- [Layout in ratatui - Rust](https://docs.rs/ratatui/latest/ratatui/layout/struct.Layout.html)
- [Backend trait - Rust](https://docs.rs/ratatui/latest/ratatui/backend/trait.Backend.html)
- [CrosstermBackend - Rust](https://docs.rs/ratatui-crossterm/latest/ratatui_crossterm/struct.CrosstermBackend.html)
- [Rendering under the hood | Ratatui](https://ratatui.rs/concepts/rendering/under-the-hood/)
- [Introduction to Widgets | Ratatui](https://ratatui.rs/concepts/widgets/)
- [ratatui-crossterm - crates.io](https://crates.io/crates/ratatui-crossterm)
- [ratatui-widgets - crates.io](https://crates.io/crates/ratatui-widgets)
- [ratatui-core - crates.io](https://crates.io/crates/ratatui-core)
- [Release ratatui-v0.30.0 - GitHub](https://github.com/ratatui/ratatui/releases/tag/ratatui-v0.30.0)
- [Terminal in ratatui - Rust](https://docs.rs/ratatui/latest/ratatui/struct.Terminal.html)
