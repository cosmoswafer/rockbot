use crate::types::{CaldavEvent, CaldavTodo, Reminder};

pub(crate) const CALENDAR_QUERY_EVENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:prop>
    <D:getetag/>
    <C:calendar-data/>
  </D:prop>
  <C:filter>
    <C:comp-filter name="VCALENDAR">
      <C:comp-filter name="VEVENT">
        <C:time-range start="START_PLACEHOLDER" end="END_PLACEHOLDER"/>
      </C:comp-filter>
    </C:comp-filter>
  </C:filter>
</C:calendar-query>"#;

pub(crate) const CALENDAR_QUERY_TODO_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:prop>
    <D:getetag/>
    <C:calendar-data/>
  </D:prop>
  <C:filter>
    <C:comp-filter name="VCALENDAR">
      <C:comp-filter name="VTODO">
        <C:comp-filter name="STATUS">
          <C:text-match negate-condition="yes">CANCELLED</C:text-match>
        </C:comp-filter>
      </C:comp-filter>
    </C:comp-filter>
  </C:filter>
</C:calendar-query>"#;

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CalDavMultiStatus {
    #[serde(rename = "response", default)]
    pub responses: Vec<CalDavResponse>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CalDavResponse {
    pub href: String,
    #[serde(rename = "propstat", default)]
    pub propstats: Vec<CalDavPropStat>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CalDavPropStat {
    pub prop: CalDavProp,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CalDavProp {
    #[serde(rename = "getetag", default)]
    pub getetag: Option<String>,
    #[serde(rename = "calendar-data", default)]
    pub calendar_data: Option<String>,
}

pub(crate) fn parse_vevents(ics: &str, href: &str, etag: &str) -> Vec<CaldavEvent> {
    let mut events = Vec::new();
    let lines: Vec<&str> = ics.lines().map(|l| l.trim()).collect();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("BEGIN:VEVENT") {
            let end = lines[i..]
                .iter()
                .position(|l| *l == "END:VEVENT")
                .unwrap_or(lines.len() - i);
            let vevent = &lines[i..=i + end];
            if let Some(event) = parse_single_vevent(vevent, href, etag) {
                events.push(event);
            }
            i += end + 1;
        } else {
            i += 1;
        }
    }

    events
}

pub(crate) fn parse_vtodos(ics: &str, href: &str) -> Vec<CaldavTodo> {
    let mut todos = Vec::new();
    let lines: Vec<&str> = ics.lines().map(|l| l.trim()).collect();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("BEGIN:VTODO") {
            let end = lines[i..]
                .iter()
                .position(|l| *l == "END:VTODO")
                .unwrap_or(lines.len() - i);
            let vtodo = &lines[i..=i + end];
            if let Some(todo) = parse_single_vtodo(vtodo, href) {
                todos.push(todo);
            }
            i += end + 1;
        } else {
            i += 1;
        }
    }

    todos
}

fn parse_single_vevent(lines: &[&str], href: &str, etag: &str) -> Option<CaldavEvent> {
    let uid = ical_value(lines, "UID")?;
    let summary = ical_value(lines, "SUMMARY").unwrap_or_default();
    let description = ical_value(lines, "DESCRIPTION");
    let location = ical_value(lines, "LOCATION");
    let dtstart = ical_value(lines, "DTSTART").unwrap_or_default();
    let dtend = ical_value(lines, "DTEND").unwrap_or_default();
    let rrule = ical_value(lines, "RRULE");
    let created = ical_value(lines, "CREATED").unwrap_or_default();
    let last_modified = ical_value(lines, "LAST-MODIFIED").unwrap_or_default();

    let mut reminders = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("BEGIN:VALARM") {
            let end = lines[i..]
                .iter()
                .position(|l| *l == "END:VALARM")
                .unwrap_or(lines.len() - i);
            let alarm = &lines[i..=i + end];
            let action = ical_value(alarm, "ACTION").unwrap_or_else(|| "DISPLAY".into());
            let trigger = ical_value(alarm, "TRIGGER").unwrap_or_default();
            if !trigger.is_empty() {
                reminders.push(Reminder { action, trigger });
            }
            i += end + 1;
        } else {
            i += 1;
        }
    }

    Some(CaldavEvent {
        uid,
        href: href.to_string(),
        etag: etag.to_string(),
        summary,
        description,
        location,
        dtstart,
        dtend,
        rrule,
        reminders,
        created,
        last_modified,
    })
}

fn parse_single_vtodo(lines: &[&str], href: &str) -> Option<CaldavTodo> {
    let uid = ical_value(lines, "UID")?;
    let summary = ical_value(lines, "SUMMARY").unwrap_or_default();
    let description = ical_value(lines, "DESCRIPTION");
    let priority = ical_value(lines, "PRIORITY")
        .and_then(|p| p.parse::<u8>().ok());
    let status = ical_value(lines, "STATUS").unwrap_or_else(|| "NEEDS-ACTION".into());
    let due = ical_value(lines, "DUE");
    let completed = ical_value(lines, "COMPLETED");
    let created = ical_value(lines, "CREATED").unwrap_or_default();

    Some(CaldavTodo {
        uid,
        href: href.to_string(),
        summary,
        description,
        priority,
        status,
        due,
        completed,
        created,
    })
}

fn ical_value(lines: &[&str], key: &str) -> Option<String> {
    let search_key = format!("{}:", key);
    for line in lines {
        if line.starts_with(&search_key) {
            let value = &line[search_key.len()..];
            return Some(unescape_ical(value));
        }
        // Handle folded lines (continuation starting with space)
        if line.starts_with(' ') && key.to_lowercase() == "trigger" {
            // Only handle trigger unfolding for now
            return Some(line.trim().to_string());
        }
    }
    None
}

