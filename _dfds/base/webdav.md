# WebDAV Storage

## 1. Purpose

Thin abstraction over HTTP-based WebDAV (NextCloud) providing typed
read/write/list/mkdir/delete operations. All bot state — configuration backups, memory
archives, and image assets — is stored remotely; the bot never writes to local
disk. Each room gets its own directory subtree, created proactively on first
use.

**Room name isolation:** Directories use human-readable room names with
namespace prefixes — `rooms/{name}/` for channels (e.g. `rooms/atomkb/`) and
`dms/{name}/` for direct messages (e.g. `dms/saru/`). The prefixes prevent
collisions between a channel and a DM user with the same slug. The harness
computes the `webdav_dir` from `room_name` + `is_dm` and injects it into tool
arguments; the raw `room_id` UUID is never used for WebDAV path construction.

The WebDAV client is used both internally (by `harness.rs` for room message
archiving) and as an AI-callable tool (`WebDavTool` in `tools/webdav.rs`) that
exposes read, write, list, mkdir, delete, and exists operations scoped to room
directories.

The client targets NextCloud's WebDAV API at the path:
`{base_url}/remote.php/dav/files/{username}`. Authentication uses HTTP Basic Auth
with an app password (generated via NextCloud's personal security settings).

- Upstream: [Configuration Management](config.md) provides `WebDavConfig`
- Upstream: [Memory Management](memory.md) stores and retrieves `.md` archives
- Upstream: [Agent Harness](../agent-harness.md) (vision tool) reads images from WebDAV
- Upstream: [Agent Harness](../agent-harness.md) (webdav tool) exposes storage to the AI agent

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
    ENSURE_ROOM(EnsureRoomDir)
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]

    CALLER -->|"path + operation"| RESOLVE
    CALLER -->|"room_id on first use"| ENSURE_ROOM
    CFG -->|"root + credentials"| RESOLVE
    CFG -->|"root + credentials"| ENSURE_ROOM
    RESOLVE -->|"get request"| READ
    RESOLVE -->|"put request"| WRITE
    RESOLVE -->|"propfind request"| LIST
    RESOLVE -->|"mkcol request"| MKDIR
    RESOLVE -->|"delete request"| DELETE
    ENSURE_ROOM -->|"mkcol request"| MKDIR
    READ -->|"GET"| HTTP
    WRITE -->|"PUT with body + AutoMkcol header"| HTTP
    LIST -->|"PROPFIND depth=1"| HTTP
    MKDIR -->|"MKCOL"| HTTP
    DELETE -->|"DELETE"| HTTP
    HTTP -->|"http request"| NC
    NC -->|"response"| HTTP
    HTTP -->|"response body / status"| RESOLVE
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]
    ERR_404[Error: Path Not Found]
    ERR_NET[Error: Network Unreachable]
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
    W(WritesInitiated)
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

### 2d. Directory Structure Deep Dive

All room data is namespaced under `rooms/` (channels) or `dms/` (direct messages).
The harness injects `webdav_dir` (e.g. `"rooms/atomkb"`) into tool arguments so
every read, write, and archive call targets the correct room subtree.

```mermaid
flowchart TD
    ROOT[(WebDAV Root)]
    ROOMS[(rooms/)]
    DMS[(dms/)]
    CH_ATOM[(atomkb)]
    CH_PROJ[(project-x)]
    DM_SARU[(saru)]
    MEM_CH_ATOM[(atomkb/memory)]
    MEM_CH_PROJ[(project-x/memory)]
    MEM_DM_SARU[(saru/memory)]
    IMG_CH_ATOM[(atomkb/images)]
    IMG_CH_PROJ[(project-x/images)]
    IMG_DM_SARU[(saru/images)]
    WSP_CH_ATOM[(atomkb/workspace)]
    WSP_CH_PROJ[(project-x/workspace)]
    WSP_DM_SARU[(saru/workspace)]
    CFG_DIR[(config/)]

    ROOT --> ROOMS
    ROOT --> DMS
    ROOT --> CFG_DIR
    ROOMS --> CH_ATOM
    ROOMS --> CH_PROJ
    DMS --> DM_SARU
    CH_ATOM --> MEM_CH_ATOM
    CH_ATOM --> IMG_CH_ATOM
    CH_ATOM --> WSP_CH_ATOM
    CH_PROJ --> MEM_CH_PROJ
    CH_PROJ --> IMG_CH_PROJ
    CH_PROJ --> WSP_CH_PROJ
    DM_SARU --> MEM_DM_SARU
    DM_SARU --> IMG_DM_SARU
    DM_SARU --> WSP_DM_SARU
```

## 3. Data Structures

#### `WebDavClient`

| Field       | Type              | Notes                                  |
| ----------- | ----------------- | -------------------------------------- |
| `base_url`  | `String`          | WebDAV endpoint including root          |
| `client`    | `reqwest::Client` | Shared HTTP client with connection pool|
| `auth_header`| `String`          | `Basic` base64-encoded credentials     |

#### `WebDavEntry`

| Field       | Type     | Notes                                      |
| ----------- | -------- | ------------------------------------------ |
| `name`      | `String` | File or directory name                     |
| `href`      | `String` | Full WebDAV href                           |
| `is_dir`    | `bool`   | True if collection (directory)             |
| `size`      | `u64`    | File size in bytes (0 for dirs)            |
| `modified`  | `String` | Last-modified timestamp                    |

#### `WebDavPath`

All methods accept a `dir_key` — either a plain room name (for backwards
compatibility) or a namespaced `webdav_dir` such as `rooms/atomkb` or
`dms/saru`. The harness computes and injects `webdav_dir`; the raw
RocketChat room UUID is never used as a path segment.

| Method                  | Returns    | Notes                                    |
| ----------------------- | ---------- | ---------------------------------------- |
| `room_dir(key)`         | `String`   | `/{root}/{key}/`                         |
| `memory_dir(key)`       | `String`   | `/{root}/{key}/memory/`                  |
| `image_dir(key)`        | `String`   | `/{root}/{key}/images/`                  |
| `workspace_dir(key)`    | `String`   | `/{root}/{key}/workspace/`               |
| `image_path(key, name)` | `String`   | `/{root}/{key}/images/{name}`            |
| `archive_path(key, seq)`| `String`   | `/{root}/{key}/memory/{seq:06}_summary.md` |
| `room_path(key, file)`  | `String`   | `/{root}/{key}/{file_path}`              |
| `parent_path(path)`     | `String`   | Strips last path segment                 |

## 4. NextCloud API Reference

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

The `X-NC-WebDAV-AutoMkcol` header (available since NextCloud 32) instructs the
server to automatically create any missing parent directories when uploading a
file. When this header is not supported (NextCloud < 32, or non-NextCloud
servers), the `WriteFileWithFallback` operation catches the 404 response,
explicitly creates parent directories via iterative `MKCOL`, then retries the
`PUT` without the header.
