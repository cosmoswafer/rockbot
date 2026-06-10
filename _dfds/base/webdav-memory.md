# WebDAV Memory

## 1. Purpose

JSON conversation memory archives persisted on WebDAV with a local write-back
cache. Writes go to cache immediately and return to the caller without waiting
for the WebDAV round-trip. Dirty entries are flushed on a configurable sync
interval or graceful shutdown. Reads check the cache first; on miss, the file
is fetched from WebDAV and cached.

Archives are stored at `{root}/{room_id}/memory/{seq:06}_memory.json`. On
startup, recent archives are preloaded into the cache from WebDAV.

- Upstream: [Memory Management](memory.md) triggers archive writes when the
  conversation character-count threshold is exceeded, and loads recent
  archives on room init
- Upstream: [Configuration Management](config.md) provides `WebDavConfig`
- Downstream: [WebDAV Directory](webdav-directory.md) provides the underlying
  PUT/GET/PROPFIND operations

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CALLER[Calling Subsystem]
    CACHE[(MemoryCache)]
    PRELOAD(PreloadCache)
    WRITE(WriteMemoryJson)
    READ(ReadMemoryJson)
    LIST(ListArchives)
    SYNC(FlushDirtyCache)
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]

    CALLER -->|"room id"| PRELOAD
    PRELOAD -->|"fetch recent *.json"| HTTP
    HTTP -->|"json files"| PRELOAD
    PRELOAD -->|"populate cache"| CACHE
    CALLER -->|"archive + room id"| WRITE
    WRITE -->|"store json"| CACHE
    CALLER -->|"archive seq + room id"| READ
    READ -->|"cached json"| CACHE
    CALLER -->|"room id"| LIST
    LIST -->|"PROPFIND depth=1"| HTTP
    HTTP -->|"archive listing"| LIST
    SYNC -->|"dirty entries"| CACHE
    SYNC -->|"PUT batch"| HTTP
    HTTP -->|"http request"| NC
    NC -->|"response"| HTTP
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    CACHE[(MemoryCache)]
    READ(ReadMemoryJson)
    SYNC(FlushDirtyCache)
    HTTP(HttpClient)
    DEFER(DeferFlush)
    ERR_NET[Network Unreachable]

    READ -.->|"cache miss"| HTTP
    HTTP -.->|"fetch failed"| ERR_NET
    SYNC -.->|"PUT batch failed"| DEFER
    DEFER -.->|"retry signal"| SYNC
    DEFER -.->|"exit: log and drop"| ERR_NET
```

### 2c. Memory Cache Deep Dive

Write-back cache for JSON memory archives. Writes return immediately after
storing in local cache; a background timer or graceful-shutdown hook flushes
dirty entries to WebDAV. Reads hit the cache first and fetch from WebDAV
only on miss (populating the cache for subsequent reads). On startup, recent
archives are preloaded into the cache from WebDAV.

```mermaid
flowchart TD
    CACHE[(MemoryCache - HashMap)]
    DIRTY[(DirtySet - pending sync)]
    HTTP(HttpClient)
    NC[(NextCloud WebDAV)]
    TIMER[Sync Timer - configurable interval]
    EXIT[Exit Hook - graceful shutdown]
    PRELOAD(Preload on Startup)

    subgraph WritePath["Write Path (write-back)"]
        direction LR
        W_STORE(Store in Cache)
        W_MARK(Mark Entry Dirty)
        W_RETURN(Return to Caller)
    end

    subgraph ReadPath["Read Path"]
        direction LR
        R_CHECK{In Cache?}
        R_HIT(Return Cached)
        R_MISS(Fetch from WebDAV)
        R_STORE(Store in Cache)
        R_RETURN(Return to Caller)
    end

    subgraph SyncPath["Sync Path (timeout or exit)"]
        direction LR
        S_SCAN(Iterate DirtySet)
        S_PUT(PUT to WebDAV)
        S_CLEAR(Clear DirtyFlag)
    end

    PRELOAD -->|"load recent archives"| HTTP
    HTTP -->|"json files"| PRELOAD
    PRELOAD -->|"populate cache"| CACHE
    W_STORE --> CACHE
    W_MARK --> DIRTY
    TIMER -->|"trigger flush"| SyncPath
    EXIT -->|"trigger flush"| SyncPath
    S_SCAN --> DIRTY
    S_PUT --> HTTP
    HTTP --> NC
    R_CHECK -->|"yes"| R_HIT
    R_CHECK -->|"no"| R_MISS
    R_MISS --> HTTP
    HTTP --> R_STORE
    R_STORE --> CACHE
```

## 3. Data Structures

#### `MemoryJson`

Code-level name for `MemoryArchive` — see [Memory Management](memory.md#memoryarchive)
for the full field definitions. Each file is persisted at
`{root}/{room_id}/memory/{seq:06}_memory.json`.

#### `MemoryCache`

Per-room write-back cache for `MemoryJson` archive files. Writes go to
cache immediately; dirty entries are flushed to WebDAV on a configurable
sync interval or on graceful shutdown.

| Field           | Type                       | Notes                                       |
| --------------- | -------------------------- | ------------------------------------------- |
| `entries`       | `HashMap<String, CachedEntry>`| Path → cached JSON + dirty flag          |
| `sync_interval` | `Duration`                 | How often to flush dirty entries (default 30s)|
| `sync_handle`   | `Option<JoinHandle<()>>`   | Background sync task handle                 |

#### `CachedEntry`

| Field       | Type         | Notes                                    |
| ----------- | ------------ | ---------------------------------------- |
| `data`      | `MemoryJson` | Parsed archive content                   |
| `dirty`     | `bool`       | True if cache is ahead of WebDAV         |
| `cached_at` | `Instant`    | When the entry was last loaded/updated   |

#### `WebDavPath` (memory methods)

| Method                   | Returns  | Notes                                       |
| ------------------------ | -------- | ------------------------------------------- |
| `memory_dir(key)`        | `String` | `/{root}/{key}/memory/`                     |
| `archive_path(key, seq)` | `String` | `/{root}/{key}/memory/{seq:06}_memory.json` |

## 4. NextCloud API Reference

Memory operations route through the local `MemoryCache` layer before touching
WebDAV. Writes are immediate to cache; sync to WebDAV happens on timer or exit.

| DFD Operation       | HTTP Method | NextCloud Endpoint                                  | Notes                              |
| ------------------- | ----------- | --------------------------------------------------- | ---------------------------------- |
| WriteMemoryJson     | `PUT`       | `{base}/files/{user}/{root}/{room}/memory/{seq:06}_memory.json` | Serialized `MemoryJson` — via cache write-back |
| ReadMemoryJson      | `GET`       | `{base}/files/{user}/{root}/{room}/memory/{seq:06}_memory.json` | Returns `MemoryJson` — cache-hit or fetch |
| ListMemoryArchives  | `PROPFIND`  | `{base}/files/{user}/{root}/{room}/memory/`          | `Depth: 1` — filter `*.json`      |
| FlushDirtyCache     | —           | — (local operation, triggers `PUT` batch)            | Called by sync timer or exit hook  |
| PreloadCache        | —           | — (local, fetches recent `*.json`)                   | Called on room init after restart  |
