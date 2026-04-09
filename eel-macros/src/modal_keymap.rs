use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::quote;
use syn::{
    Expr, ExprBlock, Ident, LitStr, Token,
    parse::{Parse, ParseStream, Result},
};

/// Returns `true` if `ident` appears anywhere in `block`'s tokens.
fn block_uses_ident(block: &ExprBlock, ident: &Ident) -> bool {
    fn scan(ts: TokenStream2, name: &str) -> bool {
        for tt in ts {
            match tt {
                TokenTree::Ident(i) if i == name => return true,
                TokenTree::Group(g) => {
                    if scan(g.stream(), name) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    scan(quote! { #block }, &ident.to_string())
}

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

        let action = if input.peek(syn::token::Brace) {
            ActionKind::Block(input.parse::<ExprBlock>()?)
        } else {
            ActionKind::Expr(input.parse::<Expr>()?)
        };

        Ok(Binding { seq, action })
    }
}

/// A `[Mode, ...]: { bindings }` group.
struct ModeGroup {
    modes: Vec<Expr>,
    bindings: Vec<Binding>,
}

impl Parse for ModeGroup {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse `[ Mode, Mode, ... ]`
        let content;
        syn::bracketed!(content in input);
        let mut modes = Vec::new();
        while !content.is_empty() {
            modes.push(content.parse::<Expr>()?);
            if !content.is_empty() {
                content.parse::<Token![,]>()?;
            }
        }

        input.parse::<Token![:]>()?;

        // Parse `{ binding, ... }`
        let block_content;
        syn::braced!(block_content in input);
        let mut bindings = Vec::new();
        while !block_content.is_empty() {
            bindings.push(block_content.parse::<Binding>()?);
            let _ = block_content.parse::<Token![,]>();
        }

        Ok(ModeGroup { modes, bindings })
    }
}

struct ModalKeymapInput {
    editor_name: Option<Ident>,
    controller_name: Option<Ident>,
    initial: Option<Expr>,
    groups: Vec<ModeGroup>,
}

impl Parse for ModalKeymapInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut editor_name = None;
        let mut controller_name = None;
        let mut initial = None;
        let mut groups = Vec::new();

        while !input.is_empty() {
            if input.peek(Ident) && !input.peek2(Token![::]) {
                let fork = input.fork();
                let kw: Ident = fork.parse()?;
                let kw_str = kw.to_string();

                if (kw_str == "editor" || kw_str == "controller" || kw_str == "initial")
                    && fork.peek(Token![:])
                    && !fork.peek2(Token![:])
                {
                    input.parse::<Ident>()?;
                    input.parse::<Token![:]>()?;

                    match kw_str.as_str() {
                        "editor" => editor_name = Some(input.parse::<Ident>()?),
                        "controller" => controller_name = Some(input.parse::<Ident>()?),
                        "initial" => initial = Some(input.parse::<Expr>()?),
                        _ => unreachable!(),
                    }

                    let _ = input.parse::<Token![,]>();
                    continue;
                }
            }

            // Must be a mode group `[...]: { ... }`
            let group: ModeGroup = input.parse()?;
            groups.push(group);
            let _ = input.parse::<Token![,]>();
        }

        Ok(ModalKeymapInput {
            editor_name,
            controller_name,
            initial,
            groups,
        })
    }
}

pub fn modal_keymap(input: TokenStream) -> TokenStream {
    let ModalKeymapInput {
        editor_name,
        controller_name,
        initial,
        groups,
    } = syn::parse_macro_input!(input as ModalKeymapInput);

    let eel = eel_path();

    let initial_expr = match initial {
        Some(e) => quote! { #e },
        None => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "modal_keymap! requires `initial: <expr>` to set the starting mode",
            )
            .into_compile_error()
            .into();
        }
    };

    let ctrl_ident: Option<Ident> = controller_name;

    let mut group_stmts = Vec::new();

    for ModeGroup { modes, bindings } in groups {
        let mut binding_stmts = Vec::new();

        for Binding { seq, action } in bindings {
            let action_tokens = match action {
                ActionKind::Block(block) => {
                    let editor_pat = match &editor_name {
                        Some(n) => quote! { #n },
                        None => quote! { _ },
                    };

                    let closure = if let Some(ctrl) = &ctrl_ident {
                        if block_uses_ident(&block, ctrl) {
                            quote! {
                                {
                                    let #ctrl = #ctrl.clone();
                                    move |#editor_pat: &_| #block
                                }
                            }
                        } else {
                            quote! { move |#editor_pat: &_| #block }
                        }
                    } else {
                        quote! { move |#editor_pat: &_| #block }
                    };

                    // Use `bind()` to constrain `_km` to `KeyMapping<E, Arc<dyn KeyAction<E>>>`.
                    binding_stmts.push(quote! {
                        _km.bind(
                            &#eel::keymap::key::parse_key_sequence(#seq)
                                .expect("invalid key sequence in modal_keymap! macro"),
                            #closure,
                        );
                    });
                    continue;
                }
                ActionKind::Expr(expr) => quote! { #expr },
            };

            binding_stmts.push(quote! {
                _km.add_binding(
                    &#eel::keymap::key::parse_key_sequence(#seq)
                        .expect("invalid key sequence in modal_keymap! macro"),
                    #action_tokens,
                );
            });
        }

        // Assign the KeyMapping to each mode in the group.
        // The last mode move-assigns; earlier ones clone.
        let n = modes.len();
        let mut assign_stmts = Vec::new();
        for (i, mode) in modes.into_iter().enumerate() {
            if i + 1 < n {
                assign_stmts.push(quote! {
                    *_mkm.keymap_for_mode(#mode) = _km.clone();
                });
            } else {
                assign_stmts.push(quote! {
                    *_mkm.keymap_for_mode(#mode) = _km;
                });
            }
        }

        group_stmts.push(quote! {
            {
                let mut _km = #eel::keymap::KeyMapping::new();
                #( #binding_stmts )*
                #( #assign_stmts )*
            }
        });
    }

    // Emit the mode controller binding if `controller:` was specified.
    let ctrl_setup = match &ctrl_ident {
        Some(ctrl) => quote! { let #ctrl = _mkm.mode_controller(); },
        None => quote! {},
    };

    quote! {{
        let mut _mkm = #eel::keymap::modal::ModalKeymap::new(#initial_expr);
        #ctrl_setup
        #( #group_stmts )*
        _mkm
    }}
    .into()
}
