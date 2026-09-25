//! 热键字符串解析、规范化与虚拟键码映射。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeySpec {
    pub modifiers: u32,
    pub vk: u32,
}

impl HotkeySpec {
    pub fn win32_modifiers(&self) -> HOT_KEY_MODIFIERS {
        HOT_KEY_MODIFIERS(self.modifiers) | MOD_NOREPEAT
    }
}

fn vk_of_key(name: &str) -> Option<u32> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    let upper = n.to_ascii_uppercase();
    if upper.len() == 1 {
        let c = upper.as_bytes()[0];
        return match c {
            b'A'..=b'Z' | b'0'..=b'9' => Some(c as u32),
            b'`' | b'~' => Some(0xC0),
            b'-' => Some(0xBD),
            b'=' => Some(0xBB),
            b'[' => Some(0xDB),
            b']' => Some(0xDD),
            b'\\' => Some(0xDC),
            b';' => Some(0xBA),
            b'\'' => Some(0xDE),
            b',' => Some(0xBC),
            b'.' => Some(0xBE),
            b'/' => Some(0xBF),
            _ => None,
        };
    }
    if let Some(num) = upper.strip_prefix('F') {
        if let Ok(k) = num.parse::<u32>() {
            if (1..=24).contains(&k) {
                return Some(0x70 + k - 1);
            }
        }
    }
    Some(match upper.as_str() {
        "SPACE" => 0x20,
        "ENTER" | "RETURN" => 0x0D,
        "TAB" => 0x09,
        "ESC" | "ESCAPE" => 0x1B,
        "BACKSPACE" => 0x08,
        "DELETE" | "DEL" => 0x2E,
        "INSERT" | "INS" => 0x2D,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" | "PGUP" => 0x21,
        "PAGEDOWN" | "PGDN" => 0x22,
        "UP" | "ARROWUP" => 0x26,
        "DOWN" | "ARROWDOWN" => 0x28,
        "LEFT" | "ARROWLEFT" => 0x25,
        "RIGHT" | "ARROWRIGHT" => 0x27,
        "PAUSE" => 0x13,
        "CAPSLOCK" => 0x14,
        "NUMLOCK" => 0x90,
        "SCROLLLOCK" => 0x91,
        "PRINTSCREEN" => 0x2C,
        _ => return None,
    })
}

/// 解析 "Ctrl+Shift+T" / "Alt+Space" 形式的热键
pub fn parse_hotkey(text: &str) -> Option<HotkeySpec> {
    let mut mods = 0u32;
    let mut key: Option<u32> = None;
    for part in text.split('+') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match p.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => mods |= MOD_CONTROL.0,
            "ALT" => mods |= MOD_ALT.0,
            "SHIFT" => mods |= MOD_SHIFT.0,
            "WIN" | "META" | "SUPER" | "CMD" => mods |= MOD_WIN.0,
            _ => key = vk_of_key(p),
        }
    }
    key.map(|vk| HotkeySpec { modifiers: mods, vk })
}

/// 规范化显示："ctrl + shift + t" -> "Ctrl+Shift+T"
pub fn normalize(text: &str) -> String {
    let mut mods: Vec<&str> = Vec::new();
    let mut key = String::new();
    for part in text.split('+') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match p.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => mods.push("Ctrl"),
            "ALT" => mods.push("Alt"),
            "SHIFT" => mods.push("Shift"),
            "WIN" | "META" | "SUPER" | "CMD" => mods.push("Win"),
            _ => key = display_key_name(p),
        }
    }
    let mut out: Vec<&str> = Vec::new();
    for m in ["Ctrl", "Alt", "Shift", "Win"] {
        if mods.contains(&m) {
            out.push(m);
        }
    }
    let mut s = out.join("+");
    if !key.is_empty() {
        if !s.is_empty() {
            s.push('+');
        }
        s.push_str(&key);
    }
    s
}

