# WebDAV Storage

## 1. Purpose

Thin abstraction over HTTP-based WebDAV (NextCloud) providing typed
read/write/list/mkdir operations. All bot state — configuration backups, memory
archives, and image assets — is stored remotely; the bot never writes to local
disk. Each room gets its own directory subtree.

- Upstream: [Configuration Management](config.md) provides `WebDavConfig`
- Upstream: [Memory Management](memory.md) stores and retrieves `.md` archives
- Upstream: [Agent Loop](agent-harness.md) (vision tool) reads images from WebDAV

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
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]

    CALLER -->|"path + operation"| RESOLVE
    CFG -->|"root + credentials"| RESOLVE
    RESOLVE -->|"get request"| READ
    RESOLVE -->|"put request"| WRITE
    RESOLVE -->|"propfind request"| LIST
    RESOLVE -->|"mkcol request"| MKDIR
    READ -->|"GET"| HTTP
    WRITE -->|"PUT with body"| HTTP
    LIST -->|"PROPFIND depth=1"| HTTP
    MKDIR -->|"MKCOL"| HTTP
    HTTP -->|"http request"| NC
    NC -->|"response"| HTTP
    HTTP -->|"response body / status"| RESOLVE
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

    HTTP -.->|"401 unauthorized"| AUTH_REFRESH
    AUTH_REFRESH -.->|"refreshed auth"| RETRY
    HTTP -->|"404 not found"| ERR_404
    HTTP -.->|"connection refused"| RETRY
    RETRY -.->|"retries exhausted"| ERR_NET
    WRITE -.->|"parent dir missing"| MKDIR
    MKDIR -.->|"mkcol success"| WRITE
```

### 2c. Directory Structure Deep Dive

```mermaid
flowchart TD
    ROOT[(WebDAV Root)]
    CH[(ch-general)]
    CH2[(ch-project-x)]
    DM[(dm-alice)]
    MEM_CH[(general/memory)]
    MEM_CH2[(project-x/memory)]
    MEM_DM[(alice/memory)]
    IMG_CH[(general/images)]
    IMG_CH2[(project-x/images)]
    IMG_DM[(alice/images)]
    WSP_CH[(general/workspace)]
    WSP_CH2[(project-x/workspace)]
    WSP_DM[(alice/workspace)]
    CFG_DIR[(Config Backups)]

    ROOT -->|"channel messages + assets"| CH
    ROOT -->|"channel messages + assets"| CH2
    ROOT -->|"dm messages + assets"| DM
    ROOT -->|"config backups"| CFG_DIR
    CH -->|"memory archives"| MEM_CH
    CH -->|"image assets"| IMG_CH
    CH -->|"workspace files"| WSP_CH
    CH2 -->|"memory archives"| MEM_CH2
    CH2 -->|"image assets"| IMG_CH2
    CH2 -->|"workspace files"| WSP_CH2
    DM -->|"memory archives"| MEM_DM
    DM -->|"image assets"| IMG_DM
    DM -->|"workspace files"| WSP_DM
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
