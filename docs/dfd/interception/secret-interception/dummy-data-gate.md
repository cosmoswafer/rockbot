# Dummy Data Gate — WebDAV Tool LLM Read Interception

## 1. Purpose

If the LLM reads `secrets.toml` through the WebDAV read tool (e.g. via path
traversal), only sanitized values (`abcd`) are returned — host and key
metadata are preserved, real credentials never reach the LLM. Referenced from
[Happy Flow — UUIDv5 Generation + Host-Scoped Injection](uuidv5-scoped-injection.md).

## 2. Diagram

```mermaid
flowchart TD
    LLM[LLM issues webdav read<br/>path: secrets.toml]
    TOOL[WebDavTool::do_read]
    CHECK{Filename is<br/>secrets.toml?}
    READ["Read real secrets.toml<br/>from room WebDAV path<br/>(client call via room_path)"]
    PARSE[Parse TOML → replace<br/>all value fields with abcd]
    DAV[(NextCloud WebDAV)]
    REAL[Return real content<br/>to harness loader only]
    DUMMY["Return sanitized TOML<br/>host + key preserved<br/>all values = 'abcd'<br/>to LLM"]
    LLM_OUT["LLM receives<br/>consistent key/host metadata<br/>but no real credentials"]

    LLM --> TOOL
    TOOL --> CHECK
    CHECK -->|"yes (LLM read attempt)"| READ
    CHECK -->|"no (normal file)"| DAV
    READ --> PARSE
    PARSE --> DUMMY
    DUMMY --> LLM_OUT
```
