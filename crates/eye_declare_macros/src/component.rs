use proc_macro2::TokenStream;
use quote::{format_ident, quote};
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

/// Check whether a type is `Elements` (matches the last path segment).
fn is_elements_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Elements" && seg.arguments.is_empty())
    } else {
        false
    }
}

struct ComponentArgs {
    props: Ident,
    state: Option<Ident>,
    children: Option<syn::Type>,
    initial_state: Option<syn::Expr>,
    crate_path: Option<syn::Path>,
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
                    let value: syn::Type = input.parse()?;
                    children = Some(value);
                }
                "crate_path" => {
                    let value: syn::Path = input.parse()?;
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
    let is_slot_children = args.children.as_ref().is_some_and(is_elements_type);

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

    let initial_state_impl = match &args.initial_state {
        Some(expr) => quote! {
            fn initial_state(&self) -> Option<#state_type> {
                Some(#expr)
            }
        },
        None => quote! {},
    };

    if has_children && !is_slot_children {
        // --- Data children path ---
        generate_data_children(
            &func,
            func_name,
            props_type,
            &crate_path,
            &state_type,
            has_state,
            has_hooks,
            &initial_state_impl,
            args.children.as_ref().unwrap(),
        )
    } else {
        // --- Slot children (Elements) or no children ---
        generate_slot_or_none(
            &func,
            func_name,
            props_type,
            &crate_path,
            &state_type,
            has_state,
            has_hooks,
            has_children,
            &initial_state_impl,
        )
    }
}

/// Generate code for components with slot children (Elements) or no children.
#[allow(clippy::too_many_arguments)]
fn generate_slot_or_none(
    func: &ItemFn,
    func_name: &Ident,
    props_type: &Ident,
    crate_path: &TokenStream,
    state_type: &TokenStream,
    has_state: bool,
    has_hooks: bool,
    has_children: bool,
    initial_state_impl: &TokenStream,
) -> syn::Result<TokenStream> {
    let update_call = {
        let mut call_args = vec![quote! { self }];
        if has_state {
            call_args.push(quote! { __state });
        }
        if has_hooks {
            call_args.push(quote! { __hooks });
        }
        if has_children {
            call_args.push(quote! { __children });
        }
        quote! { #func_name(#(#call_args),*) }
    };

    let update_impl = quote! {
        fn update(
            &self,
            __hooks: &mut #crate_path::Hooks<Self::State>,
            __state: &Self::State,
            __children: #crate_path::Elements,
        ) -> #crate_path::Elements {
            #update_call
        }
    };

    let child_collector = if has_children {
        // Slot children: generate ChildCollector inline (replaces impl_slot_children!)
        quote! {
            impl #crate_path::ChildCollector for #props_type {
                type Collector = #crate_path::Elements;
                type Output = #crate_path::ComponentWithSlot<#props_type>;

                fn finish(self, collector: #crate_path::Elements) -> #crate_path::ComponentWithSlot<#props_type> {
                    #crate_path::ComponentWithSlot::new(self, collector)
                }
            }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #func

        impl #crate_path::Component for #props_type {
            type State = #state_type;

            #initial_state_impl
            #update_impl
        }

        #child_collector
    })
}

/// Generate code for components with data children (non-Elements).
///
/// Produces:
/// 1. A hidden wrapper struct holding props + collected data
/// 2. Component impl on props type (for no-children usage, with empty data)
/// 3. Component impl on wrapper (for with-children usage, with real data)
/// 4. ChildCollector impl on props type → output is the wrapper
#[allow(clippy::too_many_arguments)]
fn generate_data_children(
    func: &ItemFn,
    func_name: &Ident,
    props_type: &Ident,
    crate_path: &TokenStream,
    state_type: &TokenStream,
    has_state: bool,
    has_hooks: bool,
    initial_state_impl: &TokenStream,
    children_type: &syn::Type,
) -> syn::Result<TokenStream> {
    let wrapper_name = format_ident!("__{props_type}WithData");

    // Build update call for the props-type impl (no data children).
    // The function receives a reference to default (empty) data.
    let props_update_call = {
        let mut call_args = vec![quote! { self }];
        if has_state {
            call_args.push(quote! { __state });
        }
        if has_hooks {
            call_args.push(quote! { __hooks });
        }
        call_args.push(quote! { &__default_data });
        quote! { #func_name(#(#call_args),*) }
    };

    // Build update call for the wrapper impl (with data children).
    // Props come from self.__props, data from self.__data.
    let wrapper_update_call = {
        let mut call_args = vec![quote! { &self.__props }];
        if has_state {
            call_args.push(quote! { __state });
        }
        if has_hooks {
            call_args.push(quote! { __hooks });
        }
        call_args.push(quote! { &self.__data });
        quote! { #func_name(#(#call_args),*) }
    };

    Ok(quote! {
        #func

        // Component impl on props type: for usage without data children.
        // Passes default (empty) data to the function.
        impl #crate_path::Component for #props_type {
            type State = #state_type;

            #initial_state_impl

            fn update(
                &self,
                __hooks: &mut #crate_path::Hooks<Self::State>,
                __state: &Self::State,
                __children: #crate_path::Elements,
            ) -> #crate_path::Elements {
                let __default_data = <#children_type as Default>::default();
                #props_update_call
            }
        }

        // Hidden wrapper: props + collected data children.
        #[doc(hidden)]
        pub struct #wrapper_name {
            __props: #props_type,
            __data: #children_type,
        }

        // Component impl on wrapper: for usage with data children.
        impl #crate_path::Component for #wrapper_name {
            type State = #state_type;

            #initial_state_impl

            fn update(
                &self,
                __hooks: &mut #crate_path::Hooks<Self::State>,
                __state: &Self::State,
                __children: #crate_path::Elements,
            ) -> #crate_path::Elements {
                #wrapper_update_call
            }
        }

        // ChildCollector: element! macro uses this when braces are present.
        impl #crate_path::ChildCollector for #props_type {
            type Collector = #children_type;
            type Output = #wrapper_name;

            fn finish(self, collector: #children_type) -> #wrapper_name {
                #wrapper_name {
                    __props: self,
                    __data: collector,
                }
            }
        }
    })
}
