# WebDAV Directory

## 1. Purpose

Thin abstraction over HTTP-based WebDAV (NextCloud) providing typed file
read/write/list/mkdir/delete with per-room directory isolation. Each room gets
its own subtree created proactively on first use. Room names use type prefixes
(`r-` for channels, `d-` for DMs) to prevent collisions.

- Upstream: [Configuration Management](../base/config.md) provides `WebDavConfig`
- Downstream: [Agent Harness](../agent-harness.md) exposes `WebDavTool` to
  the AI agent
- Downstream: [Knowledge Management](../base/knowledge.md) persists `.md` files
- Downstream: [Memory Management](../base/memory.md) uses PUT/GET/PROPFIND
  operations for JSON archive persistence

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CALLER[Calling Subsystem]
    CFG[(WebDavConfig)]
    RESOLVE(ResolvePath)
    READ(ReadFile)
    WRITE(WriteFile)
    LIST(ListDirectory)
    MKDIR(EnsureDirectory)
    DELETE(DeleteFile)
    EDIT(EditFile)
    EXISTS(CheckExists)
    ENSURE(EnsureRoomDir)
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]

    CALLER -->|"path + operation"| RESOLVE
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
    EDIT -->|"GET + content update + PUT"| WRITE
    EXISTS -->|"GET request"| READ
    ENSURE -.->|"mkcol request"| MKDIR
    READ -->|"GET"| HTTP
    WRITE -->|"PUT with body + AutoMkcol header"| HTTP
    LIST -->|"PROPFIND depth=1"| HTTP
    MKDIR -->|"MKCOL"| HTTP
    DELETE -->|"DELETE"| HTTP
    HTTP -->|"http request"| NC
    NC -->|"response"| HTTP
    HTTP -->|"response body / status"| RESOLVE
```

Note: `ensure_room_directory()` (client.rs:264) exists but is not currently called — directories are created implicitly by `write_file_with_fallback()` via AutoMkcol.

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]
    ERR_404[Path Not Found]
    ERR_NET[Network Unreachable]
    MKDIR_ALL(EnsureDirectoryAll)
    WRITE(WriteFile)
    WRITE_SIMPLE[WriteFile plain PUT]
    AUTO_MKCOL[WriteFileAutoMkcol]

    WRITE --> AUTO_MKCOL
    AUTO_MKCOL -->|"PUT + X-NC-WebDAV-AutoMkcol: 1"| HTTP
    HTTP -->|"200/201/204"| WRITE
    HTTP -.->|"404 not found"| ERR_404
    ERR_404 -.->|"extract parent path"| MKDIR_ALL
    MKDIR_ALL -.->|"mkcol success"| WRITE_SIMPLE
    WRITE_SIMPLE -.->|"PUT without mkcol header"| HTTP
    WRITE_SIMPLE -.->|"404 not found (retry exhausted)"| ERR_NET
    HTTP -.->|"connection refused / timeout"| ERR_NET
```

### 2c. Write-With-Fallback Deep Dive

```mermaid
flowchart TD
    W(WriteInitiated)
    AMC[Try AutoMkcol PUT]
    HTTP[(HTTP Client)]
    NC[(NextCloud)]
    CHECK{Status?}
    OK(Success)
    IS_404{Is 404?}
    PARENT(ExtractParentPath)
    MKCOL_ALL(MkcolAll parent dirs)
    PUT_RETRY(PUT without mkcol header)
    FAIL(Fail)

    W --> AMC
    AMC --> HTTP
    HTTP --> NC
    NC --> CHECK
    CHECK -->|"200/201/204"| OK
    CHECK -.->|"other status"| IS_404
    IS_404 -.->|"yes"| PARENT
    IS_404 -.->|"no"| FAIL
    PARENT -.-> MKCOL_ALL
    MKCOL_ALL -.-> PUT_RETRY
    PUT_RETRY -.-> HTTP
```

### 2d. Room Directory Structure

Each room (channel or DM) has three subdirectories: `memory/`, `images/`, and
`workspace/`. A shared `config/` directory holds backups. The `calendars/`
directory stores CalDAV events (see [Calendar](calendar.md)).

