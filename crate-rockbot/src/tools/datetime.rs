use async_trait::async_trait;
use serde_json::Value;

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

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

fn now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = civil_from_days(days_since_epoch as i64);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn now_human() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = civil_from_days(days_since_epoch as i64);
    let weekday = [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ][(days_since_epoch as usize + 4) % 7];

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (a {})",
        year, month, day, hours, minutes, seconds, weekday
    )
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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

#[async_trait]
impl Tool for DateTimeTool {
    fn name(&self) -> &str {
        "datetime"
    }

    fn description(&self) -> &str {
        "Get the current UTC date and time. Returns ISO 8601 timestamp, \
         human-readable date with weekday, and Unix epoch seconds."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["iso", "human", "unix", "full"],
                    "description": "Output format: iso (ISO 8601), human (readable with weekday), unix (epoch seconds), full (all three). Default: full"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = if arguments.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(arguments).map_err(|e| {
                RockBotError::ToolCallParse(format!("Failed to parse datetime arguments: {e}"))
            })?
        };

        let format = args
            .get("format")
            .and_then(|f| f.as_str())
            .unwrap_or("full");

        match format {
            "iso" => Ok(now_iso()),
            "human" => Ok(now_human()),
            "unix" => Ok(now_unix().to_string()),
            _ => Ok(format!("{}\n{}\n{}", now_iso(), now_human(), now_unix())),
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

    #[tokio::test]
    async fn test_execute_full_format() {
        let tool = DateTimeTool::new();
        let result = tool.execute("{}").await.unwrap();
        assert!(result.contains('T'));
        assert!(result.contains("UTC"));
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
    async fn test_execute_empty_args() {
        let tool = DateTimeTool::new();
        let result = tool.execute("").await.unwrap();
        assert!(result.len() > 20);
    }
}
