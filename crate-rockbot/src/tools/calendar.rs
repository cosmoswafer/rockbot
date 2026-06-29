use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, warn};
use webdav::{CaldavEvent, Reminder, ReminderAction, ReminderTrigger, WebDavClient, WebDavConfig, build_vevent_ics, quick_uid};

use crate::error::{Result, RockBotError};
use crate::tool::Tool;
use crate::validated::NonEmptyString;

#[derive(Debug, Deserialize)]
struct CalendarParams {
    action: NonEmptyString,
    #[serde(default)]
    room_id: Option<String>,
    #[serde(default)]
    webdav_dir: Option<String>,
    #[serde(default = "default_cal_start")]
    start: String,
    #[serde(default = "default_cal_end")]
    end: String,
    #[serde(default)]
    uid: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    dtstart: Option<String>,
    #[serde(default)]
    dtend: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    rrule: Option<String>,
    #[serde(default)]
    reminder_minutes: Option<i64>,
    #[serde(default = "default_timezone")]
    timezone: String,
    #[serde(default)]
    month_offset: i64,
}

fn default_cal_start() -> String { "20250101T000000Z".to_string() }
fn default_cal_end() -> String { "20990101T000000Z".to_string() }
fn default_timezone() -> String { "UTC".to_string() }

pub struct CalendarTool {
    client: WebDavClient,
    server_url: String,
    dav_path: String,
    username: String,
    room_calendars: Mutex<HashMap<String, String>>,
}

impl CalendarTool {
    pub fn from_config(client: WebDavClient, config: &WebDavConfig) -> Self {
        Self {
            client,
            server_url: config.url.to_string(),
            dav_path: config.dav_path.clone(),
            username: config.username.clone().into_inner(),
            room_calendars: Mutex::new(HashMap::new()),
        }
    }

    fn build_caldav_url(&self, calendar_name: &str) -> String {
        let url = self.server_url.trim_end_matches('/');
        let dav = self.dav_path.trim_matches('/');
        format!(
            "{url}/{dav}/calendars/{}/{}/",
            self.username, calendar_name
        )
    }

    async fn ensure_room_calendar(&self, room_id: &str, webdav_dir: &str) -> Option<String> {
        {
            let map = self.room_calendars.lock().unwrap();
            if let Some(name) = map.get(room_id) {
                return Some(self.build_caldav_url(name));
            }
        }

        let caldav_url = self.build_caldav_url(webdav_dir);

        match self
            .client
            .ensure_calendar(&caldav_url, webdav_dir)
            .await
        {
            Ok(()) => {
                debug!(
                    "Ensured calendar '{}' for room {}",
                    webdav_dir, room_id
                );
                let mut map = self.room_calendars.lock().unwrap();
                map.insert(room_id.to_string(), webdav_dir.to_string());
                Some(caldav_url)
            }
            Err(e) => {
                warn!(
                    "Failed to ensure calendar '{}' for room {}: {}",
                    webdav_dir, room_id, e
                );
                None
            }
        }
    }

    fn format_event(event: &CaldavEvent) -> String {
        let mut out = format!(
            "Event: {}\n  UID: {}\n  When: {} to {}\n",
            event.summary, event.uid, event.dtstart, event.dtend
        );
        if let Some(ref desc) = event.description {
            if !desc.is_empty() {
                out.push_str(&format!("  Description: {}\n", desc));
            }
        }
        if let Some(ref loc) = event.location {
            if !loc.is_empty() {
                out.push_str(&format!("  Location: {}\n", loc));
            }
        }
        if let Some(ref rrule) = event.rrule {
            if !rrule.is_empty() {
                out.push_str(&format!("  Recurrence: {}\n", rrule));
            }
        }
        for r in &event.reminders {
            out.push_str(&format!(
                "  Reminder: {} {}\n",
                r.action.as_str(), r.trigger.as_str()
            ));
        }
        out
    }

}

