//! Built-in components shipped with eye_declare.
//!
//! These cover common terminal UI patterns out of the box. For custom
//! components, implement the [`Component`](crate::Component) trait directly.

/// Markdown rendering component. See [`Markdown`].
pub mod markdown;
/// Animated spinner component. See [`Spinner`].
pub mod spinner;
/// Styled text component with word wrapping. See [`TextBlock`].
pub mod text;

pub use markdown::{Markdown, MarkdownState};
pub use spinner::{Spinner, SpinnerState};
pub use text::TextBlock;
