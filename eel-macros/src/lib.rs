#[cfg(any(feature = "keymap", feature = "modal"))]
use proc_macro::TokenStream;

#[cfg(feature = "keymap")]
mod keymap;
#[cfg(feature = "modal")]
mod modal_keymap;

/// Returns the path to the `eel` crate, handling re-exports.
#[cfg(any(feature = "keymap", feature = "modal"))]
pub(crate) fn eel_path() -> proc_macro2::TokenStream {
    use proc_macro_crate::{FoundCrate, crate_name};
    use proc_macro2::Span;
    use quote::quote;
    use syn::Ident;

    match crate_name("eel") {
        Ok(FoundCrate::Itself) => quote! { crate },
        Ok(FoundCrate::Name(name)) => {
            let ident = Ident::new(&name, Span::call_site());
            quote! { ::#ident }
        }
        Err(_) => quote! { ::eel },
    }
}

#[cfg(feature = "keymap")]
#[proc_macro]
pub fn keymap(input: TokenStream) -> TokenStream {
    keymap::keymap(input)
}

#[cfg(feature = "modal")]
#[proc_macro]
pub fn modal_keymap(input: TokenStream) -> TokenStream {
    modal_keymap::modal_keymap(input)
}
