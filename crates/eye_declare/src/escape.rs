use ratatui_core::style::{Color, Modifier, Style};

use crate::frame::Diff;

/// Begin Synchronized Update (DEC private mode 2026).
const BSU: &[u8] = b"\x1b[?2026h";
/// End Synchronized Update.
const ESU: &[u8] = b"\x1b[?2026l";
/// Hide cursor.
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";
/// Show cursor.
#[allow(dead_code)]
const SHOW_CURSOR: &[u8] = b"\x1b[?25h";

/// Tracks terminal cursor position and current style to minimize output.
#[derive(Debug, Clone)]
pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub style: Style,
}

impl CursorState {
    pub fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            style: Style::default(),
        }
    }
}

impl Default for CursorState {
    fn default() -> Self {
        Self::new()
    }
}

impl Diff {
    /// Convert this diff to ready-to-write terminal escape sequences.
    ///
    /// Uses **relative cursor movement only** — no absolute positioning.
    /// This is critical for inline rendering where the terminal origin
    /// row is unknown. The `cursor` tracks position in buffer-local
    /// coordinates (row 0 = first row of our content).
    ///
    /// The output is wrapped in DEC 2026 synchronized output sequences.
    /// The cursor is hidden during the update but **not shown** at the
    /// end — the caller is responsible for positioning and showing the
    /// cursor afterward (e.g., at the focused input's cursor position).
    pub fn to_escape_sequences(&self, cursor: &mut CursorState) -> Vec<u8> {
        if self.cells.is_empty() {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(self.cells.len() * 12);

        // Begin synchronized update + hide cursor
        out.extend_from_slice(BSU);
        out.extend_from_slice(HIDE_CURSOR);

        // Sort cells by (y, x) for sequential writing
        let mut cells = self.cells.clone();
        cells.sort_by_key(|(x, y, _)| (*y, *x));

        for (x, y, cell) in &cells {
            let target_row = *y;
            let target_col = *x;

            // Move cursor to the target position using relative movement
            let need_move = cursor.row != target_row || cursor.col != target_col;

            if need_move {
                write_relative_move(&mut out, cursor, target_row, target_col);
            }

            // Apply style changes
            write_style_diff(&mut out, &cursor.style, &cell.style());
            cursor.style = cell.style();

            // Write the cell symbol
            let symbol = cell.symbol();
            out.extend_from_slice(symbol.as_bytes());

            // Advance cursor column by the symbol's display width
            let width = unicode_display_width(symbol);
            cursor.col = cursor.col.saturating_add(width as u16);
        }

        // Reset style so we don't leak into terminal
        out.extend_from_slice(b"\x1b[0m");
        cursor.style = Style::default();

        // End synchronized update (caller manages cursor visibility)
        out.extend_from_slice(ESU);

        out
    }
}

/// Move the cursor from its current position to (target_row, target_col)
/// using relative movement escape sequences only.
pub fn write_relative_move(
    out: &mut Vec<u8>,
    cursor: &mut CursorState,
    target_row: u16,
    target_col: u16,
) {
    // Vertical movement
    if target_row < cursor.row {
        let n = cursor.row - target_row;
        if n == 1 {
            out.extend_from_slice(b"\x1b[A");
        } else {
            out.extend_from_slice(format!("\x1b[{}A", n).as_bytes());
        }
    } else if target_row > cursor.row {
        let n = target_row - cursor.row;
        if n == 1 {
            out.extend_from_slice(b"\x1b[B");
        } else {
            out.extend_from_slice(format!("\x1b[{}B", n).as_bytes());
        }
    }
    cursor.row = target_row;

    // Horizontal: always CR + forward, since we don't know the
    // current absolute column reliably after style sequences
    out.push(b'\r'); // carriage return → column 0
    cursor.col = 0;

    if target_col > 0 {
        if target_col == 1 {
            out.extend_from_slice(b"\x1b[C");
        } else {
            out.extend_from_slice(format!("\x1b[{}C", target_col).as_bytes());
        }
    }
    cursor.col = target_col;
}

/// Write the minimal SGR escape sequence to transition from `from` to `to`.
fn write_style_diff(out: &mut Vec<u8>, from: &Style, to: &Style) {
    if from == to {
        return;
    }

    // Check if we need a reset (removing attributes is easier with reset + re-apply)
    let from_mods = from.add_modifier;
    let to_mods = to.add_modifier;

    // If any modifiers were removed, we need to reset and re-apply
    let removed_mods = from_mods.difference(to_mods);
    let needs_reset = !removed_mods.is_empty()
        || (from.fg.is_some() && to.fg.is_none())
        || (from.bg.is_some() && to.bg.is_none());

    if needs_reset {
        out.extend_from_slice(b"\x1b[0m");
        // After reset, re-apply everything from `to`
        write_full_style(out, to);
    } else {
        // Incremental: only emit what changed
        write_incremental_style(out, from, to);
    }
}

/// Write a complete style (after reset).
fn write_full_style(out: &mut Vec<u8>, style: &Style) {
    let mut params: Vec<u8> = Vec::new();
    let mut first = true;

    macro_rules! push_param {
        ($val:expr) => {
            if !first {
                params.push(b';');
            }
            params.extend_from_slice($val.to_string().as_bytes());
            first = false;
        };
    }

    // Modifiers
    let mods = style.add_modifier;
    if mods.contains(Modifier::BOLD) {
        push_param!(1);
    }
    if mods.contains(Modifier::DIM) {
        push_param!(2);
    }
    if mods.contains(Modifier::ITALIC) {
        push_param!(3);
    }
    if mods.contains(Modifier::UNDERLINED) {
        push_param!(4);
    }
    if mods.contains(Modifier::SLOW_BLINK) {
        push_param!(5);
    }
    if mods.contains(Modifier::RAPID_BLINK) {
        push_param!(6);
    }
    if mods.contains(Modifier::REVERSED) {
        push_param!(7);
    }
    if mods.contains(Modifier::HIDDEN) {
        push_param!(8);
    }
    if mods.contains(Modifier::CROSSED_OUT) {
        push_param!(9);
    }

    // Foreground
    if let Some(fg) = style.fg {
        write_color_params(&mut params, fg, true, &mut first);
    }

    // Background
    if let Some(bg) = style.bg {
        write_color_params(&mut params, bg, false, &mut first);
    }

    if !params.is_empty() {
        out.extend_from_slice(b"\x1b[");
        out.extend_from_slice(&params);
        out.push(b'm');
    }
}

/// Write only the changed parts of a style (no reset needed).
fn write_incremental_style(out: &mut Vec<u8>, from: &Style, to: &Style) {
    let mut params: Vec<u8> = Vec::new();
    let mut first = true;

    macro_rules! push_param {
        ($val:expr) => {
            if !first {
                params.push(b';');
            }
            params.extend_from_slice($val.to_string().as_bytes());
            first = false;
        };
    }

    // Added modifiers
    let added_mods = to.add_modifier.difference(from.add_modifier);
    if added_mods.contains(Modifier::BOLD) {
        push_param!(1);
    }
    if added_mods.contains(Modifier::DIM) {
        push_param!(2);
    }
    if added_mods.contains(Modifier::ITALIC) {
        push_param!(3);
    }
    if added_mods.contains(Modifier::UNDERLINED) {
        push_param!(4);
    }
    if added_mods.contains(Modifier::SLOW_BLINK) {
        push_param!(5);
    }
    if added_mods.contains(Modifier::RAPID_BLINK) {
        push_param!(6);
    }
    if added_mods.contains(Modifier::REVERSED) {
        push_param!(7);
    }
    if added_mods.contains(Modifier::HIDDEN) {
        push_param!(8);
    }
    if added_mods.contains(Modifier::CROSSED_OUT) {
        push_param!(9);
    }

    // Foreground change
    if from.fg != to.fg
        && let Some(fg) = to.fg {
            write_color_params(&mut params, fg, true, &mut first);
        }

    // Background change
    if from.bg != to.bg
        && let Some(bg) = to.bg {
            write_color_params(&mut params, bg, false, &mut first);
        }

    if !params.is_empty() {
        out.extend_from_slice(b"\x1b[");
        out.extend_from_slice(&params);
        out.push(b'm');
    }
}

/// Append a single SGR parameter value.
fn push_param(params: &mut Vec<u8>, first: &mut bool, val: u16) {
    if !*first {
        params.push(b';');
    }
    params.extend_from_slice(val.to_string().as_bytes());
    *first = false;
}

/// Append SGR color parameters for a Color.
fn write_color_params(params: &mut Vec<u8>, color: Color, is_fg: bool, first: &mut bool) {
    let code = |fg: u16, bg: u16| -> u16 { if is_fg { fg } else { bg } };

    match color {
        Color::Reset => push_param(params, first, code(39, 49)),
        Color::Black => push_param(params, first, code(30, 40)),
        Color::Red => push_param(params, first, code(31, 41)),
        Color::Green => push_param(params, first, code(32, 42)),
        Color::Yellow => push_param(params, first, code(33, 43)),
        Color::Blue => push_param(params, first, code(34, 44)),
        Color::Magenta => push_param(params, first, code(35, 45)),
        Color::Cyan => push_param(params, first, code(36, 46)),
        Color::Gray => push_param(params, first, code(37, 47)),
        Color::DarkGray => push_param(params, first, code(90, 100)),
        Color::LightRed => push_param(params, first, code(91, 101)),
        Color::LightGreen => push_param(params, first, code(92, 102)),
        Color::LightYellow => push_param(params, first, code(93, 103)),
        Color::LightBlue => push_param(params, first, code(94, 104)),
        Color::LightMagenta => push_param(params, first, code(95, 105)),
        Color::LightCyan => push_param(params, first, code(96, 106)),
        Color::White => push_param(params, first, code(97, 107)),
        Color::Indexed(i) => {
            push_param(params, first, code(38, 48));
            push_param(params, first, 5);
            push_param(params, first, i as u16);
        }
        Color::Rgb(r, g, b) => {
            push_param(params, first, code(38, 48));
            push_param(params, first, 2);
            push_param(params, first, r as u16);
            push_param(params, first, g as u16);
            push_param(params, first, b as u16);
        }
    }
}

/// Get the display width of a string in terminal columns.
fn unicode_display_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Diff, Frame};
    use ratatui_core::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Style},
    };

    fn make_frame(lines: &[&str]) -> Frame {
        Frame::new(Buffer::with_lines(lines.iter().map(|s| s.to_string())))
    }

    #[test]
    fn empty_diff_produces_no_output() {
        let diff = Diff {
            cells: vec![],
            new_area: Rect::new(0, 0, 5, 1),
            prev_area: Rect::new(0, 0, 5, 1),
        };
        let mut cursor = CursorState::new();
        let output = diff.to_escape_sequences(&mut cursor);
        assert!(output.is_empty());
    }

    #[test]
    fn single_cell_produces_sync_wrapped_output() {
        let f1 = make_frame(&["hello"]);
        let f2 = make_frame(&["hallo"]);
        let diff = f2.diff(&f1);
        let mut cursor = CursorState::new();
        let output = diff.to_escape_sequences(&mut cursor);
        let s = String::from_utf8_lossy(&output);

        // Should start with BSU and end with ESU
        assert!(s.starts_with("\x1b[?2026h"));
        assert!(s.ends_with("\x1b[?2026l"));

        // Should contain the character 'a'
        assert!(s.contains('a'));
    }

    #[test]
    fn relative_movement_for_non_origin_cell() {
        let f1 = make_frame(&["hello"]);
        let f2 = make_frame(&["hallo"]);
        let diff = f2.diff(&f1);
        let mut cursor = CursorState::new();
        let output = diff.to_escape_sequences(&mut cursor);
        let s = String::from_utf8_lossy(&output);

        // Changed cell is at (1, 0). Cursor starts at (0, 0).
        // Should use CR + forward 1 to reach column 1 (no absolute addressing)
        assert!(s.contains('\r'));
        assert!(s.contains("\x1b[C")); // forward 1
        // Should NOT contain absolute positioning (CSI row;col H)
        assert!(!s.contains("H"));
    }

    #[test]
    fn consecutive_cells_advance_cursor() {
        let f1 = make_frame(&["hello"]);
        let f2 = make_frame(&["abllo"]);
        let diff = f2.diff(&f1);
        let mut cursor = CursorState::new();
        let output = diff.to_escape_sequences(&mut cursor);
        let s = String::from_utf8_lossy(&output);

        // Should contain both changed characters
        assert!(s.contains('a'));
        assert!(s.contains('b'));
    }

    #[test]
    fn multirow_uses_relative_vertical_movement() {
        let f1 = make_frame(&["hello", "world"]);
        let f2 = make_frame(&["hello", "earth"]);
        let diff = f2.diff(&f1);
        let mut cursor = CursorState::new();
        let output = diff.to_escape_sequences(&mut cursor);
        let s = String::from_utf8_lossy(&output);

        // Changed cells are on row 1. Cursor starts at row 0.
        // Should use CSI 1 B (down 1) for vertical movement
        assert!(s.contains("\x1b[B"));
    }

    #[test]
    fn style_change_emits_sgr() {
        let area = Rect::new(0, 0, 5, 1);
        let mut buf1 = Buffer::empty(area);
        buf1.set_string(0, 0, "hello", Style::default());

        let mut buf2 = Buffer::empty(area);
        buf2.set_string(0, 0, "hello", Style::default().fg(Color::Red));

        let f1 = Frame::new(buf1);
        let f2 = Frame::new(buf2);
        let diff = f2.diff(&f1);
        let mut cursor = CursorState::new();
        let output = diff.to_escape_sequences(&mut cursor);
        let s = String::from_utf8_lossy(&output);

        // Should contain SGR for red foreground (31)
        assert!(s.contains("31"));
    }
}
