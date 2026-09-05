//! Runtime attribute macros for geario.
use proc_macro::TokenStream;
use quote::quote;

mod sys;

/// Marks async function to be executed by the geario system.
///
/// ## Usage
///
/// ```rust
/// #[geario::main]
/// async fn main() {
///     println!("Hello world");
/// }
/// ```
///
/// ## Attributes
///
/// - `name = "..."` - Sets system name.
/// - `signals = true/false` - Enable/disable signals handling.
/// - `panic_handling = true/false` - Enable/disable panic handling.
/// - `ping_interval = N` - Sets arbiter ping interval in milliseconds for the created system.
///   To disable pings set value to zero.
/// - `rt = ..` - Sets system runtime type, it must implements Runner trait
#[proc_macro_attribute]
pub fn rt_main(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut args = syn::parse_macro_input!(args as sys::MainArgs);
    let mut input = syn::parse_macro_input!(item as syn::ItemFn);
    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &mut input.sig;
    let body = &input.block;
    let name = &sig.ident;

    if sig.asyncness.is_none() {
        return syn::Error::new_spanned(sig.fn_token, "only async fn is supported")
            .to_compile_error()
            .into();
    }

    sig.asyncness = None;

    let runner = args.gen_sys_rt();
    let config = args.gen_sys_config(name);

    (quote! {
        #(#attrs)*
        #vis #sig {
            geario::rt::System::build()
                #config
                .build( #runner )
                .block_on(async move { #body })
        }
    })
    .into()
}

/// Marks async test function to be executed by the geario runtime.
///
/// ## Usage
///
/// ```no_run
/// #[geario::test]
/// async fn my_test() {
///     assert!(true);
/// }
/// ```
#[proc_macro_attribute]
pub fn rt_test(_: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemFn);

    let ret = &input.sig.output;
    let name = &input.sig.ident;
    let body = &input.block;
    let attrs = &input.attrs;
    let mut has_test_attr = false;

    for attr in attrs {
        if attr.path().is_ident("test") {
            has_test_attr = true;
        }
    }

    if input.sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            input.sig.fn_token,
            format!("only async fn is supported, {}", input.sig.ident),
        )
        .to_compile_error()
        .into();
    }

    let result = if has_test_attr {
        quote! {
            #(#attrs)*
            fn #name() #ret {
                geario::util::enable_test_logging();
                geario::rt::System::build()
                    .name(stringify!(#name))
                    .testing()
                    .build(geario::rt::DefaultRuntime)
                    .block_on(async { #body })
            }
        }
    } else {
        quote! {
            #[test]
            #(#attrs)*
            fn #name() #ret {
                geario::util::enable_test_logging();
                geario::rt::System::build()
                    .name(stringify!(#name))
                    .testing()
                    .build(geario::rt::DefaultRuntime)
                    .block_on(async { #body })
            }
        }
    };

    result.into()
}
