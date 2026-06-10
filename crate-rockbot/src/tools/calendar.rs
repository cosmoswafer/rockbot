use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, warn};
use webdav::{CaldavEvent, Reminder, WebDavClient, WebDavConfig, build_vevent_ics, quick_uid};

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

pub struct CalendarTool {
    client: WebDavClient,
    server_url: String,
    username: String,
    room_calendars: Mutex<HashMap<String, String>>,
}

impl CalendarTool {
    pub fn from_config(client: WebDavClient, config: &WebDavConfig) -> Self {
        Self {
            client,
            server_url: config.url.clone(),
            username: config.username.clone(),
            room_calendars: Mutex::new(HashMap::new()),
        }
    }

    fn server_origin(&self) -> String {
        if let Some(pos) = self.server_url.find("/remote.php/dav/") {
            self.server_url[..pos].to_string()
        } else {
            self.server_url.trim_end_matches('/').to_string()
        }
    }

    fn build_caldav_url(&self, calendar_name: &str) -> String {
        let origin = self.server_origin();
        format!(
            "{}/remote.php/dav/calendars/{}/{}/",
            origin, self.username, calendar_name
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
                r.action, r.trigger
            ));
        }
        out
    }

}

fn build_ics_for_event(args: &serde_json::Value, uid: &str) -> Result<String> {
    let summary = required_str(args, "summary")?;
    let dtstart = required_str(args, "dtstart")?;
    let dtend = required_str(args, "dtend")?;
    let description = args.get("description").and_then(|d| d.as_str());
    let location = args.get("location").and_then(|l| l.as_str());
    let rrule = args.get("rrule").and_then(|r| r.as_str());
    let reminders = parse_reminders(args);

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

fn build_ics_for_update(args: &serde_json::Value, uid: &str, existing: &CaldavEvent) -> Result<String> {
    let summary = args
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or(&existing.summary);
    let dtstart = args
        .get("dtstart")
        .and_then(|s| s.as_str())
        .unwrap_or(&existing.dtstart);
    let dtend = args
        .get("dtend")
        .and_then(|s| s.as_str())
        .unwrap_or(&existing.dtend);
    let description = args
        .get("description")
        .and_then(|d| d.as_str())
        .or(existing.description.as_deref());
    let location = args
        .get("location")
        .and_then(|l| l.as_str())
        .or(existing.location.as_deref());
    let rrule = args
        .get("rrule")
        .and_then(|r| r.as_str())
        .or(existing.rrule.as_deref());

    let reminders = if args.get("reminder_minutes").is_some() {
        parse_reminders(args)
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

fn required_str<'a>(args: &'a serde_json::Value, field: &str) -> Result<&'a str> {
    args.get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            RockBotError::ToolCallParse(
                format!("calendar requires '{field}' field")
            )
        })
}

fn parse_reminders(args: &serde_json::Value) -> Vec<Reminder> {
    args.get("reminder_minutes")
        .and_then(|m| m.as_i64())
        .map(|mins| {
            vec![Reminder {
                action: "DISPLAY".into(),
                trigger: format!("-PT{}M", mins),
            }]
        })
        .unwrap_or_default()
}

#[async_trait]
impl Tool for CalendarTool {
    fn name(&self) -> &str {
        "calendar"
    }

