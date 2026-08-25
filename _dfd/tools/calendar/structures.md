# Calendar — Shared Structures

## 1. Overview

CalDAV event access wrapping NextCloud's calendar service. Supports listing
events by date range, create/read/update/delete individual events with
iCalendar (RFC 5545) `VEVENT` payloads, `VALARM` reminders, and a
`mini_calendar` action that renders a text-based month calendar grid (no
CalDAV calls — pure computation, replaces the former standalone datetime
tool's calendar view).

**Scope**: Calendar events are **per-room** — each RocketChat room gets its own
NextCloud calendar, auto-created on first use via CalDAV `MKCALENDAR`. The
calendar name is `{webdav_dir}` (matching the WebDAV directory name,
e.g. `r-General`, `d-bob`), stored under the configured user's CalDAV
calendar home (`/remote.php/dav/calendars/{username}/`). Events from
different rooms are fully isolated.

> **Note:** The `timezone` parameter is accepted for forward compatibility
> but the current implementation always computes dates in UTC. The LLM is
> expected to perform any necessary timezone offset arithmetic itself using
> the UTC time injected into the system prompt.

- Upstream: [Configuration Management](../../infra/config/main-path.md) provides `WebDavConfig`
  (server URL, credentials)
- Downstream: [Agent Harness](../../agent/agent-harness/agent-loop.md) injects `room_id` + `webdav_dir`
  into calendar tool arguments. `room_id` is used as the cache key in
  `room_calendars`, while `webdav_dir` names the per-room calendar
  (auto-created on first use)

## 3. Data Structures

#### `CalendarParams`

Deserializable from JSON tool-call arguments. All fields are optional except
`action`.

| Field              | Type             | Notes                                                   |
| ------------------ | ---------------- | ------------------------------------------------------- |
| `action`           | `NonEmptyString` | `"mini_calendar"`, `"list_events"`, `"get_event"`, `"add_event"`, `"update_event"`, or `"delete_event"` |
| `room_id`          | `Option<String>` | Fallback `"global"`                                     |
| `webdav_dir`       | `Option<String>` | Calendar name; defaults to `room_id`                    |
| `start`            | `String`         | ISO 8601 UTC range start for `list_events`. Default `20250101T000000Z` |
| `end`              | `String`         | ISO 8601 UTC range end. Default `20990101T000000Z`      |
| `uid`              | `Option<String>` | Required for `get_event`, `update_event`, `delete_event` |
| `summary`          | `Option<String>` | Event title; required for `add_event`                   |
| `dtstart`          | `Option<String>` | ISO 8601 UTC start; required for `add_event`            |
| `dtend`            | `Option<String>` | ISO 8601 UTC end; required for `add_event`              |
| `description`      | `Option<String>` | Event details                                           |
| `location`         | `Option<String>` | Event venue                                             |
| `rrule`            | `Option<String>` | RFC 5545 recurrence rule                                |
| `reminder_minutes` | `Option<i64>`    | Minutes before event for `VALARM`                       |
| `timezone`         | `String`         | IANA timezone name (e.g. `Asia/Macau`). Default `"UTC"`. Used by `mini_calendar` |
| `month_offset`     | `i64`            | Month offset for `mini_calendar`: 0=current, 1=next, -1=previous. Default 0 |

#### `mini_calendar` Action

Pure computation — no CalDAV calls. Renders a text-based month calendar grid
using Howard Hinnant's civil date algorithm (from `utils.rs`). The current
day is marked with `*`. Example output:

```
June 2026
Mon Tue Wed Thu Fri Sat Sun
  1    2    3    4    5    6    7
  8    9   10*  11   12   13   14
 15   16   17   18   19   20   21
 22   23   24   25   26   27   28
 29   30
```

The `timezone` parameter is accepted and echoed in the header for
readability, but the calendar grid itself is always computed in UTC.
`month_offset` allows browsing forward/backward up to 12 months.

#### `CaldavEvent`

CalDAV event resource represented as a parsed iCalendar `VEVENT` (RFC 5545).
Stored as `{uid}.ics` within the calendar collection.

| Field           | Type             | Notes                                   |
| --------------- | ---------------- | --------------------------------------- |
| `uid`           | `String`         | Globally unique event identifier        |
| `href`          | `String`         | Full CalDAV href to `{uid}.ics`         |
| `etag`          | `String`         | Opaque tag for conditional updates      |
| `summary`       | `String`         | Event title/name                        |
| `description`   | `Option<String>` | Event details/notes                     |
| `location`      | `Option<String>` | Event venue/place                       |
| `dtstart`       | `String`         | Start datetime (ISO 8601 with timezone) |
| `dtend`         | `String`         | End datetime (ISO 8601 with timezone)   |
| `rrule`         | `Option<String>` | Recurrence rule (RFC 5545 format)       |
| `reminders`     | `Vec<Reminder>`  | List of `VALARM` reminders              |
| `created`       | `String`         | Creation timestamp                      |
| `last_modified` | `String`         | Last-modified timestamp                 |

#### `Reminder` (`VALARM`)

| Field    | Type     | Notes                                         |
| -------- | -------- | --------------------------------------------- |
| `action` | `ReminderAction` | Validated newtype: non-empty, max 64 chars. `DISPLAY` or `EMAIL` |
| `trigger`| `ReminderTrigger`| Validated non-empty newtype. Duration before event (`-PT15M`) or absolute |

#### Room Calendar Mapping

| Field           | Type                                | Notes                                                 |
| --------------- | ----------------------------------- | ----------------------------------------------------- |
| room_calendars  | `HashMap<String, String>`           | `room_id → webdav_dir` mapping (in-memory, `Mutex`). `room_id` is the raw RocketChat room ID (cache key); `webdav_dir` is the human-readable directory/calendar name (e.g. `r-General`, `d-bob`) |

#### `WebDavPath` (calendar methods)

Calendar paths are built via `CalendarTool::build_caldav_url(webdav_dir)` — the
CalDAV endpoint is a separate URL (`/remote.php/dav/calendars/{user}/{webdav_dir}/`)
independent of the WebDAV file storage root. `WebDavPath` does **not** provide
calendar-specific methods. The URL is constructed directly in `CalendarTool`.

| Method                              | Returns  | Notes                             |
| ----------------------------------- | -------- | --------------------------------- |
| `build_caldav_url(calendar_name)`   | `String` | Constructs the CalDAV URL for a given calendar name — implemented in `CalendarTool`, not `WebDavPath` |

## 4. NextCloud API Reference

Per [NextCloud Calendar user guide](https://docs.nextcloud.com/server/latest/user_manual/en/groupware/calendar.html), [RFC 4791](https://datatracker.ietf.org/doc/html/rfc4791) (CalDAV), and [RFC 5545](https://datatracker.ietf.org/doc/html/rfc5545) (iCalendar). NextCloud serves CalDAV at `/remote.php/dav/calendars/{user}/{calendar-name}/`.

### New: Create Calendar

| DFD Operation   | HTTP Method  | Endpoint / Headers                                     | Notes                                                    |
| --------------- | ------------ | ------------------------------------------------------ | -------------------------------------------------------- |
| EnsureCalendar  | `MKCALENDAR` | `{origin}/remote.php/dav/calendars/{user}/{cal-name}/` | Creates a new calendar collection if it doesn't exist    |
| CalendarExists  | `PROPFIND`   | `{origin}/remote.php/dav/calendars/{user}/{cal-name}/` | Depth: 0, check for 207 response                         |

### Event Operations

| DFD Operation       | HTTP Method | Endpoint / Headers                        | Notes                                           |
| ------------------- | ----------- | ----------------------------------------- | ----------------------------------------------- |
| ListEventsByDate    | `REPORT`    | `{base}/calendars/{user}/{cal}/`          | XML body with `calendar-query`, time-range filter |
| GetEvent            | `GET`       | `{base}/calendars/{user}/{cal}/{uid}.ics` | Returns full `VEVENT` iCalendar data            |
| AddEvent            | `PUT`       | `{base}/calendars/{user}/{cal}/{uid}.ics` | Body = `VEVENT` iCalendar (RFC 5545)            |
| UpdateEvent         | `PUT`       | `{base}/calendars/{user}/{cal}/{uid}.ics` | `If-Match: {etag}` header; 409 on conflict      |
| DeleteEvent         | `DELETE`    | `{base}/calendars/{user}/{cal}/{uid}.ics` | 204 on success, 404 if not found                |
#### `MKCALENDAR` request body

```xml
<?xml version="1.0" encoding="UTF-8"?>
<C:mkcalendar xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:set>
    <D:prop>
      <D:displayname>{Display Name}</D:displayname>
      <C:supported-calendar-component-set>
        <C:comp name="VEVENT"/>
      </C:supported-calendar-component-set>
    </D:prop>
  </D:set>
</C:mkcalendar>
```

#### `calendar-query` REPORT body (listing events for a date)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:prop>
    <D:getetag/>
    <C:calendar-data/>
  </D:prop>
  <C:filter>
    <C:comp-filter name="VCALENDAR">
      <C:comp-filter name="VEVENT">
        <C:time-range start="20260601T000000Z" end="20260602T000000Z"/>
      </C:comp-filter>
    </C:comp-filter>
  </C:filter>
</C:calendar-query>
```

#### `VEVENT` iCalendar payload (create/update event with reminder)

```
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//RockBot//NextCloud Calendar//EN
BEGIN:VEVENT
UID:abc123-uuid@rockbot
DTSTART:20260615T140000Z
DTEND:20260615T150000Z
SUMMARY:Team standup
DESCRIPTION:Daily sync meeting
LOCATION:Room 4B
BEGIN:VALARM
ACTION:DISPLAY
TRIGGER:-PT15M
DESCRIPTION:Meeting in 15 minutes
END:VALARM
END:VEVENT
END:VCALENDAR
```
