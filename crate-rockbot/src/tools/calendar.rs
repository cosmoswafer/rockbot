use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;
use webdav::{CaldavEvent, CaldavTodo, Reminder, WebDavClient, WebDavConfig, build_vevent_ics, quick_uid};

use crate::error::{Result, RockBotError};
use crate::tool::Tool;

pub struct CalendarTool {
    client: WebDavClient,
    calendar_name: String,
    server_url: String,
    username: String,
}

impl CalendarTool {
    pub fn from_config(client: WebDavClient, config: &WebDavConfig) -> Option<Self> {
        config.calendar_name.as_ref().map(|name| Self {
            client,
            calendar_name: name.clone(),
            server_url: config.url.clone(),
            username: config.username.clone(),
        })
    }

    fn caldav_url(&self) -> String {
        let origin = if let Some(pos) = self.server_url.find("/remote.php/dav/files/") {
            &self.server_url[..pos]
        } else {
            self.server_url.trim_end_matches('/')
        };
        format!(
            "{}/remote.php/dav/calendars/{}/{}/",
            origin, self.username, self.calendar_name
        )
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

    fn format_todo(todo: &CaldavTodo) -> String {
        let mut out = format!(
            "Todo: {}\n  UID: {}\n  Status: {}\n",
            todo.summary, todo.uid, todo.status
        );
        if let Some(ref desc) = todo.description {
            if !desc.is_empty() {
                out.push_str(&format!("  Description: {}\n", desc));
            }
        }
        if let Some(due) = &todo.due {
            out.push_str(&format!("  Due: {}\n", due));
        }
        if let Some(pri) = todo.priority {
            out.push_str(&format!("  Priority: {}\n", pri));
        }
        out
    }
}

fn build_ics_for_event(args: &serde_json::Value, uid: &str) -> Result<String> {
    let summary = args
        .get("summary")
        .and_then(|s| s.as_str())
        .ok_or_else(|| RockBotError::ToolCallParse("calendar requires 'summary' field".into()))?;

    let dtstart = args
        .get("dtstart")
        .and_then(|s| s.as_str())
        .ok_or_else(|| {
            RockBotError::ToolCallParse(
                "calendar requires 'dtstart' (ISO 8601) field".into(),
            )
        })?;

    let dtend = args
        .get("dtend")
        .and_then(|s| s.as_str())
        .ok_or_else(|| {
            RockBotError::ToolCallParse("calendar requires 'dtend' (ISO 8601) field".into())
        })?;

    let description = args.get("description").and_then(|d| d.as_str());
    let location = args.get("location").and_then(|l| l.as_str());

    let reminders = args
        .get("reminder_minutes")
        .and_then(|m| m.as_i64())
        .map(|mins| {
            vec![Reminder {
                action: "DISPLAY".into(),
                trigger: format!("-PT{}M", mins),
            }]
        });

    Ok(build_vevent_ics(
        uid,
        summary,
        dtstart,
        dtend,
        description,
        location,
        reminders.as_deref(),
    ))
}

#[async_trait]
impl Tool for CalendarTool {
    fn name(&self) -> &str {
        "calendar"
    }

    fn description(&self) -> &str {
        "Manage calendar events and todo tasks on NextCloud CalDAV. \
         Actions: list_events (list events in a date range), \
         add_event (create a new event), update_event (modify an existing event by UID), \
         delete_event (remove an event by UID), list_todos (list active todo items). \
         Events require summary, dtstart (ISO 8601), dtend (ISO 8601). \
         Optional: description, location, reminder_minutes (e.g. 15)."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list_events", "add_event", "update_event", "delete_event", "list_todos"],
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

                debug!("calendar list_events: {} to {}", start, end);
                let events = self
                    .client
                    .list_events_by_date_range(&self.caldav_url(), start, end)
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
            "add_event" => {
                let uid = quick_uid();
                let ics = build_ics_for_event(&args, &uid)?;
                debug!(
                    "calendar add_event: uid={} summary={:?}",
                    uid,
                    args.get("summary")
                );
                self.client
                    .add_event(&self.caldav_url(), &uid, &ics)
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
                    .fetch_event_by_uid(&self.caldav_url(), uid)
                    .await
                    .map_err(|e| {
                        RockBotError::Provider(format!("Calendar fetch failed: {e}"))
                    })?;

                let existing =
                    existing.ok_or_else(|| RockBotError::Provider(format!("Event not found: {uid}")))?;

                let ics = build_ics_for_event(&args, uid)?;
                self.client
                    .update_event(&self.caldav_url(), uid, &ics, &existing.etag)
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
                    .delete_event(&self.caldav_url(), uid)
                    .await
                    .map_err(|e| {
                        RockBotError::Provider(format!("Calendar delete failed: {e}"))
                    })?;
                Ok(format!("Event deleted: {}", uid))
            }
            "list_todos" => {
                debug!("calendar list_todos");
                let todos = self
                    .client
                    .list_todos(&self.caldav_url())
                    .await
                    .map_err(|e| RockBotError::Provider(format!("Todo list failed: {e}")))?;

                if todos.is_empty() {
                    Ok("No active todo items found.".to_string())
                } else {
                    let mut out = format!("{} active todo(s):\n\n", todos.len());
                    for todo in &todos {
                        out.push_str(&Self::format_todo(todo));
                        out.push('\n');
                    }
                    Ok(out)
                }
            }
            other => Err(RockBotError::ToolCallParse(format!(
                "Unknown calendar action: {other}. Valid: list_events, add_event, update_event, delete_event, list_todos"
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
            calendar_name: "personal".into(),
            server_url: "https://example.com/remote.php/dav/files/user".into(),
            username: "user".into(),
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
    fn test_caldav_url_construction() {
        let tool = make_test_tool();
        let url = tool.caldav_url();
        assert_eq!(
            url,
            "https://example.com/remote.php/dav/calendars/user/personal/"
        );
    }

    #[test]
    fn test_caldav_url_no_dav_prefix() {
        let tool = CalendarTool {
            client: webdav::WebDavClient::new("https://cloud.example.com", "admin", "pass").unwrap(),
            calendar_name: "work".into(),
            server_url: "https://cloud.example.com".into(),
            username: "admin".into(),
        };
        let url = tool.caldav_url();
        assert_eq!(
            url,
            "https://cloud.example.com/remote.php/dav/calendars/admin/work/"
        );
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
