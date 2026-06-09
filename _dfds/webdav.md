# WebDAV Storage

## 1. Purpose

Thin abstraction over HTTP-based WebDAV (NextCloud) providing typed
read/write/list/mkdir operations. All bot state — configuration backups, memory
archives, and image assets — is stored remotely; the bot never writes to local
disk. Each room gets its own directory subtree.

- Upstream: [Configuration Management](config.md) provides `WebDavConfig`
- Upstream: [Memory Management](memory.md) stores and retrieves `.md` archives
- Upstream: [Agent Tools](agent.md) (vision) reads images from WebDAV

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CALLER[Calling Subsystem]
    RESOLVE(ResolvePath)
    OP{Operation}
    READ(ReadFile)
    WRITE(WriteFile)
    LIST(ListDirectory)
    MKDIR(EnsureDirectory)
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]
    CFG[WebDavConfig]

    CALLER -->|"path + op"| RESOLVE
    CFG -->|"root + credentials"| RESOLVE
    RESOLVE -->|"full WebDAV URL"| OP
    OP -->|"GET"| READ
    OP -->|"PUT"| WRITE
    OP -->|"PROPFIND"| LIST
    OP -->|"MKCOL"| MKDIR
    READ -->|"GET"| HTTP
    WRITE -->|"PUT with body"| HTTP
    LIST -->|"PROPFIND depth=1"| HTTP
    MKDIR -->|"MKCOL"| HTTP
    HTTP -->|"HTTP request"| NC
    NC -->|"response"| HTTP
    HTTP -->|"response body / status"| OP
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]
    AUTH_REFRESH(RefreshAuth)
    RETRY(RetryWithBackoff)
    ERR_404[Error: Path Not Found]
    ERR_NET[Error: Network Unreachable]
    MKDIR(EnsureDirectory)
    WRITE(WriteFile)

    HTTP -->|"401 Unauthorized"| AUTH_REFRESH
    AUTH_REFRESH -->|"re-read config"| RETRY
    HTTP -->|"404 Not Found"| ERR_404
    HTTP -->|"connection refused"| RETRY
    RETRY -->|"max retries"| ERR_NET
    WRITE -->|"parent dir missing"| MKDIR
    MKDIR -->|"MKCOL success"| WRITE
```

### 2c. Directory Structure Deep Dive

```mermaid
flowchart TD
    ROOT["/rockbot/"]
    GEN["general/"]
    DM["dm-alice/"]
    PROJ["project-x/"]
    MEM_GEN["memory/"]
    MEM_DM["memory/"]
    MEM_PROJ["memory/"]
    IMG_GEN["images/"]
    IMG_DM["images/"]
    CFG_DIR["_config/"]

    ROOT --> GEN
    ROOT --> DM
    ROOT --> PROJ
    ROOT --> CFG_DIR
    GEN --> MEM_GEN
    GEN --> IMG_GEN
    DM --> MEM_DM
    DM --> IMG_DM
    PROJ --> MEM_PROJ
    CFG_DIR -->|"config.json.bak"| ROOT
```

## 3. Data Structures

#### `WebDavClient`

| Field       | Type              | Notes                                  |
| ----------- | ----------------- | -------------------------------------- |
| `base_url`  | `String`          | WebDAV endpoint                        |
| `root`      | `String`          | Base directory path                    |
| `auth`      | `BasicAuth`       | Username + app password                |
| `client`    | `reqwest::Client` | Shared HTTP client with connection pool|

#### `WebDavEntry`

| Field       | Type     | Notes                                      |
| ----------- | -------- | ------------------------------------------ |
| `name`      | `String` | File or directory name                     |
| `href`      | `String` | Full WebDAV href                           |
| `is_dir`    | `bool`   | True if collection (directory)             |
| `size`      | `u64`    | File size in bytes (0 for dirs)            |
| `modified`  | `String` | Last-modified timestamp                    |

#### `WebDavPath`

| Method           | Returns    | Notes                                    |
| ---------------- | ---------- | ---------------------------------------- |
| `room_dir(id)`   | `String`   | `/{root}/{room_id}/`                     |
| `memory_dir(id)` | `String`   | `/{root}/{room_id}/memory/`              |
| `image_path(id, name)` | `String` | `/{root}/{room_id}/images/{name}`  |
| `archive_path(id, seq)` | `String` | `/{root}/{room_id}/memory/{seq}_summary.md` |