    fn description(&self) -> &str {
        "Manage calendar events on NextCloud CalDAV. \
         Events are stored per-room — each room has its own calendar \
         auto-created on first use. \
         Actions: list_events (list events in a date range), \
         get_event (fetch a single event by UID), \
         add_event (create a new event), update_event (modify an existing event by UID), \
         delete_event (remove an event by UID). \
         add_event requires summary, dtstart (ISO 8601), dtend (ISO 8601). \
         update_event uses merge semantics: specify only the fields you want to change; \
         omitted fields keep their existing values. \
         Optional for both: description, location, rrule (recurrence rule, RFC 5545), \
         reminder_minutes (e.g. 15)."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list_events", "get_event", "add_event", "update_event", "delete_event"],
                    "description": "Calendar operation to perform"
                },
                "start": {
                    "type": "string",
                    "description": "Start date/time in ISO 8601 (e.g. 20260601T000000Z). Used by list_events."
                },
                "end": {
                    "type": "string",
                    "description": "End date/time in ISO 8601. Used by list_events."
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
                    "description": "Event start in ISO 8601 (e.g. 20260615T140000Z). Required for add_event."
                },
                "dtend": {
                    "type": "string",
                    "description": "Event end in ISO 8601. Required for add_event."
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
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse calendar arguments: {e}"))
        })?;

        let room_id = args
            .get("room_id")
            .and_then(|r| r.as_str())
            .unwrap_or("global");

        let webdav_dir = args
            .get("webdav_dir")
            .and_then(|d| d.as_str())
            .unwrap_or(room_id);

        let caldav_url = self
            .ensure_room_calendar(room_id, webdav_dir)
            .await
            .unwrap_or_else(|| self.build_caldav_url(webdav_dir));

        let action = args
            .get("action")
            .and_then(|a| a.as_str())
            .ok_or_else(|| {
                RockBotError::ToolCallParse("calendar requires 'action' field".into())
            })?;

        match action {
            "list_events" => {
                let start = args
                    .get("start")
                    .and_then(|s| s.as_str())
                    .unwrap_or("20250101T000000Z");
                let end = args
                    .get("end")
                    .and_then(|s| s.as_str())
                    .unwrap_or("20990101T000000Z");

                debug!("calendar list_events: {} to {} (room={})", start, end, room_id);
                let events = self
                    .client
                    .list_events_by_date_range(&caldav_url, start, end)
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Calendar list failed: {e}")))?;

                if events.is_empty() {
                    Ok(format!("No events found between {} and {}.", start, end))
                } else {
                    let mut out = format!(
                        "{} event(s) between {} and {}:\n\n",
                        events.len(),
                        start,
                        end
                    );
                    for event in &events {
                        out.push_str(&Self::format_event(event));
                        out.push('\n');
                    }
                    Ok(out)
                }
            }
            "get_event" => {
                let uid = args
                    .get("uid")
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| {
                        RockBotError::ToolCallParse(
                            "calendar get_event requires 'uid' field".into(),
                        )
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
                let ics = build_ics_for_event(&args, &uid)?;
                debug!(
                    "calendar add_event: uid={} summary={:?} (room={})",
                    uid,
                    args.get("summary"),
                    room_id,
                );
                self.client
                    .add_event(&caldav_url, &uid, &ics)
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Calendar add failed: {e}")))?;
                Ok(format!("Event created with UID: {}", uid))
            }
            "update_event" => {
                let uid = args
                    .get("uid")
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| {
                        RockBotError::ToolCallParse(
                            "calendar update_event requires 'uid' field".into(),
                        )
                    })?;

                let existing = self
                    .client
                    .fetch_event_by_uid(&caldav_url, uid)
                    .await
                    .map_err(|e| {
                        RockBotError::Provider(format!("Calendar fetch failed: {e}"))
                    })?;

                let existing =
                    existing.ok_or_else(|| RockBotError::Provider(format!("Event not found: {uid}")))?;

                let ics = build_ics_for_update(&args, uid, &existing)?;
                self.client
                    .update_event(&caldav_url, uid, &ics, &existing.etag)
                    .await
                    .map_err(|e| {
                        RockBotError::Provider(format!("Calendar update failed: {e}"))
                    })?;
                Ok(format!("Event updated: {}", uid))
            }
            "delete_event" => {
                let uid = args
                    .get("uid")
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| {
                        RockBotError::ToolCallParse(
                            "calendar delete_event requires 'uid' field".into(),
                        )
                    })?;
                self.client
                    .delete_event(&caldav_url, uid)
                    .await
                    .map_err(|e| {
                        RockBotError::Provider(format!("Calendar delete failed: {e}"))
                    })?;
                Ok(format!("Event deleted: {}", uid))
            }
            other => Err(RockBotError::ToolCallParse(format!(
                "Unknown calendar action: {other}. Valid: list_events, get_event, add_event, update_event, delete_event"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_tool() -> CalendarTool {
        let client = webdav::WebDavClient::new("https://example.com", "user", "pass").unwrap();
        CalendarTool {
            client,
            server_url: "https://example.com/remote.php/dav/files/user".into(),
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
        assert_eq!(actions.len(), 5);
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
            client: webdav::WebDavClient::new("https://cloud.example.com", "admin", "pass").unwrap(),
            server_url: "https://cloud.example.com".into(),
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
}
