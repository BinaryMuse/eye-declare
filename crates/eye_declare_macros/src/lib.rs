use proc_macro::TokenStream;

/// Declarative element tree macro.
///
/// Builds an `Elements` list from a JSX-like syntax:
///
/// ```ignore
/// element! {
///     VStack {
///         Markdown(key: format!("msg-{i}"), source: msg)
///         #(if state.thinking {
///             Spinner(key: "thinking", label: "Thinking...")
///         })
///         "---"
///     }
/// }
/// ```
///
/// ## Syntax
///
/// - `Component(prop: val, key: "k")` — construct a component with props
/// - `Component { ... }` — component with children (slot)
/// - `Component(props) { children }` — both
/// - `"text"` — shorthand for `TextBlock::new().unstyled("text")`
/// - `#(if cond { ... })` — conditional children
/// - `#(for pat in iter { ... })` — loop children
///
/// `key` is a special prop — it maps to `.key()` on the element handle,
/// not a struct field. It provides stable identity for reconciliation.
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

mod codegen;
mod parse;
