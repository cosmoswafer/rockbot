# WebDAV Calendar

## 1. Purpose

CalDAV event access wrapping NextCloud's calendar service. Supports listing
events by date range, create/read/update/delete individual events with
iCalendar (RFC 5545) `VEVENT` payloads, and `VALARM` reminders.

**Scope**: Calendar events are **per-room** — each RocketChat room gets its own
NextCloud calendar, auto-created on first use via CalDAV `MKCALENDAR`. The
calendar name is `{webdav_dir}` (matching the WebDAV directory name,
e.g. `r-General`, `d-bob`), stored under the configured user's CalDAV
calendar home (`/remote.php/dav/calendars/{username}/`). Events from
different rooms are fully isolated.

> **Note:** `list_todos` currently does **not** accept a date range — it
> fetches todos without time-range filtering. The DFD diagrams show date
> range for todos as aspirational/planned behavior.

- Upstream: [Configuration Management](../base/config.md) provides `WebDavConfig`
  (server URL, credentials)
- Downstream: [Agent Harness](../agent-harness.md) injects `room_id` + `webdav_dir`
  into calendar tool arguments. `room_id` is used as the cache key in
  `room_calendars`, while `webdav_dir` names the per-room calendar
  (auto-created on first use)

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CALLER[Calling Subsystem]
    CAL_CFG[(WebDavConfig)]
    HTTP(HttpClient)
    NC[(NextCloud CalDAV)]
    AUTO(EnsureCalendar)
    LIST(ListEventsByDate)
    GET(GetEvent)
    ADD(AddEvent)
    UPD(UpdateEvent)
    DEL(DeleteEvent)
    LIST_TODOS(ListTodos)

    CALLER -->|"date range + room_id"| LIST
    CALLER -->|"event uid + room_id"| GET
    CALLER -->|"event details + room_id"| ADD
    CALLER -->|"event uid + updates + room_id"| UPD
    CALLER -->|"event uid + room_id"| DEL
    CALLER -->|"date range + room_id"| LIST_TODOS

    CAL_CFG -->|"server url + credentials"| AUTO
    AUTO -->|"checks room calendar mapping"| LIST
    AUTO -->|"checks room calendar mapping"| GET
    AUTO -->|"checks room calendar mapping"| ADD
    AUTO -->|"checks room calendar mapping"| UPD
    AUTO -->|"checks room calendar mapping"| DEL
    AUTO -->|"checks room calendar mapping"| LIST_TODOS

    LIST -->|"REPORT calendar-query xml"| HTTP
    GET -->|"GET .ics"| HTTP
    ADD -->|"PUT vevent ics body"| HTTP
    UPD -->|"PUT vevent ics + If-Match etag"| HTTP
    DEL -->|"DELETE .ics"| HTTP
    LIST_TODOS -->|"REPORT calendar-query with VTODO filter"| HTTP
    HTTP -->|"dav request"| NC
    NC -->|"207 multi-status"| LIST
    NC -->|"200 .ics body"| GET
    NC -->|"201 created"| ADD
    NC -->|"204 no content"| UPD
    NC -->|"204 no content"| DEL
    NC -->|"207 multi-status"| LIST_TODOS
    LIST -->|"event list"| CALLER
    GET -->|"event .ics"| CALLER
    ADD -->|"event uid"| CALLER
    UPD -->|"updated"| CALLER
    DEL -->|"deleted"| CALLER
    LIST_TODOS -->|"todo list"| CALLER
```

### 2b. Calendar Auto-Creation Flow

```mermaid
flowchart TD
    CALLER[Caller provides room_id + webdav_dir]
    MAP[(room_calendars HashMap)]
    HTTP(HttpClient)
    NC[(NextCloud CalDAV)]
    CHECK{Calendar in map?}
    EXISTS{Calendar exists on NC?}
    CREATE(MKCALENDAR)
    CAL_READY[Use per-room calendar URL]
    CAL_ERR[Warn — proceed with operation]

    CALLER --> CHECK
    CHECK -->|yes, cached| CAL_READY
    CHECK -->|no| EXISTS
    EXISTS -->|yes via PROPFIND| MAP
    EXISTS -->|no| CREATE
    EXISTS -->|error| CAL_ERR
    CREATE -->|201 created| MAP
    CREATE -->|error| CAL_ERR
    MAP --> CAL_READY
