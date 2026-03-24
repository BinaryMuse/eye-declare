use proc_macro2::TokenStream;
use quote::quote;

use crate::parse::{Node, Prop};

/// Generate an `Elements` expression from a list of nodes.
pub fn generate_elements(nodes: &[Node]) -> TokenStream {
    let body = generate_nodes(nodes);
    quote! {{
        let mut __els = ::eye_declare::Elements::new();
        #body
        __els
    }}
}

/// Generate code that adds nodes to a `__els: Elements` variable.
fn generate_nodes(nodes: &[Node]) -> TokenStream {
    let stmts: Vec<TokenStream> = nodes.iter().map(generate_node).collect();
    quote! { #(#stmts)* }
}

fn generate_node(node: &Node) -> TokenStream {
    match node {
        Node::Component {
            type_name,
            props,
            children,
        } => generate_component(type_name, props, children.as_deref()),

        Node::Text(lit) => {
            quote! {
                __els.add(::eye_declare::TextBlock::new().unstyled(#lit));
            }
        }

        Node::Conditional {
            condition,
            body,
            else_body,
        } => {
            let body_code = generate_nodes(body);
            if let Some(else_nodes) = else_body {
                let else_code = generate_nodes(else_nodes);
                quote! {
                    if #condition {
                        #body_code
                    } else {
                        #else_code
                    }
                }
            } else {
                quote! {
                    if #condition {
                        #body_code
                    }
                }
            }
        }

        Node::ConditionalLet {
            pattern,
            expr,
            body,
            else_body,
        } => {
            let body_code = generate_nodes(body);
            if let Some(else_nodes) = else_body {
                let else_code = generate_nodes(else_nodes);
                quote! {
                    if let #pattern = #expr {
                        #body_code
                    } else {
                        #else_code
                    }
                }
            } else {
                quote! {
                    if let #pattern = #expr {
                        #body_code
                    }
                }
            }
        }

        Node::Loop {
            pattern,
            iter,
            body,
        } => {
            let body_code = generate_nodes(body);
            quote! {
                for #pattern in #iter {
                    #body_code
                }
            }
        }
    }
}

fn generate_component(
    type_name: &syn::Ident,
    props: &[Prop],
    children: Option<&[Node]>,
) -> TokenStream {
    // Separate special props (key) from struct fields
    let mut key_expr: Option<&syn::Expr> = None;
    let mut field_props: Vec<&Prop> = Vec::new();

    for prop in props {
        let name = prop.name.to_string();
        match name.as_str() {
            "key" => key_expr = Some(&prop.value),
            _ => field_props.push(prop),
        }
    }

    // Construct the component
    let construct = if field_props.is_empty() {
        quote! { #type_name::default() }
    } else {
        let assignments: Vec<TokenStream> = field_props
            .iter()
            .map(|p| {
                let name = &p.name;
                let value = &p.value;
                quote! { __c.#name = (#value).into(); }
            })
            .collect();
        quote! {{
            let mut __c = #type_name::default();
            #(#assignments)*
            __c
        }}
    };

    // Add to elements (with or without children)
    let add_call = match children {
        Some(child_nodes) => {
            let children_code = generate_nodes(child_nodes);
            quote! {
                let mut __children = ::eye_declare::Elements::new();
                // Shadow outer __els so nested components add to __children
                {
                    let __els = &mut __children;
                    #children_code
                }
                __els.add_with_children(#construct, __children)
            }
        }
        None => {
            quote! { __els.add(#construct) }
        }
    };

    // Apply special props
    let key_call = key_expr.map(|k| quote! { .key(#k) });

    quote! {
        {
            #add_call #key_call;
        }
    }
}