fn build_ics_for_event(params: &CalendarParams, uid: &str) -> Result<String> {
    let summary = params.summary.as_deref().ok_or_else(|| {
        RockBotError::ToolCallParse("calendar requires 'summary' field".into())
    })?;
    let dtstart = params.dtstart.as_deref().ok_or_else(|| {
        RockBotError::ToolCallParse("calendar requires 'dtstart' field".into())
    })?;
    let dtend = params.dtend.as_deref().ok_or_else(|| {
        RockBotError::ToolCallParse("calendar requires 'dtend' field".into())
    })?;
    let description = params.description.as_deref();
    let location = params.location.as_deref();
    let rrule = params.rrule.as_deref();
    let reminders = params
        .reminder_minutes
        .map(|mins| vec![Reminder {
            action: ReminderAction::try_new("DISPLAY".to_string()).expect("DISPLAY is a valid reminder action"),
            trigger: ReminderTrigger::try_new(format!("-PT{}M", mins)).expect("reminder trigger format is valid"),
        }])
        .unwrap_or_default();

    Ok(build_vevent_ics(
        uid,
        summary,
        dtstart,
        dtend,
        description,
        location,
        rrule,
        (!reminders.is_empty()).then_some(reminders.as_slice()),
    ))
}

fn build_ics_for_update(params: &CalendarParams, uid: &str, existing: &CaldavEvent) -> Result<String> {
    let summary = params.summary.as_deref().unwrap_or(&existing.summary);
    let dtstart = params.dtstart.as_deref().unwrap_or(&existing.dtstart);
    let dtend = params.dtend.as_deref().unwrap_or(&existing.dtend);
    let description = params.description.as_deref().or(existing.description.as_deref());
    let location = params.location.as_deref().or(existing.location.as_deref());
    let rrule = params.rrule.as_deref().or(existing.rrule.as_deref());

    let reminders = if params.reminder_minutes.is_some() {
        params.reminder_minutes.map(|mins| vec![Reminder {
            action: ReminderAction::try_new("DISPLAY".to_string()).expect("DISPLAY is a valid reminder action"),
            trigger: ReminderTrigger::try_new(format!("-PT{}M", mins)).expect("reminder trigger format is valid"),
        }]).unwrap_or_default()
    } else {
        existing.reminders.clone()
    };

    Ok(build_vevent_ics(
        uid,
        summary,
        dtstart,
        dtend,
        description,
        location,
        rrule,
        (!reminders.is_empty()).then_some(reminders.as_slice()),
    ))
}

