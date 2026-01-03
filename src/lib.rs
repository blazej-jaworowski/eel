use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, ItemFn, parse_macro_input};

#[proc_macro_attribute]
pub fn nvim_test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut function = parse_macro_input!(item as ItemFn);

    let mut orig_ident = function.sig.ident;
    let new_ident = Ident::new(&format!("_{orig_ident}"), orig_ident.span());

    function.sig.ident = new_ident.clone();

    let return_type = function.sig.output.clone();

    orig_ident.set_span(Span::call_site());

    quote! {
        #function

        #[::nvim_oxi::test]
        fn #orig_ident() #return_type {
            run_nvim_async_test(|editor| #new_ident(editor))
        }
    }
    .into()
}
