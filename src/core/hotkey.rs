//! 全局快捷键的表示与文本解析。
//!
//! 存盘/展示用规范字符串，例如 `Ctrl+Alt+Shift+W`；平台层再把它翻译成
//! `global_hotkey` 的 `HotKey`。纯逻辑，可单测。

/// 一个全局快捷键组合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Windows 键（macOS 上是 Command）。
    pub win: bool,
    /// 主键：单个字母/数字（大写），或 F1..F12，或 `Space` / `Enter` 等命名键。
    pub key: String,
}

/// 默认快捷键。
pub const DEFAULT: &str = "Ctrl+Alt+Shift+W";

impl HotkeySpec {
    pub fn default_spec() -> Self {
        Self::parse(DEFAULT).expect("默认快捷键必须能解析")
    }

    /// 解析 `Ctrl+Alt+Shift+W` 这类写法。必须带至少一个修饰键。
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut spec = Self {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            key: String::new(),
        };

        let tokens: Vec<&str> = raw
            .split('+')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .collect();
        if tokens.is_empty() {
            return Err("快捷键不能为空".into());
        }

        for token in &tokens {
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "ctl" => spec.ctrl = true,
                "alt" | "option" | "opt" => spec.alt = true,
                "shift" => spec.shift = true,
                "win" | "super" | "meta" | "cmd" | "command" => spec.win = true,
                _ => {
                    if !spec.key.is_empty() {
                        return Err(format!("「{raw}」里有多个主键"));
                    }
                    spec.key = normalize_key(token)?;
                }
            }
        }

        if spec.key.is_empty() {
            return Err("缺少主键（例如 W）".into());
        }
        if !spec.has_modifier() {
            return Err("至少要带一个修饰键（Ctrl / Alt / Shift / Win）".into());
        }
        Ok(spec)
    }

    pub fn format(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Win");
        }
        let mut out = parts.join("+");
        if !out.is_empty() {
            out.push('+');
        }
        out.push_str(&self.key);
        out
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }
}

/// 规范化主键名。
fn normalize_key(token: &str) -> Result<String, String> {
    let lower = token.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" | "空格" => Some("Space"),
        "enter" | "return" => Some("Enter"),
        "tab" => Some("Tab"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pgup" | "pageup" => Some("PageUp"),
        "pgdn" | "pagedown" => Some("PageDown"),
        _ => None,
    };
    if let Some(name) = named {
        return Ok(name.to_string());
    }

    if let Some(rest) = lower.strip_prefix('f')
        && let Ok(number) = rest.parse::<u8>()
    {
        if (1..=12).contains(&number) {
            return Ok(format!("F{number}"));
        }
        return Err(format!("不支持的按键 {token}（功能键只支持 F1–F12）"));
    }

    let chars: Vec<char> = token.chars().collect();
    if chars.len() == 1 {
        let ch = chars[0];
        if ch.is_ascii_alphanumeric() {
            return Ok(ch.to_ascii_uppercase().to_string());
        }
    }

    Err(format!("不支持的按键 {token}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips() {
        let spec = HotkeySpec::default_spec();
        assert_eq!(spec.format(), DEFAULT);
        assert!(spec.ctrl && spec.alt && spec.shift && !spec.win);
        assert_eq!(spec.key, "W");
    }

    #[test]
    fn parse_is_case_and_order_insensitive() {
        let a = HotkeySpec::parse("ctrl+alt+shift+w").unwrap();
        let b = HotkeySpec::parse("Shift+Ctrl+Alt+W").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.format(), "Ctrl+Alt+Shift+W");
    }

    #[test]
    fn parse_accepts_function_and_named_keys() {
        assert_eq!(HotkeySpec::parse("Ctrl+F9").unwrap().key, "F9");
        assert_eq!(HotkeySpec::parse("Win+Alt+Space").unwrap().key, "Space");
        assert_eq!(HotkeySpec::parse("Ctrl+Alt+1").unwrap().key, "1");
    }

    #[test]
    fn modifiers_only_is_rejected() {
        assert!(HotkeySpec::parse("Ctrl+Alt").is_err(), "没有主键应报错");
    }

    #[test]
    fn missing_modifier_is_rejected() {
        assert!(HotkeySpec::parse("W").is_err(), "单键太容易误触，应拒绝");
    }

    #[test]
    fn unsupported_keys_are_rejected() {
        assert!(HotkeySpec::parse("Ctrl+F13").is_err());
        assert!(HotkeySpec::parse("Ctrl+Alt+,").is_err());
        assert!(HotkeySpec::parse("Ctrl+A+B").is_err(), "多个主键应报错");
    }

    #[test]
    fn empty_is_rejected() {
        assert!(HotkeySpec::parse("").is_err());
        assert!(HotkeySpec::parse("   ").is_err());
    }
}
