//! Declarative inline TUI rendering for Rust, built on [Ratatui](https://ratatui.rs).
//!
//! eye_declare provides a React-like component model for terminal UIs that render
//! **inline** — content grows downward into the terminal's native scrollback rather
//! than taking over the full screen. This makes it ideal for CLI tools, AI assistants,
//! build systems, and interactive prompts where earlier output should remain visible.
//!
//! # Quick start
//!
//! ```ignore
//! use eye_declare::{element, Application, Elements, Spinner};
//!
//! struct State { messages: Vec<String>, loading: bool }
//!
//! fn view(state: &State) -> Elements {
//!     element! {
//!         #(for (i, msg) in state.messages.iter().enumerate() {
//!             Text(key: format!("msg-{i}")) { #(msg.clone()) }
//!         })
//!         #(if state.loading {
//!             Spinner(key: "loading", label: "Thinking...")
//!         })
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let (mut app, handle) = Application::builder()
//!         .state(State { messages: vec![], loading: true })
//!         .view(view)
//!         .build()?;
//!
//!     tokio::spawn(async move {
//!         handle.update(|s| s.messages.push("Hello!".into()));
//!         handle.update(|s| s.loading = false);
//!     });
//!
//!     app.run().await
//! }
//! ```
//!
//! # Core concepts
//!
//! - **[`#[component]`](macro@component) + [`#[props]`](macro@props)** — Define
//!   components as functions. Props are a struct with `#[props]`; the function
//!   receives props, optional state, hooks, and children, returning an [`Elements`]
//!   tree. Hooks declare behavioral capabilities (events, focus, intervals).
//!
//! - **[`Elements`]** — A list of component descriptions returned by view functions.
//!   The framework reconciles the new list against the existing tree, preserving
//!   state for reused nodes.
//!
//! - **[`element!`]** — A JSX-like proc macro for building `Elements` declaratively.
//!   Supports props, children, keys, conditionals (`#(if ...)`), loops (`#(for ...)`),
//!   and splicing pre-built `Elements` (`#(expr)`).
//!
//! - **[`Application`]** — Owns your state and manages the render loop.
//!   [`Handle`] lets you send state updates from any thread or async task.
//!
//! - **[`InlineRenderer`]** — The lower-level rendering engine for when you need
//!   direct control over the render loop (sync code, embedding, custom event loops).
//!
//! # Built-in components
//!
//! | Component | Description |
//! |-----------|-------------|
//! | [`Text`] | Styled text with word wrapping (data children: [`Span`]) |
//! | [`Spinner`] | Animated spinner with auto-tick via lifecycle hooks |
//! | [`Markdown`] | Headings, bold, italic, inline code, code blocks, lists |
//! | [`View`] | Unified layout container with optional borders, padding, and background |
//! | [`Canvas`] | Raw buffer rendering via a user-provided closure |
//! | [`VStack`] | Vertical container — children stack top-to-bottom |
//! | [`HStack`] | Horizontal container with [`WidthConstraint`]-based layout |
//!
//! # Layout
//!
//! Vertical stacking is the default. [`HStack`] provides horizontal layout where
//! children declare their width via [`WidthConstraint::Fixed`] or [`WidthConstraint::Fill`].
//! Components can declare [`Insets`] for border/padding chrome — children render inside
//! the inset area while the component draws its chrome in the full area.
//!
//! # Lifecycle hooks
//!
//! Components declare effects using the [`Hooks`] API:
//!
//! - [`Hooks::use_interval`] — periodic callback (e.g., animation)
//! - [`Hooks::use_mount`] — fires once after the component is built
//! - [`Hooks::use_unmount`] — fires when the component is removed
//! - [`Hooks::use_autofocus`] — request focus on mount
//! - [`Hooks::use_focus_scope`] — confine Tab cycling to this subtree
//! - [`Hooks::provide_context`] — make a value available to descendants
//! - [`Hooks::use_context`] — read a value provided by an ancestor
//!
//! # Context
//!
//! The context system lets ancestor components provide values to their
//! descendants without prop-drilling. Register root-level contexts via
//! [`ApplicationBuilder::with_context`], or have components provide
//! them via [`Hooks::provide_context`] in their component function.
//! Descendants read context values with [`Hooks::use_context`].
//!
//! This is commonly used with [`Application::run_loop`] to give
//! components access to an app-domain event channel:
//!
//! ```ignore
//! let (tx, mut rx) = tokio::sync::mpsc::channel(32);
//! let (mut app, handle) = Application::builder()
//!     .state(MyState::default())
//!     .view(my_view)
//!     .with_context(tx)
//!     .build()?;
//!
//! let h = handle.clone();
//! tokio::spawn(async move {
//!     while let Some(event) = rx.recv().await {
//!         match event {
//!             AppEvent::Submit(val) => h.update(|s| s.result = val),
//!             AppEvent::Quit => { h.exit(); break; }
//!         }
//!     }
//! });
//!
//! app.run_loop().await?;
//! ```
//!
//! # Feature flags
//!
//! | Flag | Default | Description |
//! |------|---------|-------------|
//! | `macros` | yes | Enables the [`element!`] proc macro via `eye_declare_macros` |

