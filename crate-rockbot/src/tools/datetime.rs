use async_trait::async_trait;
use serde::Deserialize;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

#[derive(Debug, Deserialize)]
struct DateTimeParams {
    #[serde(default = "default_format")]
    format: String,
    #[serde(default)]
    week_offset: i64,
}

fn default_format() -> String {
    "full".to_string()
}

pub struct DateTimeTool;

impl DateTimeTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DateTimeTool {
    fn default() -> Self {
        Self
    }
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn days_since_epoch() -> i64 {
    now_unix_secs() / 86400
}

fn now_iso() -> String {
    let secs = now_unix_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = civil_from_days(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn now_human() -> String {
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

fn weekday_name(days: i64) -> &'static str {
    const WEEKDAYS: [&str; 7] = [
        "Thursday", "Friday", "Saturday",
        "Sunday", "Monday", "Tuesday", "Wednesday",
    ];
    let idx = days.rem_euclid(7);
    WEEKDAYS[idx as usize]
}

fn weekday_index(days: i64) -> i64 {
    // Returns 0=Monday, 6=Sunday
    (days + 3) % 7
}

fn iso_week_number(days: i64) -> u32 {
    let (year, _month, _day) = civil_from_days(days);

    let jan4_days = days_from_civil(year, 1, 4);
    let jan4_wday = weekday_index(jan4_days);

    let week1_start = jan4_days - jan4_wday;

    if days < week1_start {
        let prev_jan4 = days_from_civil(year - 1, 1, 4);
        let prev_jan4_wday = weekday_index(prev_jan4);
        let prev_week1_start = prev_jan4 - prev_jan4_wday;
        ((days - prev_week1_start) / 7 + 1) as u32
    } else {
        let week_num = ((days - week1_start) / 7 + 1) as u32;
        if week_num > 52 {
            let next_jan4 = days_from_civil(year + 1, 1, 4);
            let next_jan4_wday = weekday_index(next_jan4);
            let next_week1_start = next_jan4 - next_jan4_wday;
            if days >= next_week1_start {
                return 1;
            }
        }
        week_num
    }
}

fn now_weekdays(week_offset: i64) -> String {
    let base_days = days_since_epoch();
    let current_wday = weekday_index(base_days);

    let monday_days = base_days - current_wday + week_offset * 7;
    let mut out = String::new();
    const NAMES: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    for (i, name) in NAMES.iter().enumerate() {
        let day = monday_days + i as i64;
        let (y, m, d) = civil_from_days(day);
        if i > 0 {
            out.push('\n');
        }
        let marker = if day == base_days { " *" } else { "" };
        out.push_str(&format!("{}: {:04}-{:02}-{:02}{}", name, y, m, d, marker));
    }
    out
}

fn now_calendar(week_offset: i64) -> String {
    let secs = now_unix_secs();
    let today_days = secs / 86400;
    let (year, month, today_day) = civil_from_days(today_days);

    let target_month = month as i64 + week_offset;
    let (cal_year, cal_month) = if target_month > 12 {
        (year + (target_month - 1) / 12, ((target_month - 1) % 12 + 1) as u32)
    } else if target_month < 1 {
        let yr_adj = target_month / 12 - 1;
        let m = (target_month % 12 + 12) % 12;
        (year + yr_adj, if m == 0 { 12 } else { m as u32 })
    } else {
        (year, target_month as u32)
    };

    const MONTH_NAMES: [&str; 12] = [
        "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December",
    ];

    let first_day = days_from_civil(cal_year, cal_month, 1);
    let first_wday = weekday_index(first_day);
    let days_in_month = if cal_month == 12 {
        days_from_civil(cal_year + 1, 1, 1) - first_day
    } else {
        days_from_civil(cal_year, cal_month + 1, 1) - first_day
    };

    let mut out = String::new();
    out.push_str(&format!("{} {}\n", MONTH_NAMES[cal_month as usize - 1], cal_year));
    out.push_str("Mon Tue Wed Thu Fri Sat Sun\n");

    for _ in 0..first_wday {
        out.push_str("    ");
    }

    for d in 1..=days_in_month {
        let is_today = cal_year == year && cal_month == month && d == today_day as i64;
        if is_today {
            out.push_str(&format!("\x1b[7m{:>2}\x1b[0m", d));
        } else {
            out.push_str(&format!("{:>2}", d));
        }
        let col = (first_wday + d - 1) % 7;
        if col == 6 {
            out.push('\n');
        } else {
            out.push(' ');
        }
    }
    if (first_wday + days_in_month - 1) % 7 != 6 {
        out.push('\n');
    }

    out.trim_end().to_string()
}

fn now_week_number() -> String {
    let days = days_since_epoch();
    let week = iso_week_number(days);
    week.to_string()
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
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

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m <= 2 { m + 9 } else { m - 3 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

#[async_trait]
impl Tool for DateTimeTool {
    fn name(&self) -> &str {
        "datetime"
    }

    fn description(&self) -> &str {
        "Get the current UTC date and time. Returns ISO 8601 timestamp, \
         human-readable date with weekday, Unix epoch seconds, calendar month view, \
         week number (ISO 8601), or weekday list. Supports week_offset for prev/next week views."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["iso", "human", "unix", "full", "calendar", "weekdays", "week_number"],
                    "description": "Output format: iso (ISO 8601), human (readable with weekday), unix (epoch seconds), calendar (month grid), weekdays (list of weekdays with dates), week_number (ISO week number), full (all). Default: full"
                },
                "week_offset": {
                    "type": "integer",
                    "description": "Offset for calendar/weekdays format: 0=current week/month, 1=next, -1=previous. Default: 0"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let params: DateTimeParams = if arguments.is_empty() {
            DateTimeParams { format: "full".to_string(), week_offset: 0 }
        } else {
            serde_json::from_str(arguments).map_err(|e| {
                RockBotError::ToolCallParse(format!("Failed to parse datetime arguments: {e}"))
            })?
        };

        match params.format.as_str() {
            "iso" => Ok(now_iso()),
            "human" => Ok(now_human()),
            "unix" => Ok(now_unix_secs().to_string()),
            "calendar" => Ok(now_calendar(params.week_offset)),
            "weekdays" => Ok(now_weekdays(params.week_offset)),
            "week_number" => Ok(now_week_number()),
            _ => {
                let iso = now_iso();
                let human = now_human();
                let unix = now_unix_secs().to_string();
                let calendar = now_calendar(0);
                let weekdays = now_weekdays(0);
                let week_num = now_week_number();
                Ok(format!(
                    "{}\n{}\n{}\nWeek Number: {}\n\nWeekdays:\n{}\n\nCalendar:\n{}",
                    iso, human, unix, week_num, weekdays, calendar
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datetime_tool_definition() {
        let tool = DateTimeTool::new();
        assert_eq!(tool.name(), "datetime");
        assert!(tool.description().contains("current UTC"));
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
    }

    #[test]
    fn test_civil_from_days_epoch() {
        let (y, m, d) = civil_from_days(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_civil_from_days_known() {
        let (y, m, d) = civil_from_days(20089);
        assert_eq!((y, m, d), (2025, 1, 1));
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
    fn test_iso_week_number_known() {
        let jan1_2026 = days_from_civil(2026, 1, 1);
        assert_eq!(iso_week_number(jan1_2026), 1);
        let dec31_2025 = days_from_civil(2025, 12, 31);
        assert_eq!(iso_week_number(dec31_2025), 1);
        let jan1_2025 = days_from_civil(2025, 1, 1);
        assert_eq!(iso_week_number(jan1_2025), 1);
    }

    #[test]
    fn test_calendar_output() {
        let cal = now_calendar(0);
        assert!(!cal.is_empty());
        let months = [
            "January", "February", "March", "April", "May", "June",
            "July", "August", "September", "October", "November", "December",
        ];
        let has_month = months.iter().any(|m| cal.contains(m));
        assert!(has_month, "Calendar should contain a month name, got: {cal}");
    }

    #[test]
    fn test_weekdays_output() {
        let wd = now_weekdays(0);
        assert!(wd.contains("Monday"));
        assert!(wd.contains("Sunday"));
    }

    #[tokio::test]
    async fn test_execute_full_format() {
        let tool = DateTimeTool::new();
        let result = tool.execute("{}").await.unwrap();
        assert!(result.contains('T'));
        assert!(result.contains("UTC"));
        assert!(result.contains("Week Number"));
        assert!(result.contains("Weekdays"));
        assert!(result.contains("Calendar"));
    }

    #[tokio::test]
    async fn test_execute_iso_format() {
        let tool = DateTimeTool::new();
        let result = tool.execute(r#"{"format":"iso"}"#).await.unwrap();
        assert!(result.contains('T'));
        assert!(result.ends_with('Z'));
    }

    #[tokio::test]
    async fn test_execute_human_format() {
        let tool = DateTimeTool::new();
        let result = tool.execute(r#"{"format":"human"}"#).await.unwrap();
        assert!(result.contains("UTC"));
        assert!(result.contains('('));
    }

    #[tokio::test]
    async fn test_execute_unix_format() {
        let tool = DateTimeTool::new();
        let result = tool.execute(r#"{"format":"unix"}"#).await.unwrap();
        let ts: i64 = result.trim().parse().unwrap();
        assert!(ts > 1_700_000_000);
    }

    #[tokio::test]
    async fn test_execute_calendar_format() {
        let tool = DateTimeTool::new();
        let result = tool.execute(r#"{"format":"calendar"}"#).await.unwrap();
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_execute_weekdays_format() {
        let tool = DateTimeTool::new();
        let result = tool.execute(r#"{"format":"weekdays"}"#).await.unwrap();
        assert!(result.contains("Monday"));
        assert!(result.contains("Sunday"));
    }

    #[tokio::test]
    async fn test_execute_week_number_format() {
        let tool = DateTimeTool::new();
        let result = tool.execute(r#"{"format":"week_number"}"#).await.unwrap();
        let wn: u32 = result.trim().parse().unwrap();
        assert!((1..=53).contains(&wn));
    }

    #[tokio::test]
    async fn test_execute_empty_args() {
        let tool = DateTimeTool::new();
        let result = tool.execute("").await.unwrap();
        assert!(result.len() > 20);
    }

    #[tokio::test]
    async fn test_execute_calendar_with_offset() {
        let tool = DateTimeTool::new();
        let result = tool.execute(r#"{"format":"calendar","week_offset":1}"#).await.unwrap();
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_execute_weekdays_with_offset() {
        let tool = DateTimeTool::new();
        let result = tool.execute(r#"{"format":"weekdays","week_offset":1}"#).await.unwrap();
        assert!(result.contains("Monday"));
    }
}
