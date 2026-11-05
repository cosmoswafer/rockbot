# Deep Argument Traversal — All Injection Points

## 1. Purpose

Shows how the `web_fetch` argument JSON is walked field-by-field so every
string value — `url`, `headers`, `body`, and nested `body_json` leaves — goes
through secret-reference replacement. Referenced from
[Happy Flow — UUIDv5 Generation + Host-Scoped Injection](uuidv5-scoped-injection.md)
and detailed in [Secret Reference Replacement (Per-String, UUID-Based)](level-2/reference-replacement.md).

## 2. Diagram

```mermaid
flowchart TD
    ARGS[Parse web_fetch arguments as JSON]
    URL["Walk url (string) → replace secret:uuid"]
    HEADERS["Walk headers (object keys → string values) → replace secret:uuid"]
    BODY["Walk body (string) → replace secret:uuid"]
    BODY_JSON["Walk body_json (object, recursive) → replace secret:uuid in all leaf strings"]
    RESOLVE["Resolved arguments JSON<br/>all secret:uuid references<br/>replaced in every field"]
    ARGS --> URL
    ARGS --> HEADERS
    ARGS --> BODY
    ARGS --> BODY_JSON
    URL --> RESOLVE
    HEADERS --> RESOLVE
    BODY --> RESOLVE
    BODY_JSON --> RESOLVE
```
