use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, Field, Fields, Meta, parse2};

/// Implementation of the `#[props]` attribute macro.
///
/// Translates `#[default(expr)]` on fields to `#[builder(default = expr, setter(into))]`
/// and adds `#[derive(typed_builder::TypedBuilder)]` to the struct.
///
/// Fields without `#[default]` are required — the builder won't compile
/// without them being set. Fields with `#[default(expr)]` are optional.
pub fn props_impl(input: TokenStream) -> syn::Result<TokenStream> {
    let mut item: DeriveInput = parse2(input)?;

    let fields = match &mut item.data {
        syn::Data::Struct(data) => match &mut data.fields {
            Fields::Named(fields) => &mut fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &item.ident,
                    "#[props] only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &item.ident,
                "#[props] can only be applied to structs",
            ));
        }
    };

    // Transform field attributes: #[default(expr)] → #[builder(default = expr, setter(into))]
    // Fields without #[default] get #[builder(setter(into))] (required)
    for field in fields.iter_mut() {
        let default_expr = extract_and_remove_default_attr(field)?;

        match default_expr {
            Some(expr) => {
                // Optional field with default
                field.attrs.push(syn::parse_quote! {
                    #[builder(default = #expr, setter(into))]
                });
            }
            None => {
                // Required field
                field.attrs.push(syn::parse_quote! {
                    #[builder(setter(into))]
                });
            }
        }
    }

    // Add #[derive(::eye_declare::TypedBuilder)] to the struct,
    // using the re-export so downstream crates don't need typed_builder
    // as a direct dependency.
    item.attrs.push(syn::parse_quote! {
        #[derive(::eye_declare::TypedBuilder)]
    });

    Ok(quote! { #item })
}

/// Extract and remove a `#[default(expr)]` attribute from a field.
/// Returns `Some(expr)` if found, `None` otherwise.
fn extract_and_remove_default_attr(field: &mut Field) -> syn::Result<Option<Expr>> {
    let mut default_expr = None;

    for attr in field.attrs.iter() {
        if attr.path().is_ident("default") {
            if default_expr.is_some() {
                return Err(syn::Error::new_spanned(
                    attr,
                    "duplicate #[default] attribute",
                ));
            }

            let expr: Expr = match &attr.meta {
                Meta::List(list) => syn::parse2(list.tokens.clone())?,
                Meta::Path(_) => {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "#[default] requires a value: #[default(expr)]",
                    ));
                }
                Meta::NameValue(_) => {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "use #[default(expr)] not #[default = expr]",
                    ));
                }
            };

            default_expr = Some(expr);
        }
    }

    // Strip #[default] attributes
    if default_expr.is_some() {
        field.attrs.retain(|a| !a.path().is_ident("default"));
    }

    Ok(default_expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_typed_builder_derive() {
        let input = quote! {
            struct MyProps {
                pub title: String,
            }
        };
        let result = props_impl(input).expect("macro should succeed");
        let output = result.to_string();
        assert!(
            output.contains("TypedBuilder"),
            "should have TypedBuilder derive: {}",
            output
        );
    }

    #[test]
    fn required_field_gets_setter_into() {
        let input = quote! {
            struct MyProps {
                pub title: String,
            }
        };
        let result = props_impl(input).expect("macro should succeed");
        let output = result.to_string();
        // proc_macro2 adds spaces: "setter (into)"
        assert!(
            output.contains("setter"),
            "required field should have setter(into): {}",
            output
        );
    }

    #[test]
    fn default_field_gets_builder_default() {
        let input = quote! {
            struct MyProps {
                #[default(true)]
                pub visible: bool,
            }
        };
        let result = props_impl(input).expect("macro should succeed");
        let output = result.to_string();
        assert!(
            output.contains("default"),
            "optional field should have builder default: {}",
            output
        );
        // #[default(true)] should be stripped, replaced with #[builder(...)]
        assert!(
            !output.contains("# [default (true)]"),
            "original #[default] should be stripped: {}",
            output
        );
    }

    #[test]
    fn rejects_enum() {
        let input = quote! {
            enum Bad { A, B }
        };
        assert!(props_impl(input).is_err());
    }

    #[test]
    fn rejects_tuple_struct() {
        let input = quote! {
            struct Bad(u32, String);
        };
        assert!(props_impl(input).is_err());
    }
}
