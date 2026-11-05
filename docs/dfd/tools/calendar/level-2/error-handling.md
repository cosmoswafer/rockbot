# WebDAV Calendar — Error Handling

## 1. Purpose

Error paths and fallbacks for calendar operations; see [Main Path](../main-path.md) for the happy flow.

## 2. Diagram

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
