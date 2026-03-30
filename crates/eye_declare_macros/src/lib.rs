use proc_macro::TokenStream;

/// Declarative element tree macro — the primary way to build UIs in eye_declare.
///
/// Returns an `Elements` list from JSX-like syntax. View functions typically
/// return the result directly:
///
/// ```ignore
/// fn my_view(state: &AppState) -> Elements {
///     element! {
///         VStack {
///             Markdown(key: format!("msg-{}", state.id), source: state.text.clone())
///             #(if state.thinking {
///                 Spinner(key: "thinking", label: "Thinking...")
///             })
///             "---"
///         }
///     }
/// }
/// ```
///
/// # Syntax reference
///
/// | Syntax | Description |
/// |--------|-------------|
/// | `Component(prop: val)` | Construct with props (struct field init) |
/// | `Component { ... }` | Component with children (slot or data) |
/// | `Component(props) { children }` | Props and children |
/// | `"text"` | String literal — auto-wrapped as `Text` |
/// | `#(if cond { ... })` | Conditional children |
/// | `#(if let pat = expr { ... })` | Pattern-matching conditional |
/// | `#(for pat in iter { ... })` | Loop children |
/// | `#(expr)` | Splice a pre-built `Elements` value inline |
///
/// # Keys
///
/// `key` is a special prop — it maps to `.key()` on the element handle,
/// not a struct field. Keys provide stable identity for reconciliation:
/// keyed elements survive reordering with their state preserved.
///
/// ```ignore
/// element! {
///     #(for (i, item) in items.iter().enumerate() {
///         Markdown(key: format!("item-{i}"), source: item.clone())
///     })
/// }
/// ```
/// Attribute macro for defining component props with compile-time
/// required field enforcement.
///
/// Generates a [`TypedBuilder`] impl for the struct. Fields without
/// `#[default]` are required — omitting them in `element!` is a compile
/// error. Fields with `#[default(expr)]` are optional. All setters
/// accept `impl Into<T>` for ergonomic value passing.
///
/// # Example
///
/// ```ignore
/// use eye_declare::props;
///
/// #[props]
/// struct CardProps {
///     pub title: String,              // required
///     #[default(true)]
///     pub visible: bool,              // optional, defaults to true
///     pub border: Option<BorderType>, // optional, defaults to None
/// }
///
/// // In element! macro:
/// element! {
///     CardProps(title: "My Card")           // visible & border use defaults
///     CardProps(title: "Hidden", visible: false)  // override default
///     // CardProps(visible: true)           // compile error — title is required
/// }
/// ```
#[proc_macro_attribute]
pub fn props(_attr: TokenStream, input: TokenStream) -> TokenStream {
    match props::props_impl(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro]
pub fn element(input: TokenStream) -> TokenStream {
    match element_impl(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn element_impl(input: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    let nodes = parse::parse_nodes(input)?;
    Ok(codegen::generate_elements(&nodes))
}

/// Attribute macro for defining function components.
///
/// Generates a [`Component`] impl for the props type, mapping the function
/// body to `lifecycle()` (for hooks) and `view()` (for the element tree).
///
/// # Attributes
///
/// - `props = Type` — **required**. The struct that becomes the Component.
/// - `state = Type` — optional. The component's state type. Defaults to `()`.
/// - `children = Elements` — optional. Generates slot children support (`impl_slot_children!`).
///
/// # Example
///
/// ```ignore
/// use eye_declare::{component, props, Component, Elements, Hooks, View, Canvas};
/// use ratatui_widgets::borders::BorderType;
///
/// #[props]
/// struct CardProps {
///     title: String,
///     #[default(true)]
///     visible: bool,
/// }
///
/// #[component(props = CardProps, children = Elements)]
/// fn card(props: &CardProps, children: Elements) -> Elements {
///     if !props.visible {
///         return Elements::new();
///     }
///     let mut els = Elements::new();
///     els.add_with_children(
///         View { border: Some(BorderType::Rounded),
///                title: Some(props.title.clone()),
///                ..View::default() },
///         children,
///     );
///     els
/// }
///
/// // Usage in element! macro:
/// element! {
///     Card(title: "My Card") {
///         "Card content"
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn component(attr: TokenStream, input: TokenStream) -> TokenStream {
    match component::component_impl(attr.into(), input.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

mod codegen;
mod component;
mod parse;
mod props;
