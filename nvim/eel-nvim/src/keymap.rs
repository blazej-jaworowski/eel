use eel::Result;
use eel::keymap::{
    KeyEditor, KeyPress,
    key::{Key, Modifiers, SpecialKey},
};

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
            // keytrans uppercases the char in modifier combos (<C-a> → <C-A>).
            // This does NOT imply Shift — it is purely a notation convention.
            // Normalise back to lowercase for a stable round-trip.
            let c = if mods != Modifiers::none() {
                c.to_ascii_lowercase()
            } else {
                c
            };
            Key::Char(c)
        }
        s => Key::Special(SpecialKey::Unknown(s.to_string())),
    };

    Some(KeyPress::new(key, mods))
}

impl KeyEditor for NvimEditor {
    fn capture_keys<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(&KeyPress) + Send + Sync + 'static,
    {
        self.dispatch(move || -> std::result::Result<(), NvimError> {
            use nvim_oxi::mlua::{self, lua};

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
            Key::Char('<') => return "<LT>".to_string(),
            Key::Char('\\') => return "<Bslash>".to_string(),
            // Shift+ASCII-lowercase: send the uppercase char directly — a single
            // byte that vim.fn.feedkeys passes through unchanged.
            Key::Char(c) if kp.modifiers == Modifiers::shift() && c.is_ascii_lowercase() => {
                return c.to_ascii_uppercase().to_string();
            }
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
