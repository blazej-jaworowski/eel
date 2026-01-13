use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Expr, Ident, ItemFn, parse_macro_input, spanned::Spanned};

#[derive(deluxe::ParseMetaItem)]
#[deluxe(attributes(nvim_test))]
struct NvimTestArgs {
    editor_factory: Expr,
}

#[proc_macro_attribute]
pub fn nvim_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut function = parse_macro_input!(item as ItemFn);

    let editor_factory = {
        let args: NvimTestArgs = match deluxe::parse(attr) {
            Ok(args) => args,
            Err(e) => return e.into_compile_error().into(),
        };
        args.editor_factory
    };

    // Identifier of nvim_oxi test function
    let test_ident = Ident::new(&function.sig.ident.to_string(), Span::call_site());

    // Modifying identifier of the original function to avoid duplicate
    let new_ident = Ident::new(&format!("_{}", function.sig.ident), function.sig.span());
    function.sig.ident = new_ident.clone();

    let return_type = function.sig.output.clone();

    quote! {
        #function

        #[::nvim_oxi::test]
        fn #test_ident() #return_type {
            let editor_factory = #editor_factory;
            crate::test_utils::run_nvim_async_test(#new_ident, editor_factory)
        }
    }
    .into()
}