/// Application wrapper, builder, handle, and control flow types.
///
/// See [`Application`] for the high-level entry point.
pub mod app;

/// Traits and types for the `element!` macro's child collection system.
///
/// Most users won't interact with this module directly — it powers the
/// `element!` macro's ability to type-check parent-child relationships
/// at compile time. See [`ChildCollector`] if you're building a component
/// that accepts data children (like [`Text`](crate::Text) accepts [`Span`](crate::Span)s).
pub mod children;

/// The [`Component`] trait (framework-internal) and built-in containers
/// ([`VStack`], [`HStack`], [`Column`]).
pub mod component;

/// Built-in components: [`Text`](components::text::Text),
/// [`Spinner`](components::spinner::Spinner),
/// [`Markdown`](components::markdown::Markdown), and
/// [`View`](components::view::View).
pub mod components;

/// The [`Elements`] list and [`ElementHandle`] for building component trees.
pub mod element;

/// Lifecycle hooks for declaring component effects.
///
/// See [`Hooks`] for the API used inside `#[component]` functions.
pub mod hooks;

/// The [`InlineRenderer`] — low-level inline rendering engine.
///
/// Use this when you need direct control over the render loop
/// rather than the higher-level [`Application`] wrapper.
pub mod inline;

/// The [`Insets`] type for declaring content padding and border chrome.
pub mod insets;

/// Cell measurement type for component props. See [`Cells`].
pub mod cells;

pub(crate) mod context;
pub(crate) mod escape;
pub(crate) mod frame;
pub(crate) mod node;
pub(crate) mod renderer;
pub(crate) mod wrap;

pub use app::{
    Application, ApplicationBuilder, CommittedElement, ControlFlow, CtrlCBehavior, Handle,
    KeyboardProtocol,
};
pub use cells::Cells;
pub use children::{
    AddTo, ChildCollector, ComponentWithSlot, DataChildren, DataHandle, SpliceInto,
};
pub use component::{Column, Component, EventResult, HStack, Tracked, TrackedRef, VStack};
pub use components::canvas::Canvas;
pub use components::markdown::{Markdown, MarkdownState};
pub use components::spinner::{Spinner, SpinnerState};
pub use components::text::{Span, Text, TextChild};
pub use components::view::{Direction, View};
pub use components::viewport::Viewport;
pub use element::{ElementHandle, Elements};

pub use hooks::Hooks;
pub use inline::InlineRenderer;
pub use insets::Insets;
pub use node::{Layout, NodeId, WidthConstraint};
pub use typed_builder::TypedBuilder;

// Re-exports for component props
/// Re-exported from `ratatui_widgets` for use with [`View::border`].
pub use ratatui_widgets::borders::BorderType;

/// Declarative element tree macro.
///
/// Builds an [`Elements`] list from JSX-like syntax. This is the primary
/// way to describe UI trees in eye_declare.
///
/// # Syntax
///
/// ```ignore
/// element! {
///     // Component with props
///     Spinner(label: "Loading...", done: false)
///
///     // Component with children (slot)
///     VStack {
///         "hello"
///     }
///
///     // Key for stable identity across rebuilds
///     Markdown(key: "intro", source: "# Hello".into())
///
///     // String literal shorthand (becomes a Text)
///     "Some plain text"
///
///     // Conditional children
///     #(if state.loading {
///         Spinner(label: "Please wait...")
///     })
///
///     // Loop children
///     #(for item in &state.items {
///         Markdown(key: item.id.clone(), source: item.text.clone())
///     })
///
///     // Splice pre-built Elements
///     #(footer_elements(state))
/// }
/// ```
///
/// The macro returns an [`Elements`] value. View functions typically
/// return this directly:
///
/// ```ignore
/// fn my_view(state: &MyState) -> Elements {
///     element! {
///         Spinner(label: "working...")
///     }
/// }
/// ```
#[cfg(feature = "macros")]
pub use eye_declare_macros::component;
#[cfg(feature = "macros")]
pub use eye_declare_macros::element;
#[cfg(feature = "macros")]
pub use eye_declare_macros::props;
