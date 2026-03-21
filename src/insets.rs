/// Insets define the content area within a component's render area.
///
/// The renderer lays out children inside the inset region. The component
/// renders its own chrome (borders, padding, backgrounds) in the outer
/// area via `render()`.
///
/// ```ignore
/// Insets::all(1)       // 1-cell border on all sides
/// Insets::symmetric(1, 2)  // 1 top/bottom, 2 left/right
/// Insets::new().top(1).left(2)  // builder style
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

impl Insets {
    /// No insets — children get the full area.
    pub const ZERO: Insets = Insets {
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
    };

    /// Create zero insets (same as ZERO).
    pub fn new() -> Self {
        Self::ZERO
    }

    /// Uniform insets on all sides.
    pub fn all(n: u16) -> Self {
        Self {
            top: n,
            right: n,
            bottom: n,
            left: n,
        }
    }

    /// Symmetric insets: `vertical` for top/bottom, `horizontal` for left/right.
    pub fn symmetric(vertical: u16, horizontal: u16) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Set the top inset.
    pub fn top(mut self, v: u16) -> Self {
        self.top = v;
        self
    }

    /// Set the bottom inset.
    pub fn bottom(mut self, v: u16) -> Self {
        self.bottom = v;
        self
    }

    /// Set the left inset.
    pub fn left(mut self, v: u16) -> Self {
        self.left = v;
        self
    }

    /// Set the right inset.
    pub fn right(mut self, v: u16) -> Self {
        self.right = v;
        self
    }

    /// Total horizontal inset (left + right).
    pub fn horizontal(&self) -> u16 {
        self.left + self.right
    }

    /// Total vertical inset (top + bottom).
    pub fn vertical(&self) -> u16 {
        self.top + self.bottom
    }
}
