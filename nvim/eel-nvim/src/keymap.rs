use eel::Result;
use eel::keymap::{
    KeyEditor, KeyPress,
    key::{Key, Modifiers, SpecialKey},
};
use nvim_oxi::api::types::Mode;

use crate::{editor::NvimEditor, error::Error as NvimError};

/// Parse a Neovim key notation string (as returned by `vim.fn.keytrans`) into
/// a [`KeyPress`].  Returns `None` for empty or unrecognised input.
pub(crate) fn parse_key_notation(s: &str) -> Option<KeyPress> {
    if s.is_empty() {
        return None;
    }

    if s.starts_with('<') && s.ends_with('>') {
        return parse_angle_bracket(&s[1..s.len() - 1]);
    }

    let mut chars = s.chars();
    let c = chars.next()?;
    if chars.next().is_none() {
        Some(KeyPress::char(c))
    } else {
        None
    }
}

fn parse_angle_bracket(inner: &str) -> Option<KeyPress> {
    let mut mods = Modifiers::none();
    let mut rest = inner;

    loop {
        if let Some(r) = rest.strip_prefix("C-") {
            mods.ctrl = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("S-") {
            mods.shift = true;
            rest = r;
        } else if rest.strip_prefix("A-").is_some() || rest.strip_prefix("M-").is_some() {
            // Meta/Alt: not representable in our KeyPress model (terminal converts
            // Alt+key to a Unicode character before nvim sees it on this system).
            return None;
        } else {
            break;
        }
    }

    let key = match rest.to_uppercase().as_str() {
        "CR" | "RETURN" | "ENTER" => Key::Special(SpecialKey::Enter),
        "ESC" | "ESCAPE" => Key::Special(SpecialKey::Escape),
        "BS" | "BACKSPACE" => Key::Special(SpecialKey::Backspace),
        "TAB" => Key::Special(SpecialKey::Tab),
        "UP" => Key::Special(SpecialKey::Up),
        "DOWN" => Key::Special(SpecialKey::Down),
        "LEFT" => Key::Special(SpecialKey::Left),
        "RIGHT" => Key::Special(SpecialKey::Right),
        "HOME" => Key::Special(SpecialKey::Home),
        "END" => Key::Special(SpecialKey::End),
        "PAGEUP" => Key::Special(SpecialKey::PageUp),
        "PAGEDOWN" => Key::Special(SpecialKey::PageDown),
        "DEL" | "DELETE" => Key::Special(SpecialKey::Delete),
        "INS" | "INSERT" => Key::Special(SpecialKey::Insert),
        "LT" => Key::Char('<'),
        "BSLASH" => Key::Char('\\'),
        "SPACE" => Key::Char(' '),
        s if s.starts_with('F') => {
            let n: u8 = s[1..].parse().ok()?;
            Key::Special(SpecialKey::F(n))
        }
        s if s.chars().count() == 1 => {
            let c = s.chars().next()?;
            if mods.ctrl {
                // keytrans uppercases the char in ctrl combos (<C-a> → <C-A>).
                // This does NOT imply Shift; normalise back to lowercase.
                Key::Char(c.to_ascii_lowercase())
            } else if mods.shift && c.is_ascii_alphabetic() {
                // shift+letter: encode as the uppercase char, absorb shift into case.
                mods.shift = false;
                Key::Char(c.to_ascii_uppercase())
            } else {
                Key::Char(c)
            }
        }
        s => Key::Special(SpecialKey::Unknown(s.to_string())),
    };

    Some(KeyPress::new(key, mods))
}

/// Clear all user-defined keymaps for a given mode so that raw keypresses
/// reach the `vim.on_key` handler without mapping expansion.
fn clear_keymaps_for_mode(mode: Mode) {
    use std::panic::catch_unwind;

    catch_unwind(|| {
        for binding in nvim_oxi::api::get_keymap(mode) {
            if binding.lhs.starts_with("<Plug>") {
                continue;
            }
            let _ = nvim_oxi::api::del_keymap(mode, &binding.lhs);
        }
    })
    .ok();
}

impl KeyEditor for NvimEditor {
    fn capture_keys<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(&KeyPress) + Send + Sync + 'static,
    {
        self.dispatch(move || -> std::result::Result<(), NvimError> {
            use nvim_oxi::mlua::{self, lua};

            // Clear all user-defined keymaps so every key goes directly to
            // this handler without being intercepted by mapping expansion.
            for mode in [Mode::Normal, Mode::Insert, Mode::Visual] {
                clear_keymaps_for_mode(mode);
            }

            let ns_id = nvim_oxi::api::create_namespace("eel");

            let on_key_cb = lua().create_function(move |lua_ctx, key: mlua::String| {
                let raw_bytes = key.as_bytes().to_vec();

                let keytrans_fn: mlua::Function = {
                    let vim: mlua::Table = lua_ctx.globals().get("vim")?;
                    let fn_table: mlua::Table = vim.get("fn")?;
                    fn_table.get("keytrans")?
                };

                let key_lua_str = lua_ctx.create_string(&raw_bytes)?;
                let key_str: String = keytrans_fn.call(key_lua_str)?;

                if let Some(key_press) = parse_key_notation(&key_str) {
                    callback(&key_press);
                }

                // Return "" to suppress normal key processing.
                // NOTE: <C-c> is fired twice by nvim — once via the normal keypress path
                // and once by the C-level interrupt mechanism (got_int), which re-checks
                // the typeahead regardless of our return value.  The duplicate arrives as
                // an identical on_key invocation.  This is a known limitation: callers
                // should be prepared to receive two consecutive identical KeyPress values
                // when the user presses <C-c>.
                lua_ctx.create_string(b"")
            })?;

            let vim: mlua::Table = lua().globals().get("vim")?;
            let on_key_fn: mlua::Function = vim.get("on_key")?;
            on_key_fn.call::<()>((on_key_cb, ns_id))?;

            Ok(())
        })??;

        Ok(())
    }
}

#[cfg(feature = "nvim-tests")]
mod tests {
    use eel::eel_keyeditor_tests;
    use eel::keymap::KeyPress;
    use eel::keymap::key::{Key, Modifiers, SpecialKey};

    use crate::editor::NvimEditor;
    use crate::error::Error as NvimError;

    fn key_press_to_notation(kp: &KeyPress) -> String {
        let key_str = match &kp.key {
            // These chars need angle-bracket notation regardless of modifiers, so
            // produce them as key_str and fall through to the mod-building code.
            Key::Char('<') => "LT".to_string(),
            Key::Char('\\') => "Bslash".to_string(),
            Key::Char(c) if kp.modifiers == Modifiers::none() => return c.to_string(),
            Key::Char(c) => c.to_string(),
            Key::Special(s) => match s {
                SpecialKey::Enter => "CR".to_string(),
                SpecialKey::Escape => "Esc".to_string(),
                SpecialKey::Backspace => "BS".to_string(),
                SpecialKey::Tab => "Tab".to_string(),
                SpecialKey::Up => "Up".to_string(),
                SpecialKey::Down => "Down".to_string(),
                SpecialKey::Left => "Left".to_string(),
                SpecialKey::Right => "Right".to_string(),
                SpecialKey::Home => "Home".to_string(),
                SpecialKey::End => "End".to_string(),
                SpecialKey::PageUp => "PageUp".to_string(),
                SpecialKey::PageDown => "PageDown".to_string(),
                SpecialKey::Delete => "Del".to_string(),
                SpecialKey::Insert => "Insert".to_string(),
                SpecialKey::F(n) => format!("F{n}"),
                SpecialKey::Unknown(s) => return format!("<{s}>"),
            },
        };

        let mods = format!(
            "{}{}",
            if kp.modifiers.ctrl { "C-" } else { "" },
            if kp.modifiers.shift { "S-" } else { "" },
        );

        format!("<{mods}{key_str}>")
    }

    impl eel::keymap::tests::TestKeyEditor for NvimEditor {
        fn send_test_key(&self, key: &KeyPress) {
            let notation = key_press_to_notation(key);
            self.dispatch(move || -> std::result::Result<(), NvimError> {
                use nvim_oxi::mlua::{self, lua};
                let vim: mlua::Table = lua().globals().get("vim")?;
                let api: mlua::Table = vim.get("api")?;
                let replace_termcodes: mlua::Function = api.get("nvim_replace_termcodes")?;
                let feedkeys: mlua::Function = api.get("nvim_feedkeys")?;
                // Convert notation (<LT>, <Up>, <C-a>, …) to K_SPECIAL-encoded
                // bytes (the internal key representation nvim uses in typeahead).
                let replaced: mlua::String = replace_termcodes.call::<mlua::String>((
                    notation.as_str(),
                    true,
                    true,
                    true,
                ))?;
                // escape_ks=false: bytes are already in internal format, insert as-is.
                feedkeys.call::<()>((replaced, "x", false))?;
                Ok(())
            })
            .expect("send_test_key dispatch failed")
            .expect("send_test_key Lua error");
        }
    }

    eel_keyeditor_tests!(
        ::eel_nvim_macros::nvim_test,
        crate::test_utils::nvim_editor_factory
    );
}

#[cfg(all(feature = "nvim-tests", feature = "modal"))]
mod modal_tests {
    use eel::eel_modal_tests;

    eel_modal_tests!(
        ::eel_nvim_macros::nvim_test,
        crate::test_utils::nvim_editor_factory
    );
}

#[cfg(test)]
mod parse_key_notation_tests {
    use super::parse_key_notation;
    use eel::keymap::{
        KeyPress,
        key::{Key, Modifiers, SpecialKey},
    };

    fn ctrl(c: char) -> KeyPress {
        KeyPress::new(
            Key::Char(c),
            Modifiers {
                ctrl: true,
                shift: false,
            },
        )
    }

    fn shift_special(k: SpecialKey) -> KeyPress {
        KeyPress::new(
            Key::Special(k),
            Modifiers {
                ctrl: false,
                shift: true,
            },
        )
    }

    #[test]
    fn empty_input_returns_none() {
        assert_eq!(parse_key_notation(""), None);
    }

    #[test]
    fn multichar_non_notation_returns_none() {
        // A bare multi-char string that isn't angle-bracket notation is unrecognised.
        assert_eq!(parse_key_notation("ab"), None);
    }

    #[test]
    fn plain_single_char() {
        assert_eq!(parse_key_notation("a"), Some(KeyPress::char('a')));
        assert_eq!(parse_key_notation("Z"), Some(KeyPress::char('Z')));
        assert_eq!(parse_key_notation("1"), Some(KeyPress::char('1')));
    }

    #[test]
    fn cr_maps_to_enter() {
        assert_eq!(
            parse_key_notation("<CR>"),
            Some(KeyPress::special(SpecialKey::Enter))
        );
        assert_eq!(
            parse_key_notation("<Return>"),
            Some(KeyPress::special(SpecialKey::Enter))
        );
    }

    #[test]
    fn esc_maps_to_escape() {
        assert_eq!(
            parse_key_notation("<Esc>"),
            Some(KeyPress::special(SpecialKey::Escape))
        );
    }

    #[test]
    fn bs_maps_to_backspace() {
        assert_eq!(
            parse_key_notation("<BS>"),
            Some(KeyPress::special(SpecialKey::Backspace))
        );
    }

    #[test]
    fn space_lt_bslash() {
        assert_eq!(parse_key_notation("<Space>"), Some(KeyPress::char(' ')));
        assert_eq!(parse_key_notation("<LT>"), Some(KeyPress::char('<')));
        assert_eq!(parse_key_notation("<Bslash>"), Some(KeyPress::char('\\')));
    }

    #[test]
    fn ctrl_letter_normalised() {
        // keytrans emits <C-A> for ctrl+a; parse_angle_bracket lowercases.
        assert_eq!(parse_key_notation("<C-A>"), Some(ctrl('a')));
        assert_eq!(parse_key_notation("<C-j>"), Some(ctrl('j')));
    }

    #[test]
    fn shift_letter_absorbed() {
        // shift+alpha: encoded as uppercase char, shift bit cleared.
        assert_eq!(parse_key_notation("<S-a>"), Some(KeyPress::char('A')));
    }

    #[test]
    fn shift_special_key() {
        assert_eq!(
            parse_key_notation("<S-Up>"),
            Some(shift_special(SpecialKey::Up))
        );
    }

    #[test]
    fn function_keys() {
        assert_eq!(
            parse_key_notation("<F1>"),
            Some(KeyPress::special(SpecialKey::F(1)))
        );
        assert_eq!(
            parse_key_notation("<F12>"),
            Some(KeyPress::special(SpecialKey::F(12)))
        );
    }

    #[test]
    fn meta_alt_returns_none() {
        // Meta/Alt combos are not representable in our model.
        assert_eq!(parse_key_notation("<A-a>"), None);
        assert_eq!(parse_key_notation("<M-a>"), None);
    }
}
