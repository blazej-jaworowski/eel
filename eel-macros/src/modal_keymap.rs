use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Expr, Ident, Token,
    parse::{Parse, ParseStream, Result},
};

use crate::keymap::{ActionKind, Binding, KeymapInput};

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

    let eel = crate::eel_path();

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

    let mut group_stmts: Vec<TokenStream2> = Vec::new();

    for ModeGroup { modes, bindings } in groups {
        // Build a KeymapInput for this mode group. When a controller is present,
        // prepend `let ctrl = ctrl.clone(); let _ = &ctrl;` to every Block action
        // so the controller is available inside each move closure regardless of
        // whether the user references it.
        let km_bindings: Vec<Binding> = if let Some(ctrl) = &ctrl_ident {
            bindings
                .into_iter()
                .map(
                    |Binding {
                         seq,
                         action,
                         pre_closure,
                     }| {
                        let pre_closure = match &action {
                            ActionKind::Block(_) => {
                                quote! { let #ctrl = #ctrl.clone(); let _ = &#ctrl; }
                            }
                            ActionKind::Expr(_) => pre_closure,
                        };
                        Binding {
                            seq,
                            action,
                            pre_closure,
                        }
                    },
                )
                .collect()
        } else {
            bindings
        };

        let km_expr = KeymapInput {
            editor_name: editor_name.clone(),
            bindings: km_bindings,
        }
        .emit();

        // Assign the emitted KeyMapping to each mode in this group.
        // For a single mode, assign directly. For multiple modes, bind to a
        // temporary and clone for all but the last.
        let n = modes.len();
        let assign_stmts: Vec<TokenStream2> = if n == 1 {
            let mode = &modes[0];
            vec![quote! { *_mkm.keymap_for_mode(#mode) = #km_expr; }]
        } else {
            let mut stmts = vec![quote! { let _km_val = #km_expr; }];
            for (i, mode) in modes.iter().enumerate() {
                if i + 1 < n {
                    stmts.push(quote! { *_mkm.keymap_for_mode(#mode) = _km_val.clone(); });
                } else {
                    stmts.push(quote! { *_mkm.keymap_for_mode(#mode) = _km_val; });
                }
            }
            stmts
        };

        group_stmts.push(quote! { { #( #assign_stmts )* } });
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
