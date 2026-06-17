/// Returns the current UTC time as an ISO 8601 string.
pub fn now_iso_string() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time = secs % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}

/// Strip emoji characters from a string. Keeps CJK characters, ASCII, and
/// common punctuation. Removes variation selectors, ZWJ, skin-tone modifiers,
/// and all codepoints in known emoji blocks.
pub fn strip_emoji(s: &str) -> String {
    let stripped: String = s.chars().filter(|&c| !is_emoji(c)).collect();
    stripped.trim().to_string()
}

fn is_emoji(c: char) -> bool {
    matches!(
        c,
        '\u{200D}' // ZWJ
        | '\u{FE0F}' // VS16
        | '\u{FE0E}' // VS15
        | '\u{00A9}' | '\u{00AE}' // © ®
        | '\u{203C}' | '\u{2049}' // ‼ ⁉
        | '\u{2122}' | '\u{2139}' // ™ ℹ
        | '\u{2194}'..='\u{2199}' // arrows
        | '\u{21A9}'..='\u{21AA}' // ↩ ↪
        | '\u{231A}'..='\u{231B}' // ⌚ ⌛
        | '\u{2328}' | '\u{23CF}' // ⌨ ⏏
        | '\u{23E9}'..='\u{23F3}' // ⏩-⏳
        | '\u{23F8}'..='\u{23FA}' // ⏸-⏺
        | '\u{24C2}' | '\u{25AA}'..='\u{25AB}' // Ⓜ ▪ ▫
        | '\u{25B6}' | '\u{25C0}' | '\u{25FB}'..='\u{25FE}' // ▶ ◀ ◻-◾
        | '\u{2600}'..='\u{27BF}' // Misc Symbols, Dingbats
        | '\u{2934}'..='\u{2935}' // ⤴ ⤵
        | '\u{2B05}'..='\u{2B07}' // ←-↓
        | '\u{2B1B}'..='\u{2B1C}' // ⬛ ⬜
        | '\u{2B50}' | '\u{2B55}' // ⭐ ⭕
        | '\u{3030}' | '\u{303D}' // 〰 〽
        | '\u{3297}' | '\u{3299}' // ㊗ ㊙
        | '\u{1F004}' | '\u{1F0CF}' // 🀄 🃏
        | '\u{1F170}'..='\u{1F171}' // 🅰 🅱
        | '\u{1F17E}'..='\u{1F17F}' // 🅾 🅿
        | '\u{1F18E}' // 🆎
        | '\u{1F191}'..='\u{1F19A}' // 🆑-🆚
        | '\u{1F1E6}'..='\u{1F1FF}' // Regional indicators (flags)
        | '\u{1F201}'..='\u{1F202}' // 🈁 🈂
        | '\u{1F21A}' | '\u{1F22F}' // 🈚 🈯
        | '\u{1F232}'..='\u{1F23A}' // 🈲-🈺
        | '\u{1F250}'..='\u{1F251}' // 🉐 🉑
        | '\u{1F300}'..='\u{1F5FF}' // Misc Symbols and Pictographs
        | '\u{1F600}'..='\u{1F64F}' // Emoticons
        | '\u{1F680}'..='\u{1F6FF}' // Transport and Map
        | '\u{1F7E0}'..='\u{1F7EB}' // 🟠-🟫
        | '\u{1F7F0}' // 🟰
        | '\u{1F900}'..='\u{1F9FF}' // Supplemental Symbols
        | '\u{1FA00}'..='\u{1FA6F}' // Chess Symbols
        | '\u{1FA70}'..='\u{1FAFF}' // Symbols Ext-A
    )
}