```

### 2c. Calendar Operations Deep Dive

Per [NextCloud Calendar user guide](https://docs.nextcloud.com/server/latest/user_manual/en/groupware/calendar.html) and [RFC 4791](https://datatracker.ietf.org/doc/html/rfc4791). Events are iCalendar (RFC 5545) `VEVENT` objects. The CalDAV base URL is `/remote.php/dav/calendars/{username}/{webdav_dir}/` (e.g. `/remote.php/dav/calendars/bot/r-General/`). Each event is a resource named `{uid}.ics` within that collection.

```mermaid
flowchart TD
    HTTP(HttpClient)
    NC[(NextCloud CalDAV)]

    subgraph CalendarCRUD[Calendar CRUD Operations]
        direction LR
        EVT_LIST(REPORT calendar-query)
        EVT_GET(GET .ics resource)
        EVT_ADD(PUT new .ics)
        EVT_UPD(PUT existing .ics + If-Match)
        EVT_DEL(DELETE .ics resource)
        EVT_LIST_TODOS(REPORT calendar-query<br/>with VTODO filter)
    end

    EVT_LIST -->|"REPORT + calendar-query xml"| HTTP
    EVT_GET -->|"GET .ics"| HTTP
    EVT_ADD -->|"PUT vevent ics body"| HTTP
    EVT_UPD -->|"PUT vevent ics + If-Match: etag"| HTTP
    EVT_DEL -->|"DELETE .ics"| HTTP
    EVT_LIST_TODOS -->|"REPORT + calendar-query<br/>with comp-filter VTODO"| HTTP

    HTTP -->|"dav request"| NC
    NC -->|"207 multi-status"| EVT_LIST
    NC -->|"200 .ics body"| EVT_GET
    NC -->|"201 created"| EVT_ADD
    NC -->|"204 no content"| EVT_UPD
    NC -->|"204 no content"| EVT_DEL
    NC -->|"207 multi-status"| EVT_LIST_TODOS

    subgraph VTODOStructure[VTODO Content]
        direction LR
        TODOSUMMARY[summary: title]
        TODODESCRIPTION[description: details]
        TODODUE[due: datetime]
        TODOSTATUS[status: COMPLETED/NEEDS-ACTION]
        TODOPRIORITY[priority: 1-9]
    end

    EVT_LIST_TODOS -->|"parses time-range filtered vtodos"| VTODOStructure

    subgraph VEVENTStructure[VEVENT Content]
        direction LR
        DTSTART[dtstart: datetime]
        DTEND[dtend: datetime]
        SUMMARY[summary: title]
        DESCRIPTION[description: details]
        LOCATION[location: string]
        RRULE[rrule: recurrence]
        VALARM[valarm: reminder trigger]
    end

    EVT_ADD -->|"builds vevent"| VEVENTStructure
    EVT_UPD -->|"merges updates into vevent"| VEVENTStructure
    EVT_LIST -->|"parses time-range filtered vevents"| VEVENTStructure
```

### 2d. Error Handling & Fallbacks

```mermaid
flowchart TD
    HTTP(HttpClient)
    NC[(NextCloud CalDAV)]
    ERR_CONFLICT[CalDAV 409 Conflict]
    ERR_BAD_ICS[Invalid iCalendar]
    ERR_404[Event Not Found]
    CAL_UPD(UpdateEvent)
    CAL_REFETCH(RefetchEvent)
    CAL_RETRY(RetryUpdate)
    CAL_AUTO_ERR[MKCALENDAR failed]

    CAL_AUTO_ERR -.->|"log warn, still attempt operation"| HTTP
    CAL_UPD -.->|"409 conflict: etag mismatch"| ERR_CONFLICT
    HTTP -.->|"400 bad request"| ERR_BAD_ICS
    HTTP -.->|"404 not found"| ERR_404
```

Note: The 409 Conflict retry loop (refetch → merge → retry with new etag) is not yet implemented. Calendar update returns an error on etag mismatch. MKCALENDAR failure (permissions, unsupported) is non-fatal — the operation still proceeds against the target URL.

## 3. Data Structures

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
| `action` | `String` | `DISPLAY` or `EMAIL`                          |
| `trigger`| `String` | Duration before event (`-PT15M`) or absolute   |

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
| ListTodos           | `REPORT`    | `{base}/calendars/{user}/{cal}/`          | XML body with `calendar-query`, comp-filter `VTODO`, time-range filter |

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