fn display_key_name(p: &str) -> String {
    let upper = p.to_ascii_uppercase();
    match upper.as_str() {
        "SPACE" => "Space".into(),
        "ENTER" | "RETURN" => "Enter".into(),
        "TAB" => "Tab".into(),
        "ESC" | "ESCAPE" => "Esc".into(),
        "BACKSPACE" => "Backspace".into(),
        "DELETE" | "DEL" => "Delete".into(),
        "INSERT" | "INS" => "Insert".into(),
        "HOME" => "Home".into(),
        "END" => "End".into(),
        "PAGEUP" | "PGUP" => "PageUp".into(),
        "PAGEDOWN" | "PGDN" => "PageDown".into(),
        "UP" | "ARROWUP" => "Up".into(),
        "DOWN" | "ARROWDOWN" => "Down".into(),
        "LEFT" | "ARROWLEFT" => "Left".into(),
        "RIGHT" | "ARROWRIGHT" => "Right".into(),
        _ => upper,
    }
}

/// 将 Slint KeyEvent 的 text（含私有区特殊键码）转换为按键名；修饰键本身返回 None。
pub fn key_name_from_slint(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        // 多字符文本（如输入法）忽略
        return None;
    }
    let name = match c {
        '\u{0010}'..='\u{0018}' => return None, // Shift/Control/Alt/Meta 等修饰键
        ' ' => "Space".to_string(),
        '\u{000a}' | '\r' => "Enter".into(),
        '\u{0009}' => "Tab".into(),
        '\u{001b}' => "Esc".into(),
        '\u{0008}' => "Backspace".into(),
        '\u{007f}' => "Delete".into(),
        '\u{F700}' => "Up".into(),
        '\u{F701}' => "Down".into(),
        '\u{F702}' => "Left".into(),
        '\u{F703}' => "Right".into(),
        '\u{F704}'..='\u{F71B}' => format!("F{}", (c as u32 - 0xF704) + 1),
        '\u{F727}' => "Insert".into(),
        '\u{F729}' => "Home".into(),
        '\u{F72B}' => "End".into(),
        '\u{F72C}' => "PageUp".into(),
        '\u{F72D}' => "PageDown".into(),
        c if c.is_ascii_alphanumeric() => c.to_ascii_uppercase().to_string(),
        c if c.is_ascii_punctuation() => c.to_string(),
        c if (c as u32) < 0x20 => {
            // Ctrl+字母在部分平台会产生控制字符
            let letter = (b'A' + (c as u8 - 1)) as char;
            if letter.is_ascii_uppercase() {
                letter.to_string()
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(name)
}

/// 组合修饰键与按键名为规范化字符串
pub fn combo(ctrl: bool, alt: bool, shift: bool, win: bool, key: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if win {
        parts.push("Win");
    }
    parts.push(key);
    parts.join("+")
}

/// 与系统常用组合冲突时给出提示（本地静态规则）
pub fn system_conflict_hint(hotkey: &str) -> Option<&'static str> {
    let n = normalize(hotkey);
    let reserved = [
        ("Ctrl+C", "系统复制"),
        ("Ctrl+V", "系统粘贴"),
        ("Ctrl+X", "系统剪切"),
        ("Ctrl+Z", "撤销"),
        ("Ctrl+A", "全选"),
        ("Ctrl+S", "保存"),
        ("Alt+Tab", "窗口切换"),
        ("Alt+F4", "关闭窗口"),
        ("Win+L", "锁定屏幕"),
        ("Win+D", "显示桌面"),
        ("Win+E", "资源管理器"),
        ("Win+R", "运行"),
        ("Win+S", "Windows 搜索"),
        ("Ctrl+Alt+Delete", "安全桌面"),
    ];
    reserved.iter().find(|(k, _)| *k == n).map(|(_, why)| *why)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let s = parse_hotkey("Alt+C").unwrap();
        assert_eq!(s.modifiers, MOD_ALT.0);
        assert_eq!(s.vk, 0x43);
        let s = parse_hotkey("ctrl + shift + F5").unwrap();
        assert_eq!(s.modifiers, MOD_CONTROL.0 | MOD_SHIFT.0);
        assert_eq!(s.vk, 0x74);
        assert!(parse_hotkey("Alt").is_none());
        assert_eq!(normalize("shift+ctrl+t"), "Ctrl+Shift+T");
    }

    #[test]
    fn slint_key_names() {
        assert_eq!(key_name_from_slint("c").as_deref(), Some("C"));
        assert_eq!(key_name_from_slint(" ").as_deref(), Some("Space"));
        assert_eq!(key_name_from_slint("\u{F704}").as_deref(), Some("F1"));
        assert!(key_name_from_slint("\u{0011}").is_none());
    }
}