/// Returns today's date as YYYY-MM-DD (UTC).
pub fn today_iso_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let days = (now.as_secs() / 86400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Converts days since Unix epoch to (year, month, day) using Howard Hinnant's algorithm.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Converts (year, month, day) to days since Unix epoch using Howard Hinnant's algorithm.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m <= 2 { m + 9 } else { m - 3 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// Returns the weekday name for a given number of days since Unix epoch.
/// Epoch day 0 is Thursday (1970-01-01).
pub fn weekday_name(days: i64) -> &'static str {
    const WEEKDAYS: [&str; 7] = [
        "Thursday", "Friday", "Saturday",
        "Sunday", "Monday", "Tuesday", "Wednesday",
    ];
    let idx = days.rem_euclid(7);
    WEEKDAYS[idx as usize]
}

/// Returns weekday index: 0=Monday, 6=Sunday.
pub fn weekday_index(days: i64) -> i64 {
    (days + 3) % 7
}

/// Returns the current UTC time as a human-readable string: "YYYY-MM-DD HH:MM:SS UTC (Weekday)".
pub fn now_utc_human() -> String {
    let secs = now_unix_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = civil_from_days(days);
    let weekday = weekday_name(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC ({})",
        year, month, day, hours, minutes, seconds, weekday
    )
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Removes a markdown image link `![any text](image_id)` from text.
/// Also strips any trailing newline left behind.
pub fn strip_markdown_image_id(text: &str, image_id: &str) -> String {
    let search = format!("]({})", image_id);
    if let Some(pos) = text.find(&search) {
        if let Some(start) = text[..pos].rfind("![") {
            let end = pos + search.len();
            let mut result = String::with_capacity(text.len());
            result.push_str(&text[..start]);
            if end < text.len() && text.as_bytes().get(end) == Some(&b'\n') {
                result.push_str(&text[end + 1..]);
            } else {
                result.push_str(&text[end..]);
            }
            return result.trim().to_string();
        }
    }
    text.replace(image_id, "")
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_civil_from_days_epoch() {
        let (y, m, d) = civil_from_days(0);
        assert_eq!(y, 1970);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn test_civil_from_days_known() {
        let (y, m, d) = civil_from_days(20089);
        assert_eq!(y, 2025);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn test_days_from_civil_epoch() {
        let days = days_from_civil(1970, 1, 1);
        assert_eq!(days, 0);
    }

    #[test]
    fn test_days_from_civil_roundtrip() {
        let days = 20089;
        let (y, m, d) = civil_from_days(days);
        let back = days_from_civil(y, m, d);
        assert_eq!(days, back);
    }

    #[test]
    fn test_weekday_name_epoch() {
        assert_eq!(weekday_name(0), "Thursday");
    }

    #[test]
    fn test_weekday_name_known() {
        let mon = days_from_civil(2026, 6, 8);
        assert_eq!(weekday_name(mon), "Monday");
        let wed = days_from_civil(2026, 6, 10);
        assert_eq!(weekday_name(wed), "Wednesday");
        let sun = days_from_civil(2026, 6, 14);
        assert_eq!(weekday_name(sun), "Sunday");
    }

    #[test]
    fn test_weekday_index() {
        let mon = days_from_civil(2026, 6, 8);
        assert_eq!(weekday_index(mon), 0);
        let sun = days_from_civil(2026, 6, 14);
        assert_eq!(weekday_index(sun), 6);
    }

    #[test]
    fn test_now_utc_human() {
        let s = now_utc_human();
        assert!(s.contains("UTC ("), "Expected 'UTC (' in: {s}");
        // Should contain a weekday name in parentheses
        for w in ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"] {
            if s.contains(w) {
                return;
            }
        }
        panic!("Expected a weekday name in: {s}");
    }

    #[test]
    fn test_strip_emoji_basic() {
        assert_eq!(strip_emoji("零夢 ✨"), "零夢");
        assert_eq!(strip_emoji("香菜 🌿"), "香菜");
        assert_eq!(strip_emoji("雪山泡芙 ✨"), "雪山泡芙");
    }

    #[test]
    fn test_strip_emoji_no_emoji() {
        assert_eq!(strip_emoji("零夢"), "零夢");
        assert_eq!(strip_emoji("rockbot"), "rockbot");
    }

    #[test]
    fn test_strip_emoji_emoji_only() {
        assert_eq!(strip_emoji("✨🌿"), "");
    }

    #[test]
    fn test_strip_markdown_image_id_basic() {
        let text = "Here is an image:\n\n![A cat](call_abc123)\n\nDo you like it?";
        let result = strip_markdown_image_id(text, "call_abc123");
        assert!(!result.contains("call_abc123"));
        assert!(!result.contains("!["), "markdown image syntax should be removed");
        assert!(result.contains("Here is an image"));
        assert!(result.contains("Do you like it"));
    }

    #[test]
    fn test_strip_markdown_image_id_no_match() {
        let text = "No image here, just text.";
        let result = strip_markdown_image_id(text, "call_abc123");
        assert_eq!(result, text);
    }

    #[test]
    fn test_strip_markdown_image_id_inline() {
        let text = "Look at ![the cat](call_xyz)\nSo cute!";
        let result = strip_markdown_image_id(text, "call_xyz");
        assert!(!result.contains("!["), "markdown image syntax should be removed");
        assert!(result.contains("So cute!"));
    }

    #[test]
    fn test_strip_markdown_image_id_key_only() {
        let text = "The image key is call_abc123 in plain text.";
        let result = strip_markdown_image_id(text, "call_abc123");
        assert!(!result.contains("call_abc123"));
        assert!(result.contains("The image key is"));
    }
}
