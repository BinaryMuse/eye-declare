use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, ItemFn, Token, parse2};

fn is_hooks_type(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(type_ref) = ty
        && type_ref.mutability.is_some()
        && let syn::Type::Path(type_path) = type_ref.elem.as_ref()
    {
        type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Hooks")
    } else {
        false
    }
}

struct ComponentArgs {
    props: Ident,
    state: Option<Ident>,
    children: Option<Ident>,
    initial_state: Option<syn::Expr>,
    crate_path: Option<TokenStream>,
}

impl syn::parse::Parse for ComponentArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut props = None;
        let mut state = None;
        let mut children = None;
        let mut initial_state = None;
        let mut initial_state_key_span: Option<proc_macro2::Span> = None;
        let mut crate_path = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "initial_state" => {
                    initial_state_key_span = Some(key.span());
                    let expr: syn::Expr = input.parse()?;
                    initial_state = Some(expr);
                }
                "props" => {
                    let value: Ident = input.parse()?;
                    props = Some(value);
                }
                "state" => {
                    let value: Ident = input.parse()?;
                    state = Some(value);
                }
                "children" => {
                    let value: Ident = input.parse()?;
                    children = Some(value);
                }
                "crate_path" => {
                    let value: TokenStream = input.parse()?;
                    crate_path = Some(value);
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        key,
                        format!("unknown component attribute: `{other}`"),
                    ));
                }
            }

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        let props = props.ok_or_else(|| input.error("#[component] requires `props = Type`"))?;

        if let Some(span) = initial_state_key_span
            && state.is_none()
        {
            return Err(syn::Error::new(
                span,
                "#[component] `initial_state` requires `state` to also be specified",
            ));
        }

        Ok(ComponentArgs {
            props,
            state,
            children,
            initial_state,
            crate_path,
        })
    }
}

pub fn component_impl(attr: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let args: ComponentArgs = parse2(attr)?;
    let func: ItemFn = parse2(input)?;

    let func_name = &func.sig.ident;
    let props_type = &args.props;

    let crate_path = args
        .crate_path
        .as_ref()
        .map(|p| quote! { #p })
        .unwrap_or_else(|| quote! { ::eye_declare });

    let state_type = args
        .state
        .as_ref()
        .map(|s| quote! { #s })
        .unwrap_or_else(|| quote! { () });

    let has_state = args.state.is_some();
    let has_children = args.children.is_some();

    let param_count = func.sig.inputs.len();

    let has_hooks = func.sig.inputs.iter().any(|arg| {
        if let syn::FnArg::Typed(pat_type) = arg {
            is_hooks_type(&pat_type.ty)
        } else {
            false
        }
    });

    let expected = 1 + has_state as usize + has_hooks as usize + has_children as usize;
    if param_count != expected {
        let mut expected_params = vec!["props"];
        if has_state {
            expected_params.push("state");
        }
        if has_hooks {
            expected_params.push("hooks");
        }
        if has_children {
            expected_params.push("children");
        }
        return Err(syn::Error::new_spanned(
            &func.sig,
            format!(
                "expected {} parameters ({}), found {param_count}",
                expected,
                expected_params.join(", "),
            ),
        ));
    }

    let param_names: Vec<String> = func
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(pat_type) = arg
                && let syn::Pat::Ident(ident) = pat_type.pat.as_ref()
            {
                return Some(ident.ident.to_string());
            }
            None
        })
        .collect();
    let has_children_param = param_names.iter().any(|n| n == "children");
    if has_children && !has_children_param {
        return Err(syn::Error::new_spanned(
            &func.sig,
            "attribute declares `children` but function has no `children` parameter",
        ));
    }
    if !has_children && has_children_param {
        return Err(syn::Error::new_spanned(
            &func.sig,
            "function has `children` parameter but attribute doesn't declare `children = Type`",
        ));
    }

    let lifecycle_call = {
        let mut call_args = vec![quote! { self }];
        if has_state {
            call_args.push(quote! { __state });
        }
        if has_hooks {
            call_args.push(quote! { __hooks });
        }
        if has_children {
            call_args.push(quote! { #crate_path::Elements::new() });
        }
        quote! { #func_name(#(#call_args),*) }
    };

    let view_call = {
        let mut call_args = vec![quote! { self }];
        if has_state {
            call_args.push(quote! { __state });
        }
        if has_hooks {
            call_args.push(quote! { &mut #crate_path::Hooks::new() });
        }
        if has_children {
            call_args.push(quote! { __children });
        }
        quote! { #func_name(#(#call_args),*) }
    };

    let initial_state_impl = match &args.initial_state {
        Some(expr) => quote! {
            fn initial_state(&self) -> Option<#state_type> {
                Some(#expr)
            }
        },
        None => quote! {},
    };

    let lifecycle_impl = if has_hooks {
        quote! {
            fn lifecycle(
                &self,
                __hooks: &mut #crate_path::Hooks<Self::State>,
                __state: &Self::State,
            ) {
                let _ = #lifecycle_call;
            }
        }
    } else {
        quote! {}
    };

    let view_impl = quote! {
        fn view(
            &self,
            __state: &Self::State,
            __children: #crate_path::Elements,
        ) -> #crate_path::Elements {
            #view_call
        }
    };

    let child_collector = match &args.children {
        Some(child_type) if child_type == "Elements" => {
            quote! {
                #crate_path::impl_slot_children!(#props_type);
            }
        }
        Some(child_type) => {
            return Err(syn::Error::new_spanned(
                child_type,
                format!(
                    "only `children = Elements` is currently supported; \
                     data children (`children = {child_type}`) require additional design"
                ),
            ));
        }
        None => quote! {},
    };

    Ok(quote! {
        #func

        impl #crate_path::Component for #props_type {
            type State = #state_type;

            #initial_state_impl
            #lifecycle_impl
            #view_impl
        }

        #child_collector
    })
}
