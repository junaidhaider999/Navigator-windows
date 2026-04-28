//! Parse `[hotkey]` chord strings into `RegisterHotKey` parameters.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_0, VK_A, VK_BACK,
    VK_ESCAPE, VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10, VK_F11,
    VK_F12, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_COMMA,
    VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS, VK_SPACE, VK_TAB, VK_Z,
};

/// Returns `(modifiers | MOD_NOREPEAT, vk)`.
pub fn parse_chord(raw: &str) -> Result<(HOT_KEY_MODIFIERS, u32), String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty [hotkey].chord".into());
    }
    let parts: Vec<&str> = s
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 {
        return Err(
            "hotkey must list one or more modifiers and a key (example: alt+/ or ctrl+shift+a)"
                .into(),
        );
    }
    let key_tok = parts[parts.len() - 1];
    let mut mods = MOD_NOREPEAT.0;
    for p in &parts[..parts.len() - 1] {
        match p.to_ascii_lowercase().as_str() {
            "alt" => mods |= MOD_ALT.0,
            "ctrl" | "control" => mods |= MOD_CONTROL.0,
            "shift" => mods |= MOD_SHIFT.0,
            "win" | "meta" | "windows" => mods |= MOD_WIN.0,
            _ => {
                return Err(format!(
                    "unknown modifier {p:?} (use alt, ctrl, shift, win)"
                ));
            }
        }
    }
    let need_mod = MOD_ALT.0 | MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_WIN.0;
    if mods & need_mod == 0 {
        return Err("add at least one of alt, ctrl, shift, win".into());
    }
    let vk = parse_key_token(key_tok)?;
    Ok((HOT_KEY_MODIFIERS(mods), vk))
}

fn parse_key_token(tok: &str) -> Result<u32, String> {
    let t = tok.trim();
    if t.is_empty() {
        return Err("missing key after '+'".into());
    }
    let tl = t.to_ascii_lowercase();
    if tl.len() >= 2 && tl.starts_with('f') {
        let n: u32 = tl[1..]
            .parse()
            .map_err(|_| format!("invalid function key {t:?}"))?;
        let vk = match n {
            1 => VK_F1,
            2 => VK_F2,
            3 => VK_F3,
            4 => VK_F4,
            5 => VK_F5,
            6 => VK_F6,
            7 => VK_F7,
            8 => VK_F8,
            9 => VK_F9,
            10 => VK_F10,
            11 => VK_F11,
            12 => VK_F12,
            _ => return Err("only F1 through F12 are supported".into()),
        };
        return Ok(vk.0 as u32);
    }
    match tl.as_str() {
        "space" => return Ok(VK_SPACE.0 as u32),
        "tab" => return Ok(VK_TAB.0 as u32),
        "escape" | "esc" => return Ok(VK_ESCAPE.0 as u32),
        "backspace" | "back" => return Ok(VK_BACK.0 as u32),
        _ => {}
    }
    let mut ch = t.chars();
    let Some(c0) = ch.next() else {
        return Err("missing key".into());
    };
    if ch.next().is_some() {
        return Err(format!(
            "unknown key {t:?} — use a single character, F1–F12, space, tab, esc, or backspace"
        ));
    }
    single_char_vk(c0)
}

fn single_char_vk(c: char) -> Result<u32, String> {
    let c = c.to_ascii_lowercase();
    match c {
        'a'..='z' => Ok((VK_A.0 + (c as u16 - 'a' as u16)) as u32),
        '0'..='9' => Ok((VK_0.0 as u32) + (c as u32 - u32::from(b'0'))),
        ';' => Ok(VK_OEM_1.0 as u32),
        '=' => Ok(VK_OEM_PLUS.0 as u32),
        '-' => Ok(VK_OEM_MINUS.0 as u32),
        ',' => Ok(VK_OEM_COMMA.0 as u32),
        '.' => Ok(VK_OEM_PERIOD.0 as u32),
        '/' => Ok(VK_OEM_2.0 as u32),
        '`' => Ok(VK_OEM_3.0 as u32),
        '[' => Ok(VK_OEM_4.0 as u32),
        '\\' => Ok(VK_OEM_5.0 as u32),
        ']' => Ok(VK_OEM_6.0 as u32),
        '\'' => Ok(VK_OEM_7.0 as u32),
        _ => Err(format!(
            "unsupported key {c:?} — try a letter, digit, ; , . / [ ] \\ ` ' = -, or F1–F12"
        )),
    }
}

#[inline]
pub(crate) fn vk_session_char(vk: u32) -> Option<char> {
    if (VK_A.0 as u32..=VK_Z.0 as u32).contains(&vk) {
        return Some(char::from_u32(vk - VK_A.0 as u32 + u32::from(b'a')).unwrap_or('a'));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_alt_slash() {
        let (m, vk) = parse_chord("alt+/").expect("parse");
        assert_eq!(vk, VK_OEM_2.0 as u32);
        assert_ne!(m.0 & MOD_ALT.0, 0);
        assert_ne!(m.0 & MOD_NOREPEAT.0, 0);
    }

    #[test]
    fn ctrl_shift_a() {
        let (m, vk) = parse_chord("ctrl+shift+a").expect("parse");
        assert_eq!(vk, VK_A.0 as u32);
        assert_ne!(m.0 & MOD_CONTROL.0, 0);
        assert_ne!(m.0 & MOD_SHIFT.0, 0);
    }

    #[test]
    fn rejects_plain_key() {
        assert!(parse_chord("a").is_err());
    }
}
