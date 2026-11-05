# WebDAV Calendar — Operations Deep Dive

## 1. Purpose

Details of the per-operation request/response shapes and VEVENT content used
in [Main Path](../main-path.md).

Per [NextCloud Calendar user guide](https://docs.nextcloud.com/server/latest/user_manual/en/groupware/calendar.html) and [RFC 4791](https://datatracker.ietf.org/doc/html/rfc4791). Events are iCalendar (RFC 5545) `VEVENT` objects. The CalDAV base URL is `/remote.php/dav/calendars/{username}/{webdav_dir}/` (e.g. `/remote.php/dav/calendars/bot/r-General/`). Each event is a resource named `{uid}.ics` within that collection.

## 2. Diagram

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
