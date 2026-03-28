//! Cell measurement type for component props.
//!
//! [`Cells`] is a newtype over `u16` that represents a measurement in
//! terminal cells (columns or rows). It exists primarily to make the
//! [`element!`](crate::element!) macro ergonomic — integer literals like
//! `1` or `3` convert seamlessly via `.into()` without needing a `u16`
//! suffix.
//!
//! # Examples
//!
//! ```
//! use eye_declare::Cells;
//!
//! // From integer literals
//! let c: Cells = 4.into();
//! assert_eq!(c.0, 4);
//!
//! // Negative values clamp to 0
//! let c: Cells = (-1).into();
//! assert_eq!(c.0, 0);
//!
//! // Large values clamp to u16::MAX
//! let c: Cells = (100_000i32).into();
//! assert_eq!(c.0, u16::MAX);
//! ```

/// A measurement in terminal cells (columns or rows).
///
/// Wraps `u16`. Implements `From` for common integer types so that
/// bare literals work in the [`element!`](crate::element!) macro:
///
/// ```ignore
/// View(padding: 1) { ... }  // 1i32 → Cells via From<i32>
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cells(pub u16);

impl Cells {
    /// Zero cells.
    pub const ZERO: Cells = Cells(0);
}

impl From<u16> for Cells {
    fn from(v: u16) -> Self {
        Cells(v)
    }
}

impl From<u8> for Cells {
    fn from(v: u8) -> Self {
        Cells(v as u16)
    }
}

impl From<i32> for Cells {
    fn from(v: i32) -> Self {
        Cells(v.clamp(0, u16::MAX as i32) as u16)
    }
}

impl From<usize> for Cells {
    fn from(v: usize) -> Self {
        Cells(v.min(u16::MAX as usize) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_i32() {
        let c: Cells = 5.into();
        assert_eq!(c.0, 5);
    }

    #[test]
    fn from_i32_negative_clamps_to_zero() {
        let c: Cells = (-10).into();
        assert_eq!(c.0, 0);
    }

    #[test]
    fn from_i32_large_clamps_to_max() {
        let c: Cells = (100_000i32).into();
        assert_eq!(c.0, u16::MAX);
    }

    #[test]
    fn from_u16() {
        let c: Cells = 42u16.into();
        assert_eq!(c.0, 42);
    }

    #[test]
    fn from_u8() {
        let c: Cells = 7u8.into();
        assert_eq!(c.0, 7);
    }

    #[test]
    fn from_usize() {
        let c: Cells = 100usize.into();
        assert_eq!(c.0, 100);
    }

    #[test]
    fn from_usize_large_clamps_to_max() {
        let c: Cells = (usize::MAX).into();
        assert_eq!(c.0, u16::MAX);
    }

    #[test]
    fn default_is_zero() {
        assert_eq!(Cells::default(), Cells(0));
    }
}