fn generate_calendar_grid(month_offset: i64) -> String {
    use crate::utils::{civil_from_days, days_from_civil, weekday_index};

    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let today_days = secs / 86400;
    let (year, month, today_day) = civil_from_days(today_days);

    let target_month = month as i64 + month_offset;
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
            out.push_str(&format!("{:>2}*", d));
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

#[async_trait]
impl Tool for CalendarTool {
    fn name(&self) -> &str {
        "calendar"
    }

    fn description(&self) -> &str {
        "Manage calendar events on NextCloud CalDAV and display calendar grids. \
         Events are stored per-room — each room has its own calendar \
         auto-created on first use. \
         Actions: mini_calendar (display a month calendar grid), \
         list_events (list events in a date range), \
         get_event (fetch a single event by UID), \
         add_event (create a new event), update_event (modify an existing event by UID), \
         delete_event (remove an event by UID). \
         add_event requires summary, dtstart (ISO 8601, UTC), dtend (ISO 8601, UTC). \
         update_event uses merge semantics: specify only the fields you want to change; \
         omitted fields keep their existing values. \
         Optional for both: description, location, rrule (recurrence rule, RFC 5545), \
         reminder_minutes (e.g. 15). \
         mini_calendar accepts optional month_offset (0=current month, 1=next, -1=previous) \
         and timezone (default UTC). \
         All date/time values must be in UTC — use the Z suffix (e.g. 20260615T140000Z) \
         or omit seconds (e.g. 20260601T000000Z). Floating times (without Z) are not supported."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["mini_calendar", "list_events", "get_event", "add_event", "update_event", "delete_event"],
                    "description": "Calendar operation to perform"
                },
                "start": {
                    "type": "string",
                    "description": "Start of date range in ISO 8601 UTC (e.g. 20260601T000000Z). Used by list_events."
                },
                "end": {
                    "type": "string",
                    "description": "End of date range in ISO 8601 UTC. Used by list_events."
                },
                "uid": {
                    "type": "string",
                    "description": "Event UID. Required for update_event and delete_event."
                },
                "summary": {
                    "type": "string",
                    "description": "Event title/summary. Required for add_event and update_event."
                },
                "dtstart": {
                    "type": "string",
                    "description": "Event start in ISO 8601 UTC (e.g. 20260615T140000Z). Required for add_event."
                },
                "dtend": {
                    "type": "string",
                    "description": "Event end in ISO 8601 UTC. Required for add_event."
                },
                "description": {
                    "type": "string",
                    "description": "Optional event description/details."
                },
                "location": {
                    "type": "string",
                    "description": "Optional event location."
                },
                "rrule": {
                    "type": "string",
                    "description": "Optional recurrence rule in RFC 5545 format (e.g. FREQ=WEEKLY;BYDAY=MO)."
                },
                "reminder_minutes": {
                    "type": "integer",
                    "description": "Optional reminder in minutes before event (e.g. 15)."
                },
                "timezone": {
                    "type": "string",
                    "description": "IANA timezone name (e.g. Asia/Macau, America/New_York). Default: UTC. Used by mini_calendar."
                },
                "month_offset": {
                    "type": "integer",
                    "description": "Month offset for mini_calendar: 0=current month, 1=next month, -1=previous. Default: 0."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let params: CalendarParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse calendar arguments: {e}"))
        })?;

        let room_id = params.room_id.as_deref().unwrap_or("global");
        let webdav_dir = params.webdav_dir.as_deref().unwrap_or(room_id);

        let caldav_url = self
            .ensure_room_calendar(room_id, webdav_dir)
            .await
            .unwrap_or_else(|| self.build_caldav_url(webdav_dir));

        match params.action.as_str() {
            "list_events" => {
                debug!("calendar list_events: {} to {} (room={})", params.start, params.end, room_id);
                let events = self
                    .client
                    .list_events_by_date_range(&caldav_url, &params.start, &params.end)
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Calendar list failed: {e}")))?;

                if events.is_empty() {
                    Ok(format!("No events found between {} and {}.", params.start, params.end))
                } else {
                    let mut out = format!(
                        "{} event(s) between {} and {}:\n\n",
                        events.len(),
                        params.start,
                        params.end
                    );
                    for event in &events {
                        out.push_str(&Self::format_event(event));
                        out.push('\n');
                    }
                    Ok(out)
                }
            }
            "get_event" => {
                let uid = params.uid.as_deref().ok_or_else(|| {
                    RockBotError::ToolCallParse("calendar get_event requires 'uid' field".into())
                })?;
                let event = self
                    .client
                    .get_event(&caldav_url, uid)
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Calendar get failed: {e}")))?;
                Ok(Self::format_event(&event))
            }
            "add_event" => {
                let uid = quick_uid();
                let ics = build_ics_for_event(&params, &uid)?;
                debug!(
                    "calendar add_event: uid={} summary={:?} (room={})",
                    uid,
                    params.summary,
                    room_id,
                );
                self.client
                    .add_event(&caldav_url, &uid, &ics)
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Calendar add failed: {e}")))?;
                Ok(format!("Event created with UID: {}", uid))
            }
            "update_event" => {
                let uid = params.uid.as_deref().ok_or_else(|| {
                    RockBotError::ToolCallParse("calendar update_event requires 'uid' field".into())
                })?;
                let existing = self
                    .client
                    .fetch_event_by_uid(&caldav_url, uid)
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Calendar fetch failed: {e}")))?;
                let existing =
                    existing.ok_or_else(|| RockBotError::Provider(format!("Event not found: {uid}")))?;
                let ics = build_ics_for_update(&params, uid, &existing)?;
                self.client
                    .update_event(&caldav_url, uid, &ics, &existing.etag)
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Calendar update failed: {e}")))?;
                Ok(format!("Event updated: {}", uid))
            }
            "delete_event" => {
                let uid = params.uid.as_deref().ok_or_else(|| {
                    RockBotError::ToolCallParse("calendar delete_event requires 'uid' field".into())
                })?;
                self.client
                    .delete_event(&caldav_url, uid)
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Calendar delete failed: {e}")))?;
                Ok(format!("Event deleted: {}", uid))
            }
            "mini_calendar" => {
                let tz = params.timezone.as_str();
                let tz_display = if tz == "UTC" { "UTC".to_string() } else { format!("UTC ({})", tz) };
                let cal = generate_calendar_grid(params.month_offset);
                Ok(format!("Calendar ({}, month_offset={}):\n\n{}", tz_display, params.month_offset, cal))
            }
            other => Err(RockBotError::ToolCallParse(format!(
                "Unknown calendar action: {other}. Valid: mini_calendar, list_events, get_event, add_event, update_event, delete_event"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_tool() -> CalendarTool {
        let client = webdav::WebDavClient::new(
            "https://example.com/remote.php/dav/files/user/rockbot",
            "user",
            "pass",
        )
        .unwrap();
        CalendarTool {
            client,
            server_url: "https://example.com".into(),
            dav_path: "/remote.php/dav".into(),
            username: "user".into(),
            room_calendars: Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn test_calendar_tool_definition() {
        let tool = make_test_tool();
        assert_eq!(tool.name(), "calendar");
        assert!(tool.description().contains("calendar"));

        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("action"))
        );
        let actions = params["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), 6);
    }

    #[test]
    fn test_build_caldav_url() {
        let tool = make_test_tool();
        let url = tool.build_caldav_url("r-testroom");
        assert_eq!(
            url,
            "https://example.com/remote.php/dav/calendars/user/r-testroom/"
        );
    }

    #[test]
    fn test_build_caldav_url_no_dav_prefix() {
        let tool = CalendarTool {
            client: webdav::WebDavClient::new(
                "https://cloud.example.com/remote.php/dav/files/admin",
                "admin",
                "pass",
            )
            .unwrap(),
            server_url: "https://cloud.example.com".into(),
            dav_path: "/remote.php/dav".into(),
            username: "admin".into(),
            room_calendars: Mutex::new(HashMap::new()),
        };
        let url = tool.build_caldav_url("r-general");
        assert_eq!(
            url,
            "https://cloud.example.com/remote.php/dav/calendars/admin/r-general/"
        );
    }

    #[test]
    fn test_room_calendars_cached() {
        let tool = make_test_tool();
        {
            let mut map = tool.room_calendars.lock().unwrap();
            map.insert("room1".to_string(), "r-hr".to_string());
        }
        {
            let map = tool.room_calendars.lock().unwrap();
            assert_eq!(map.get("room1").unwrap(), "r-hr");
        }
    }

    #[test]
    fn test_calendar_tool_parameters_include_all_actions() {
        let tool = make_test_tool();
        let params = tool.parameters();
        let actions = params["properties"]["action"]["enum"].as_array().unwrap();
        assert!(actions.contains(&serde_json::json!("mini_calendar")));
        assert!(actions.contains(&serde_json::json!("list_events")));
        assert!(actions.contains(&serde_json::json!("get_event")));
        assert!(actions.contains(&serde_json::json!("add_event")));
        assert!(actions.contains(&serde_json::json!("update_event")));
        assert!(actions.contains(&serde_json::json!("delete_event")));
    }

    #[tokio::test]
    async fn test_execute_missing_action() {
        let tool = make_test_tool();
        let result = tool.execute(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_unknown_action() {
        let tool = make_test_tool();
        let result = tool.execute(r#"{"action": "unknown"}"#).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown calendar action"));
    }

    #[tokio::test]
    async fn test_execute_add_event_missing_summary() {
        let tool = make_test_tool();
        let result = tool
            .execute(
                r#"{"action": "add_event", "dtstart": "20260101T000000Z", "dtend": "20260101T010000Z"}"#,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_mini_calendar_default() {
        let tool = make_test_tool();
        let result = tool.execute(r#"{"action": "mini_calendar"}"#).await.unwrap();
        assert!(result.contains("Calendar (UTC, month_offset=0)"));
        assert!(result.contains("Mon Tue Wed Thu Fri Sat Sun"));
    }

    #[tokio::test]
    async fn test_execute_mini_calendar_with_offset() {
        let tool = make_test_tool();
        let result = tool.execute(r#"{"action": "mini_calendar", "month_offset": 1}"#).await.unwrap();
        assert!(result.contains("month_offset=1"));
    }

    #[tokio::test]
    async fn test_execute_mini_calendar_with_timezone() {
        let tool = make_test_tool();
        let result = tool.execute(r#"{"action": "mini_calendar", "timezone": "Asia/Macau"}"#).await.unwrap();
        assert!(result.contains("UTC (Asia/Macau)"));
    }

    #[test]
    fn test_generate_calendar_grid_basic() {
        let cal = generate_calendar_grid(0);
        assert!(!cal.is_empty());
        let months = [
            "January", "February", "March", "April", "May", "June",
            "July", "August", "September", "October", "November", "December",
        ];
        let has_month = months.iter().any(|m| cal.contains(m));
        assert!(has_month, "Calendar should contain a month name, got: {cal}");
    }
}
