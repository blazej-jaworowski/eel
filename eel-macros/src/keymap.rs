use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Expr, ExprBlock, Ident, LitStr, Token,
    parse::{Parse, ParseStream, Result},
};

/// A single binding arm: `"seq" => ACTION`
///
/// ACTION is either a bare braced block `{ ... }` or an arbitrary expression.
pub(crate) enum ActionKind {
    Block(ExprBlock),
    Expr(Expr),
}

pub(crate) struct Binding {
    pub(crate) seq: LitStr,
    pub(crate) action: ActionKind,
    /// Tokens emitted in a wrapping block immediately before the `move` closure,
    /// allowing callers to clone captured variables without moving them.
    pub(crate) pre_closure: TokenStream2,
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

        Ok(Binding {
            seq,
            action,
            pre_closure: TokenStream2::new(),
        })
    }
}

pub(crate) struct KeymapInput {
    pub(crate) editor_name: Option<Ident>,
    pub(crate) bindings: Vec<Binding>,
}

impl Parse for KeymapInput {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse optional `editor: IDENT` header — only allowed at the top,
        // before any bindings.
        let editor_name = if input.peek(Ident) {
            let fork = input.fork();
            let kw: Ident = fork.parse()?;
            if kw == "editor" && fork.peek(Token![:]) {
                input.parse::<Ident>()?;
                input.parse::<Token![:]>()?;
                let name: Ident = input.parse()?;
                let _ = input.parse::<Token![,]>();
                Some(name)
            } else {
                None
            }
        } else {
            None
        };

        let mut bindings = Vec::new();
        while !input.is_empty() {
            bindings.push(input.parse::<Binding>()?);
            let _ = input.parse::<Token![,]>();
        }

        Ok(KeymapInput {
            editor_name,
            bindings,
        })
    }
}

impl KeymapInput {
    pub(crate) fn emit(&self) -> TokenStream2 {
        let eel = crate::eel_path();
        let editor_pat = match &self.editor_name {
            Some(n) => quote! { #n },
            None => quote! { _ },
        };

        let mut binding_stmts = Vec::new();

        for Binding {
            seq,
            action,
            pre_closure,
        } in &self.bindings
        {
            match action {
                ActionKind::Block(block) => {
                    // Wrap the move closure in a block so that `pre_closure` tokens
                    // (e.g. variable clones) are evaluated outside the move boundary.
                    binding_stmts.push(quote! {
                        _m.bind(
                            &#eel::keymap::key::parse_key_sequence(#seq)
                                .expect("invalid key sequence in keymap! macro"),
                            { #pre_closure move |#editor_pat: &_| #block },
                        );
                    });
                }
                ActionKind::Expr(expr) => {
                    binding_stmts.push(quote! {
                        _m.add_binding(
                            &#eel::keymap::key::parse_key_sequence(#seq)
                                .expect("invalid key sequence in keymap! macro"),
                            #expr,
                        );
                    });
                }
            }
        }

        quote! {{
            let mut _m = #eel::keymap::KeyMapping::new();
            #( #binding_stmts )*
            _m
        }}
    }
}

pub fn keymap(input: TokenStream) -> TokenStream {
    syn::parse_macro_input!(input as KeymapInput).emit().into()
}