fn unescape_ical(s: &str) -> String {
    s.replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
        .replace("\\n", "\n")
        .replace("\\N", "\n")
}

#[allow(clippy::too_many_arguments)]
pub fn build_vevent_ics(
    uid: &str,
    summary: &str,
    dtstart: &str,
    dtend: &str,
    description: Option<&str>,
    location: Option<&str>,
    rrule: Option<&str>,
    reminders: Option<&[Reminder]>,
) -> String {
    let mut ics = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//RockBot//NextCloud Calendar//EN\r\nBEGIN:VEVENT\r\nUID:{}\r\nDTSTART:{}\r\nDTEND:{}\r\nSUMMARY:{}\r\n",
        escape_ical(uid),
        escape_ical(dtstart),
        escape_ical(dtend),
        escape_ical(summary),
    );

    if let Some(desc) = description {
        if !desc.is_empty() {
            ics.push_str(&format!("DESCRIPTION:{}\r\n", escape_ical(desc)));
        }
    }
    if let Some(loc) = location {
        if !loc.is_empty() {
            ics.push_str(&format!("LOCATION:{}\r\n", escape_ical(loc)));
        }
    }
    if let Some(rule) = rrule {
        if !rule.is_empty() {
            ics.push_str(&format!("RRULE:{}\r\n", escape_ical(rule)));
        }
    }

    if let Some(rems) = reminders {
        for r in rems {
            ics.push_str("BEGIN:VALARM\r\n");
            ics.push_str(&format!("ACTION:{}\r\n", escape_ical(&r.action)));
            ics.push_str(&format!("TRIGGER:{}\r\n", escape_ical(&r.trigger)));
            ics.push_str(&format!(
                "DESCRIPTION:Reminder for {}\r\n",
                escape_ical(summary)
            ));
            ics.push_str("END:VALARM\r\n");
        }
    }

    ics.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    ics
}

fn escape_ical(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace(';', "\\;")
        .replace('\n', "\\n")
}

pub fn quick_uid() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{:08x}-{:04x}-{:04x}@rockbot",
        now.as_secs() as u32,
        now.subsec_millis() as u16,
        (now.subsec_micros() % 10000) as u16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vevent_simple() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:test-123\r\nDTSTART:20260615T140000Z\r\nDTEND:20260615T150000Z\r\nSUMMARY:Team standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_vevents(ics, "/cal/test-123.ics", "\"etag1\"");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].uid, "test-123");
        assert_eq!(events[0].summary, "Team standup");
        assert_eq!(events[0].dtstart, "20260615T140000Z");
    }

    #[test]
    fn test_parse_vevent_with_description_and_location() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:evt-1\r\nDTSTART:20260601T090000Z\r\nDTEND:20260601T100000Z\r\nSUMMARY:Sprint review\r\nDESCRIPTION:Review progress\\\\, discuss blockers\r\nLOCATION:Room 4B\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_vevents(ics, "/cal/evt-1.ics", "");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "Sprint review");
        assert_eq!(events[0].location.as_deref(), Some("Room 4B"));
        assert!(events[0].description.as_deref().unwrap().contains("Review"));
    }

    #[test]
    fn test_parse_vevent_with_reminder() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:remind-1\r\nDTSTART:20260615T140000Z\r\nDTEND:20260615T150000Z\r\nSUMMARY:Meeting\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Meeting in 15\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_vevents(ics, "/cal/remind-1.ics", "");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].reminders.len(), 1);
        assert_eq!(events[0].reminders[0].action, "DISPLAY");
        assert_eq!(events[0].reminders[0].trigger, "-PT15M");
    }

    #[test]
    fn test_parse_vtodo() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:todo-1\r\nSUMMARY:Buy groceries\r\nPRIORITY:5\r\nSTATUS:NEEDS-ACTION\r\nDUE:20260620T120000Z\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let todos = parse_vtodos(ics, "/cal/todo-1.ics");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].uid, "todo-1");
        assert_eq!(todos[0].summary, "Buy groceries");
        assert_eq!(todos[0].priority, Some(5));
        assert_eq!(todos[0].status, "NEEDS-ACTION");
    }

    #[test]
    fn test_build_vevent_ics() {
        let ics = build_vevent_ics(
            "evt-1",
            "Test event",
            "20260615T140000Z",
            "20260615T150000Z",
            Some("A test event description"),
            Some("Room 1"),
            None,
            None,
        );
        assert!(ics.contains("UID:evt-1"));
        assert!(ics.contains("SUMMARY:Test event"));
        assert!(ics.contains("DESCRIPTION:A test event description"));
        assert!(ics.contains("LOCATION:Room 1"));
    }

    #[test]
    fn test_build_vevent_ics_with_reminder() {
        let reminders = vec![Reminder {
            action: "DISPLAY".into(),
            trigger: "-PT15M".into(),
        }];
        let ics = build_vevent_ics(
            "evt-2",
            "With alarm",
            "20260615T090000Z",
            "20260615T100000Z",
            None,
            None,
            None,
            Some(&reminders),
        );
        assert!(ics.contains("BEGIN:VALARM"));
        assert!(ics.contains("ACTION:DISPLAY"));
        assert!(ics.contains("TRIGGER:-PT15M"));
    }

    #[test]
    fn test_escape_ical() {
        let input = "test, with; special\\chars\n";
        let escaped = escape_ical(input);
        assert!(escaped.contains("\\,"));
        assert!(escaped.contains("\\;"));
        assert!(escaped.contains("\\\\"));
        assert!(escaped.contains("\\n"));
    }

    #[test]
    fn test_unescape_ical() {
        let input = "test\\, with\\; special\\\\chars\\n";
        let unescaped = unescape_ical(input);
        assert_eq!(unescaped, "test, with; special\\chars\n");
    }
}
