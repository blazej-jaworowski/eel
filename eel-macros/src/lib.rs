#[cfg(any(feature = "keymap", feature = "modal"))]
use proc_macro::TokenStream;

#[cfg(feature = "keymap")]
mod keymap;
#[cfg(feature = "modal")]
mod modal_keymap;

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
