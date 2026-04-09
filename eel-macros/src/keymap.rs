use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Expr, ExprBlock, Ident, LitStr, Token,
    parse::{Parse, ParseStream, Result},
};

fn eel_path() -> TokenStream2 {
    match crate_name("eel") {
        Ok(FoundCrate::Itself) => quote! { crate },
        Ok(FoundCrate::Name(name)) => {
            let ident = Ident::new(&name, Span::call_site());
            quote! { ::#ident }
        }
        Err(_) => quote! { ::eel },
    }
}

/// A single binding arm: `"seq" => ACTION`
///
/// ACTION is either a bare braced block `{ ... }` or an arbitrary expression.
enum ActionKind {
    Block(ExprBlock),
    Expr(Expr),
}

struct Binding {
    seq: LitStr,
    action: ActionKind,
}

impl Parse for Binding {
    fn parse(input: ParseStream) -> Result<Self> {
        let seq: LitStr = input.parse()?;
        input.parse::<Token![=>]>()?;

        // Peek: if the next token is a `{`, parse as a braced block expression.
        // We need to distinguish a bare block `{ stmt; Ok(()) }` from a struct
        // literal or a closure. `syn::ExprBlock` handles this correctly.
        let action = if input.peek(syn::token::Brace) {
            ActionKind::Block(input.parse::<ExprBlock>()?)
        } else {
            ActionKind::Expr(input.parse::<Expr>()?)
        };

        Ok(Binding { seq, action })
    }
}

struct KeymapInput {
    editor_name: Option<Ident>,
    bindings: Vec<Binding>,
}

impl Parse for KeymapInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut editor_name = None;
        let mut bindings = Vec::new();

        while !input.is_empty() {
            // Peek for `editor: IDENT` header (keyword `editor` followed by `:`)
            if input.peek(Ident) {
                let fork = input.fork();
                let kw: Ident = fork.parse()?;
                if kw == "editor" && fork.peek(Token![:]) {
                    // Consume from real stream
                    input.parse::<Ident>()?;
                    input.parse::<Token![:]>()?;
                    let name: Ident = input.parse()?;
                    editor_name = Some(name);
                    // Optional trailing comma
                    let _ = input.parse::<Token![,]>();
                    continue;
                }
            }

            // Parse binding
            let binding: Binding = input.parse()?;
            bindings.push(binding);
            let _ = input.parse::<Token![,]>();
        }

        Ok(KeymapInput {
            editor_name,
            bindings,
        })
    }
}

pub fn keymap(input: TokenStream) -> TokenStream {
    let KeymapInput {
        editor_name,
        bindings,
    } = syn::parse_macro_input!(input as KeymapInput);

    let eel = eel_path();

    let mut binding_stmts = Vec::new();

    for Binding { seq, action } in bindings {
        let action_tokens = match action {
            ActionKind::Block(block) => {
                let editor_pat = match &editor_name {
                    Some(n) => quote! { #n },
                    None => quote! { _ },
                };
                // Use `bind()` which takes `impl KeyAction<E>` directly, constraining
                // `_m` to `KeyMapping<E, Arc<dyn KeyAction<E>>>` and allowing coercion.
                binding_stmts.push(quote! {
                    _m.bind(
                        &#eel::keymap::key::parse_key_sequence(#seq)
                            .expect("invalid key sequence in keymap! macro"),
                        move |#editor_pat: &_| #block,
                    );
                });
                continue;
            }
            ActionKind::Expr(expr) => quote! { #expr },
        };

        binding_stmts.push(quote! {
            _m.add_binding(
                &#eel::keymap::key::parse_key_sequence(#seq)
                    .expect("invalid key sequence in keymap! macro"),
                #action_tokens,
            );
        });
    }

    quote! {{
        let mut _m = #eel::keymap::KeyMapping::new();
        #( #binding_stmts )*
        _m
    }}
    .into()
}
