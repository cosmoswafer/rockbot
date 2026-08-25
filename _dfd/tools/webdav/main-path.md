# WebDAV — Main Path

## 1. Purpose

Happy flow of the WebDAV tool layer: a caller supplies a raw path and an
operation; the path is sanitized, resolved against the room-scoped prefix,
and dispatched as the corresponding HTTP request to NextCloud. The whole
flow is a thin abstraction — data structures are shared, see
[structures.md](structures.md).

## 2. Diagram

```mermaid
flowchart TD
    CALLER[Calling Subsystem]
    CFG[(WebDavConfig)]
    SANITIZE(SanitizePath)
    RESOLVE(ResolvePath)
    READ(ReadFile)
    WRITE(WriteFile)
    LIST(ListDirectory)
    MKDIR(EnsureDirectory)
    DELETE(DeleteFile)
    EDIT(EditFile)
    EXISTS(CheckExists)
    RENAME(RenameFile)
    ENSURE(EnsureRoomDir)
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]

    CALLER -->|"raw path + operation"| SANITIZE
    SANITIZE -->|"sanitized path (see 2e)"| RESOLVE
    CALLER -.->|"room on first use"| ENSURE
    CFG -->|"root + credentials"| RESOLVE
    CFG -.->|"root + credentials"| ENSURE
    RESOLVE -->|"get request"| READ
    RESOLVE -->|"put request"| WRITE
    RESOLVE -->|"propfind request"| LIST
    RESOLVE -->|"mkcol request"| MKDIR
    RESOLVE -->|"delete request"| DELETE
    RESOLVE -->|"edit request"| EDIT
    RESOLVE -->|"exists request"| EXISTS
    RESOLVE -->|"move request"| RENAME
    EDIT -->|"GET + content update + PUT"| WRITE
    EXISTS -->|"GET request"| READ
    ENSURE -.->|"mkcol request"| MKDIR
    READ -->|"GET"| HTTP
    WRITE -->|"PUT with body + AutoMkcol header"| HTTP
    LIST -->|"PROPFIND depth=1"| HTTP
    MKDIR -->|"MKCOL"| HTTP
    DELETE -->|"DELETE"| HTTP
    RENAME -->|"MOVE + Destination header"| HTTP
    HTTP -->|"http request"| NC
    NC -->|"response"| HTTP
    HTTP -->|"response body / status"| RESOLVE
```

Note: `ensure_room_directory()` (client.rs:264) exists but is not currently called — directories are created implicitly by `write_file_with_fallback()` via AutoMkcol.
