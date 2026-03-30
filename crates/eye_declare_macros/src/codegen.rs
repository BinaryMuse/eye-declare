use proc_macro2::TokenStream;
use quote::quote;

use crate::parse::{Node, Prop};

/// Generate an `Elements` expression from a list of nodes.
pub fn generate_elements(nodes: &[Node]) -> TokenStream {
    let body = generate_nodes(nodes);
    quote! {{
        let mut __result = ::eye_declare::Elements::new();
        let __els = &mut __result;
        #body
        __result
    }}
}

/// Generate code that adds nodes to the current collector (`__els`).
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
            // String literal sugar: dispatches through AddTo.
            // In an Elements context → creates a Text component (via AddTo<Elements> for String).
            // In a DataChildren context → adds as a data child (via Into<T> for String).
            quote! {
                ::eye_declare::AddTo::add_to(String::from(#lit), __els);
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

        Node::Splice(expr) => {
            quote! {
                ::eye_declare::SpliceInto::splice_into(#expr, __els);
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

    // Construct the component via builder pattern.
    // The type must provide Type::builder() returning a builder with
    // setter methods for each field and a .build() finalizer.
    let setters: Vec<TokenStream> = field_props
        .iter()
        .map(|p| {
            let name = &p.name;
            let value = &p.value;
            quote! { .#name(#value) }
        })
        .collect();
    let construct = quote! {
        #type_name::builder() #(#setters)* .build()
    };

    // Apply special props
    let key_call = key_expr.map(|k| quote! { .key(#k) });

    // Add to collector (with or without children)
    match children {
        Some(child_nodes) => {
            let children_code = generate_nodes(child_nodes);
            quote! {
                {
                    let mut __collector =
                        <<#type_name as ::eye_declare::ChildCollector>::Collector>::default();
                    {
                        let __els = &mut __collector;
                        #children_code
                    }
                    let __output =
                        <#type_name as ::eye_declare::ChildCollector>::finish(
                            #construct,
                            __collector,
                        );
                    ::eye_declare::AddTo::add_to(__output, __els) #key_call;
                }
            }
        }
        None => {
            quote! {
                {
                    ::eye_declare::AddTo::add_to(#construct, __els) #key_call;
                }
            }
        }
    }
}