```mermaid
flowchart TD
    ROOT[(WebDAV Root)]
    CH_ATOM[(r-atomkb)]
    CH_PROJ[(r-project-x)]
    DM_SARU[(d-saru)]
    MEM_ATOM[(r-atomkb/memory)]
    IMG_ATOM[(r-atomkb/images)]
    WSP_ATOM[(r-atomkb/workspace)]
    MEM_PROJ[(r-project-x/memory)]
    IMG_PROJ[(r-project-x/images)]
    WSP_PROJ[(r-project-x/workspace)]
    MEM_SARU[(d-saru/memory)]
    IMG_SARU[(d-saru/images)]
    WSP_SARU[(d-saru/workspace)]
    CFG_DIR[(config/)]
    CAL_DIR[(calendars/)]

    ROOT --> CH_ATOM
    ROOT --> CH_PROJ
    ROOT --> DM_SARU
    ROOT --> CFG_DIR
    ROOT --> CAL_DIR
    CH_ATOM --> MEM_ATOM
    CH_ATOM --> IMG_ATOM
    CH_ATOM --> WSP_ATOM
    CH_PROJ --> MEM_PROJ
    CH_PROJ --> IMG_PROJ
    CH_PROJ --> WSP_PROJ
    DM_SARU --> MEM_SARU
    DM_SARU --> IMG_SARU
    DM_SARU --> WSP_SARU
```

## 3. Data Structures

#### `WebDavClient`

| Field        | Type              | Notes                                  |
| ------------ | ----------------- | -------------------------------------- |
| `base_url`   | `String`          | WebDAV endpoint including root         |
| `client`     | `reqwest::Client` | Shared HTTP client with connection pool|
| `auth_header`| `String`          | `Basic` base64-encoded credentials     |

#### `WebDavEntry`

| Field      | Type     | Notes                              |
| ---------- | -------- | ---------------------------------- |
| `name`     | `String` | File or directory name             |
| `href`     | `String` | Full WebDAV href                   |
| `is_dir`   | `bool`   | True if collection (directory)     |
| `size`     | `u64`    | File size in bytes (0 for dirs)    |
| `modified` | `String` | Last-modified timestamp            |

#### `WebDavPath`

All methods accept a `dir_key` — a flat type-prefixed directory name such
as `r-森林生態` or `d-saru`. The harness computes `webdav_dir` preferring
`room_fname` (the friendly display name) over `room_name` (the ASCII slug);
the raw RocketChat room UUID is never used as a path segment.

| Method                   | Returns  | Notes                                       |
| ------------------------ | -------- | ------------------------------------------- |
| `room_dir(key)`          | `String` | `/{root}/{key}/`                            |
| `room_path(key, file)`   | `String` | `/{root}/{key}/{file_path}`                 |
| `image_dir(key)`         | `String` | `/{root}/{key}/images/`                     |
| `workspace_dir(key)`     | `String` | `/{root}/{key}/workspace/`                  |
| `image_path(key, name)`  | `String` | `/{root}/{key}/images/{name}`               |
| `parent_path(path)`      | `String` | Strips last path segment                    |

## 4. NextCloud API Reference

Per [NextCloud WebDAV basic operations](https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/basic.html).

| DFD Operation           | HTTP Method | NextCloud Endpoint                        | Notes                                |
| ----------------------- | ----------- | ----------------------------------------- | ------------------------------------ |
| ReadFile                | `GET`       | `{base}/files/{user}/{path}`              | Returns raw file bytes               |
| WriteFile               | `PUT`       | `{base}/files/{user}/{path}`              | Overwrites existing files            |
| WriteFileAutoMkcol      | `PUT`       | `{base}/files/{user}/{path}`              | Set `X-NC-WebDAV-AutoMkcol: 1` header |
| WriteFileWithFallback   | `PUT`       | `{base}/files/{user}/{path}`              | Tries AutoMkcol; 404 → mkcol parents → retry PUT |
| ListDirectory           | `PROPFIND`  | `{base}/files/{user}/{path}`              | `Depth: 1` for children              |
| EnsureDirectory         | `MKCOL`     | `{base}/files/{user}/{path}`              | Returns 405 if exists                |
| EnsureDirectoryAll      | `MKCOL`     | `{base}/files/{user}/{path}`              | Iterative MKCOL per segment          |
| EnsureRoomDirectory     | `MKCOL`     | `{base}/files/{user}/{root}/{room}/`      | Creates room dir on first use        |
| Delete                  | `DELETE`    | `{base}/files/{user}/{path}`              | Recursive for folders                |
| Exists                  | `PROPFIND`  | `{base}/files/{user}/{path}`              | `Depth: 0` — 207 = exists, 404 = no  |
