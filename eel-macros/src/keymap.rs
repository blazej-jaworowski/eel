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
    pub(crate) fn emit(&self) -> syn::Result<TokenStream2> {
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
            let kp_tokens = parse_and_emit_sequence(&eel, seq)?;
            let seq_expr = quote! { &[#(#kp_tokens,)*] };

            match action {
                ActionKind::Block(block) => {
                    // Wrap the move closure in a block so that `pre_closure` tokens
                    // (e.g. variable clones) are evaluated outside the move boundary.
                    binding_stmts.push(quote! {
                        _m.bind(
                            #seq_expr,
                            { #pre_closure move |#editor_pat: &_| #block },
                        );
                    });
                }
                ActionKind::Expr(expr) => {
                    binding_stmts.push(quote! {
                        _m.add_binding(#seq_expr, #expr);
                    });
                }
            }
        }

        Ok(quote! {{
            let mut _m = #eel::keymap::KeyMapping::new();
            #( #binding_stmts )*
            _m
        }})
    }
}

fn parse_and_emit_sequence(eel: &TokenStream2, lit: &LitStr) -> syn::Result<Vec<TokenStream2>> {
    let key_presses = eel_key_parse::parse_key_sequence(&lit.value())
        .map_err(|e| syn::Error::new(lit.span(), format!("invalid key sequence: {e}")))?;
    Ok(key_presses
        .iter()
        .map(|kp| emit_key_press(eel, kp))
        .collect())
}

fn emit_key_press(eel: &TokenStream2, kp: &eel_key_parse::KeyPress) -> TokenStream2 {
    let key = emit_key(eel, &kp.key);
    let ctrl = kp.modifiers.ctrl;
    let shift = kp.modifiers.shift;
    quote! {
        #eel::keymap::key::KeyPress::new(
            #key,
            #eel::keymap::key::Modifiers { ctrl: #ctrl, shift: #shift },
        )
    }
}

fn emit_key(eel: &TokenStream2, key: &eel_key_parse::Key) -> TokenStream2 {
    match key {
        eel_key_parse::Key::Char(c) => quote! { #eel::keymap::key::Key::Char(#c) },
        eel_key_parse::Key::Special(s) => {
            let special = emit_special_key(eel, s);
            quote! { #eel::keymap::key::Key::Special(#special) }
        }
    }
}

fn emit_special_key(eel: &TokenStream2, sk: &eel_key_parse::SpecialKey) -> TokenStream2 {
    let p = quote! { #eel::keymap::key::SpecialKey };
    match sk {
        eel_key_parse::SpecialKey::F(n) => quote! { #p::F(#n) },
        eel_key_parse::SpecialKey::Unknown(s) => quote! { #p::Unknown(#s.to_string()) },
        _ => {
            let name: syn::Ident = syn::parse_str(&sk.to_string()).unwrap();
            quote! { #p::#name }
        }
    }
}

pub fn keymap(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as KeymapInput);
    input
        .emit()
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

pub fn key(input: TokenStream) -> TokenStream {
    let lit = syn::parse_macro_input!(input as LitStr);
    let eel = crate::eel_path();
    let kp_tokens = match parse_and_emit_sequence(&eel, &lit) {
        Ok(v) => v,
        Err(e) => return e.into_compile_error().into(),
    };
    match kp_tokens.as_slice() {
        [kp] => kp.clone().into(),
        [] => syn::Error::new(
            lit.span(),
            "expected exactly one key press, got empty string",
        )
        .into_compile_error()
        .into(),
        _ => syn::Error::new(
            lit.span(),
            format!("expected exactly one key press, got {}", kp_tokens.len()),
        )
        .into_compile_error()
        .into(),
    }
}

pub fn keys(input: TokenStream) -> TokenStream {
    let lit = syn::parse_macro_input!(input as LitStr);
    let eel = crate::eel_path();
    let kp_tokens = match parse_and_emit_sequence(&eel, &lit) {
        Ok(v) => v,
        Err(e) => return e.into_compile_error().into(),
    };
    quote! { &[#(#kp_tokens,)*] }.into()
}
