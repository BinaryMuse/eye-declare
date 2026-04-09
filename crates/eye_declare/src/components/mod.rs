//! Built-in components shipped with eye_declare.
//!
//! These cover common terminal UI patterns out of the box. For custom
//! components, implement the [`Component`](crate::Component) trait directly.

/// Raw buffer rendering component. See [`Canvas`].
pub mod canvas;
/// Markdown rendering component. See [`Markdown`].
pub mod markdown;
/// Animated spinner component. See [`Spinner`].
pub mod spinner;
/// Styled text component with word wrapping. See [`Text`](text::Text).
pub mod text;
/// Unified layout container. See [`View`].
pub mod view;
/// Fixed-height viewport into a text buffer. See [`Viewport`].
pub mod viewport;

pub use canvas::Canvas;
pub use markdown::{Markdown, MarkdownState};
pub use spinner::{Spinner, SpinnerState};
pub use text::{Span, Text, TextChild};
pub use view::{Direction, View};
pub use viewport::Viewport;
