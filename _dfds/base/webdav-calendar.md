# WebDAV Calendar

## 1. Purpose

CalDAV event access wrapping NextCloud's calendar service. Supports listing
events by date range, create/read/update/delete individual events with
iCalendar (RFC 5545) `VEVENT` payloads, and `VALARM` reminders.

- Upstream: [Configuration Management](config.md) provides `WebDavConfig`
  plus calendar name
- Downstream: [Agent Harness](../agent-harness.md) exposes calendar event
  access to the AI agent via the calendar tool

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CALLER[Calling Subsystem]
    CAL_CFG[(WebDavConfig + calendar-name)]
    HTTP(HttpClient)
    NC[(NextCloud CalDAV)]
    LIST(ListEventsByDate)
    GET(GetEvent)
    ADD(AddEvent)
    UPD(UpdateEvent)
    DEL(DeleteEvent)

    CALLER -->|"date range"| LIST
    CALLER -->|"event uid"| GET
    CALLER -->|"event details"| ADD
    CALLER -->|"event uid + updates"| UPD
    CALLER -->|"event uid"| DEL
    CAL_CFG -->|"caldav url + credentials"| LIST
    CAL_CFG -->|"caldav url + credentials"| GET
    CAL_CFG -->|"caldav url + credentials"| ADD
    CAL_CFG -->|"caldav url + credentials"| UPD
    CAL_CFG -->|"caldav url + credentials"| DEL
    LIST -->|"REPORT calendar-query xml"| HTTP
    GET -->|"GET .ics"| HTTP
    ADD -->|"PUT vevent ics body"| HTTP
    UPD -->|"PUT vevent ics + If-Match etag"| HTTP
    DEL -->|"DELETE .ics"| HTTP
    HTTP -->|"dav request"| NC
    NC -->|"207 multi-status"| LIST
    NC -->|"200 .ics body"| GET
    NC -->|"201 created"| ADD
    NC -->|"204 no content"| UPD
    NC -->|"204 no content"| DEL
    LIST -->|"event list"| CALLER
    GET -->|"event .ics"| CALLER
    ADD -->|"event uid"| CALLER
    UPD -->|"updated"| CALLER
    DEL -->|"deleted"| CALLER
```

### 2b. Calendar Operations Deep Dive

Per [NextCloud Calendar user guide](https://docs.nextcloud.com/server/latest/user_manual/en/groupware/calendar.html) and [RFC 4791](https://datatracker.ietf.org/doc/html/rfc4791). Events are iCalendar (RFC 5545) `VEVENT` objects. The CalDAV base URL is `/remote.php/dav/calendars/{username}/{calendar-name}/`. Each event is a resource named `{uid}.ics` within that collection.

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
    end

    EVT_LIST -->|"REPORT + calendar-query xml"| HTTP
    EVT_GET -->|"GET .ics"| HTTP
    EVT_ADD -->|"PUT vevent ics body"| HTTP
    EVT_UPD -->|"PUT vevent ics + If-Match: etag"| HTTP
    EVT_DEL -->|"DELETE .ics"| HTTP

    HTTP -->|"dav request"| NC
    NC -->|"207 multi-status"| EVT_LIST
    NC -->|"200 .ics body"| EVT_GET
    NC -->|"201 created"| EVT_ADD
    NC -->|"204 no content"| EVT_UPD
    NC -->|"204 no content"| EVT_DEL

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

### 2c. Error Handling & Fallbacks

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

    CAL_UPD -.->|"409 conflict: etag mismatch"| ERR_CONFLICT
    ERR_CONFLICT -.->|"GET current event"| CAL_REFETCH
    CAL_REFETCH -.->|"merge + PUT with new etag"| CAL_RETRY
    CAL_RETRY -.->|"retry update"| HTTP
    HTTP -.->|"400 bad request"| ERR_BAD_ICS
    HTTP -.->|"404 not found"| ERR_404
```

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

#### `WebDavPath` (calendar methods)

| Method                      | Returns  | Notes                             |
| --------------------------- | -------- | --------------------------------- |
| `calendar_path(calendar)`   | `String` | `/calendars/{calendar}/`          |
| `event_path(calendar, uid)` | `String` | `/calendars/{calendar}/{uid}.ics` |

## 4. NextCloud API Reference

Per [NextCloud Calendar user guide](https://docs.nextcloud.com/server/latest/user_manual/en/groupware/calendar.html), [RFC 4791](https://datatracker.ietf.org/doc/html/rfc4791) (CalDAV), and [RFC 5545](https://datatracker.ietf.org/doc/html/rfc5545) (iCalendar). NextCloud serves CalDAV at `/remote.php/dav/calendars/{user}/{calendar-name}/`.

| DFD Operation       | HTTP Method | Endpoint / Headers                        | Notes                                           |
| ------------------- | ----------- | ----------------------------------------- | ----------------------------------------------- |
| ListEventsByDate    | `REPORT`    | `{base}/calendars/{user}/{cal}/`          | XML body with `calendar-query`, time-range filter |
| GetEvent            | `GET`       | `{base}/calendars/{user}/{cal}/{uid}.ics` | Returns full `VEVENT` iCalendar data            |
| AddEvent            | `PUT`       | `{base}/calendars/{user}/{cal}/{uid}.ics` | Body = `VEVENT` iCalendar (RFC 5545)            |
| UpdateEvent         | `PUT`       | `{base}/calendars/{user}/{cal}/{uid}.ics` | `If-Match: {etag}` header; 409 on conflict      |
| DeleteEvent         | `DELETE`    | `{base}/calendars/{user}/{cal}/{uid}.ics` | 204 on success, 404 if not found                |

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
