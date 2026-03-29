use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, ItemFn, Token, parse2};

/// Parsed arguments from `#[component(props = T, state = S, children = C)]`.
struct ComponentArgs {
    props: Ident,
    state: Option<Ident>,
    children: Option<Ident>,
}

impl syn::parse::Parse for ComponentArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut props = None;
        let mut state = None;
        let mut children = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: Ident = input.parse()?;

            match key.to_string().as_str() {
                "props" => props = Some(value),
                "state" => state = Some(value),
                "children" => children = Some(value),
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

        Ok(ComponentArgs {
            props,
            state,
            children,
        })
    }
}

/// Implementation of the `#[component]` attribute macro.
///
/// Takes a function definition and generates:
/// 1. The original function (kept as-is)
/// 2. `impl Component for PropsType` with lifecycle() and view()
/// 3. `impl_slot_children!` if children = Elements
/// 4. `ChildCollector` with `DataChildren<T>` if children = other type
pub fn component_impl(attr: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let args: ComponentArgs = parse2(attr)?;
    let func: ItemFn = parse2(input)?;

    let func_name = &func.sig.ident;
    let props_type = &args.props;

    // State type: defaults to () if not specified
    let state_type = args
        .state
        .as_ref()
        .map(|s| quote! { #s })
        .unwrap_or_else(|| quote! { () });

    // Determine which parameters the function expects by checking the arg count.
    // The function can have 1-4 parameters in this order:
    //   props: &PropsType
    //   state: &StateType        (if state specified)
    //   hooks: &mut Hooks<State> (optional)
    //   children: Elements       (if children specified)
    //
    // We generate the call sites based on what was declared in the attribute.
    let has_state = args.state.is_some();
    let has_children = args.children.is_some();

    // Detect if function has a hooks parameter by checking arg count.
    // Min args: props (1). Max: props + state + hooks + children (4).
    let param_count = func.sig.inputs.len();
    let expected_min = 1 + has_state as usize + has_children as usize;
    let expected_max = expected_min + 1; // +1 for optional hooks
    let has_hooks = param_count == expected_max;

    if param_count < expected_min || param_count > expected_max {
        return Err(syn::Error::new_spanned(
            &func.sig,
            format!("expected {expected_min}-{expected_max} parameters, found {param_count}"),
        ));
    }

    // Build the call arguments for lifecycle() — hooks are real, children are empty
    let lifecycle_call = {
        let mut call_args = vec![quote! { self }];
        if has_state {
            call_args.push(quote! { __state });
        }
        if has_hooks {
            call_args.push(quote! { __hooks });
        }
        if has_children {
            call_args.push(quote! { ::eye_declare::Elements::new() });
        }
        quote! { #func_name(#(#call_args),*) }
    };

    // Build the call arguments for view() — hooks are discarded, children are real
    let view_call = {
        let mut call_args = vec![quote! { self }];
        if has_state {
            call_args.push(quote! { __state });
        }
        if has_hooks {
            call_args.push(quote! { &mut ::eye_declare::Hooks::new() });
        }
        if has_children {
            call_args.push(quote! { __children });
        }
        quote! { #func_name(#(#call_args),*) }
    };

    // Generate lifecycle() only if hooks are used
    let lifecycle_impl = if has_hooks {
        quote! {
            fn lifecycle(
                &self,
                __hooks: &mut ::eye_declare::Hooks<Self::State>,
                __state: &Self::State,
            ) {
                let _ = #lifecycle_call;
            }
        }
    } else {
        quote! {}
    };

    // Generate view()
    let view_impl = quote! {
        fn view(
            &self,
            __state: &Self::State,
            __children: ::eye_declare::Elements,
        ) -> ::eye_declare::Elements {
            #view_call
        }
    };

    // Generate ChildCollector
    let child_collector = match &args.children {
        Some(child_type) if child_type == "Elements" => {
            // Slot children
            quote! {
                ::eye_declare::impl_slot_children!(#props_type);
            }
        }
        Some(child_type) => {
            // Data children
            quote! {
                impl ::eye_declare::ChildCollector for #props_type {
                    type Collector = ::eye_declare::DataChildren<#child_type>;
                    type Output = #props_type;

                    fn finish(self, _collector: ::eye_declare::DataChildren<#child_type>) -> #props_type {
                        self
                    }
                }
            }
        }
        None => quote! {},
    };

    Ok(quote! {
        #func

        impl ::eye_declare::Component for #props_type {
            type State = #state_type;

            #lifecycle_impl
            #view_impl
        }

        #child_collector
    })
}
