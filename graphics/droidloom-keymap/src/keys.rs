//! Key and button names accepted in profiles, as Linux evdev codes.

/// Parse a key or button name, or a number, into an evdev code.
///
/// Names are case-insensitive: `A` to `Z`, `0` to `9`, `F1` to `F12`,
/// `Space`, `Tab`, `Enter`, `Escape`, `Grave`, `Minus`, `Equal`, `CapsLock`,
/// `LeftShift`, `RightShift`, `LeftCtrl`, `LeftAlt`, and the mouse buttons
/// `MouseRight`, `MouseMiddle`, `MouseSide` and `MouseExtra`. A plain decimal
/// number, or `code:N`, is taken as the evdev code itself.
#[must_use]
pub fn parse_key(name: &str) -> Option<u16> {
    let trimmed = name.trim();
    if let Some(number) = trimmed.strip_prefix("code:") {
        return number.trim().parse().ok();
    }
    if let Ok(number) = trimmed.parse::<u16>() {
        return Some(number);
    }
    let lower = trimmed.to_ascii_lowercase();
    let code = match lower.as_str() {
        "escape" | "esc" => 1,
        "1" => 2,
        "2" => 3,
        "3" => 4,
        "4" => 5,
        "5" => 6,
        "6" => 7,
        "7" => 8,
        "8" => 9,
        "9" => 10,
        "0" => 11,
        "minus" => 12,
        "equal" => 13,
        "tab" => 15,
        "q" => 16,
        "w" => 17,
        "e" => 18,
        "r" => 19,
        "t" => 20,
        "y" => 21,
        "u" => 22,
        "i" => 23,
        "o" => 24,
        "p" => 25,
        "enter" => 28,
        "leftctrl" => 29,
        "a" => 30,
        "s" => 31,
        "d" => 32,
        "f" => 33,
        "g" => 34,
        "h" => 35,
        "j" => 36,
        "k" => 37,
        "l" => 38,
        "grave" => 41,
        "leftshift" => 42,
        "z" => 44,
        "x" => 45,
        "c" => 46,
        "v" => 47,
        "b" => 48,
        "n" => 49,
        "m" => 50,
        "rightshift" => 54,
        "leftalt" => 56,
        "space" => 57,
        "capslock" => 58,
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "f6" => 64,
        "f7" => 65,
        "f8" => 66,
        "f9" => 67,
        "f10" => 68,
        "f11" => 87,
        "f12" => 88,
        "mouseright" => 0x111,
        "mousemiddle" => 0x112,
        "mouseside" => 0x113,
        "mouseextra" => 0x114,
        _ => return None,
    };
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::parse_key;

    #[test]
    fn names_numbers_and_prefixed_codes_agree() {
        assert_eq!(parse_key("E"), Some(18));
        assert_eq!(parse_key("e"), Some(18));
        assert_eq!(parse_key("18"), Some(18));
        assert_eq!(parse_key("code:18"), Some(18));
        assert_eq!(parse_key(" Space "), Some(57));
        assert_eq!(parse_key("MouseRight"), Some(0x111));
    }

    #[test]
    fn unknown_names_are_rejected() {
        assert_eq!(parse_key("Banana"), None);
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("code:x"), None);
        assert_eq!(parse_key("70000"), None);
    }
}
