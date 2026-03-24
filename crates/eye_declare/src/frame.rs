use ratatui_core::{
    buffer::{Buffer, Cell},
    layout::Rect,
};

/// The output of a render pass. Owns the buffer.
pub struct Frame {
    buffer: Buffer,
}

impl Frame {
    pub fn new(buffer: Buffer) -> Self {
        Self { buffer }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn area(&self) -> Rect {
        self.buffer.area
    }

    /// Diff against a previous frame, producing the set of changed cells.
    ///
    /// Handles height mismatches: if the frames have different heights,
    /// the shorter one is logically padded with empty cells so that
    /// `Buffer::diff` can operate on matching dimensions.
    pub fn diff(&self, previous: &Frame) -> Diff {
        let new_area = self.buffer.area;
        let prev_area = previous.buffer.area;

        // Fast path: same dimensions, use Buffer::diff directly
        if new_area == prev_area {
            let changes = previous
                .buffer
                .diff(&self.buffer)
                .into_iter()
                .map(|(x, y, cell)| (x, y, cell.clone()))
                .collect();
            return Diff {
                cells: changes,
                new_area,
                prev_area,
            };
        }

        // Heights differ — compare directly without allocating padded buffers.
        // Cells within both buffers are compared normally; cells only in one
        // buffer are compared against a default (empty) cell.
        let max_width = new_area.width.max(prev_area.width);
        let max_height = new_area.height.max(prev_area.height);
        let default_cell = Cell::default();
        let mut changes = Vec::new();

        for y in 0..max_height {
            for x in 0..max_width {
                let in_prev = x < prev_area.width && y < prev_area.height;
                let in_new = x < new_area.width && y < new_area.height;

                let prev_cell = if in_prev {
                    &previous.buffer[(x, y)]
                } else {
                    &default_cell
                };
                let new_cell = if in_new {
                    &self.buffer[(x, y)]
                } else {
                    &default_cell
                };

                if prev_cell != new_cell {
                    changes.push((x, y, new_cell.clone()));
                }
            }
        }

        Diff {
            cells: changes,
            new_area,
            prev_area,
        }
    }
}

/// A set of changed cells between two frames.
pub struct Diff {
    /// Changed cells: (x, y, new_cell).
    pub(crate) cells: Vec<(u16, u16, Cell)>,
    /// The area of the new (current) frame.
    pub(crate) new_area: Rect,
    /// The area of the previous frame.
    pub(crate) prev_area: Rect,
}

impl Frame {
    /// Create a new frame with the top `n` rows removed.
    ///
    /// Used for committed scrollback: committed rows are sliced off
    /// so subsequent diffs only cover the active region.
    pub fn slice_top_rows(&self, n: u16) -> Frame {
        let old_area = self.buffer.area;
        let new_height = old_area.height.saturating_sub(n);
        if new_height == 0 {
            return Frame::new(Buffer::empty(Rect::new(0, 0, old_area.width, 0)));
        }
        let new_area = Rect::new(0, 0, old_area.width, new_height);
        let mut new_buf = Buffer::empty(new_area);
        for y in 0..new_height {
            for x in 0..old_area.width {
                new_buf[(x, y)] = self.buffer[(x, y + n)].clone();
            }
        }
        Frame::new(new_buf)
    }
}

impl Diff {
    /// Whether there are no changes.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Number of changed cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// The area of the new frame.
    pub fn new_area(&self) -> Rect {
        self.new_area
    }

    /// The area of the previous frame.
    pub fn prev_area(&self) -> Rect {
        self.prev_area
    }

    /// Whether the frame grew (new frame is taller than previous).
    pub fn grew(&self) -> bool {
        self.new_area.height > self.prev_area.height
    }

    /// How many rows the frame grew by (0 if it didn't grow).
    pub fn growth(&self) -> u16 {
        self.new_area.height.saturating_sub(self.prev_area.height)
    }

    /// Remove cells that are above the visible area (in scrollback).
    ///
    /// Cells at row < `min_row` are in terminal scrollback and can't
    /// be modified. Filtering them prevents cursor tracking drift.
    pub fn retain_visible(&mut self, min_row: u16) {
        if min_row > 0 {
            self.cells.retain(|(_, y, _)| *y >= min_row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_frame(lines: &[&str]) -> Frame {
        Frame::new(Buffer::with_lines(lines.iter().map(|s| s.to_string())))
    }

    #[test]
    fn diff_identical_frames_is_empty() {
        let f1 = make_frame(&["hello", "world"]);
        let f2 = make_frame(&["hello", "world"]);
        let diff = f2.diff(&f1);
        assert!(diff.is_empty());
    }

    #[test]
    fn diff_single_cell_change() {
        let f1 = make_frame(&["hello"]);
        let f2 = make_frame(&["hallo"]);
        let diff = f2.diff(&f1);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff.cells[0].0, 1); // x=1 (the 'a' vs 'e')
        assert_eq!(diff.cells[0].1, 0); // y=0
    }

    #[test]
    fn diff_height_growth() {
        let f1 = make_frame(&["hello"]);
        let f2 = make_frame(&["hello", "world"]);
        let diff = f2.diff(&f1);
        assert!(diff.grew());
        assert_eq!(diff.growth(), 1);
        // The new row should have changed cells
        let new_row_cells: Vec<_> = diff.cells.iter().filter(|(_, y, _)| *y == 1).collect();
        assert!(!new_row_cells.is_empty());
    }

    #[test]
    fn diff_no_growth_same_height() {
        let f1 = make_frame(&["hello", "world"]);
        let f2 = make_frame(&["hello", "earth"]);
        let diff = f2.diff(&f1);
        assert!(!diff.grew());
        assert_eq!(diff.growth(), 0);
    }

    #[test]
    fn diff_height_shrink() {
        let f1 = make_frame(&["hello", "world"]);
        let f2 = make_frame(&["hello"]);
        let diff = f2.diff(&f1);
        assert!(!diff.grew());
        // The removed row should show as changed (cleared to empty)
        let removed_row_cells: Vec<_> = diff.cells.iter().filter(|(_, y, _)| *y == 1).collect();
        assert!(!removed_row_cells.is_empty());
    }
}
