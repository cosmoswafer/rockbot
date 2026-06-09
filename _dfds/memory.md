# Memory Management

## 1. Purpose

Per-room in-memory conversation store with character-count threshold monitoring.
When the local history exceeds the configured maximum, the oldest messages are
summarized via the AI provider into a compressed `.md` file and archived to the
room's WebDAV directory. On startup, recent archives are loaded back to seed
context.

- Upstream: [Configuration Management](config.md) provides `MemoryConfig`
- Upstream: [Agent Harness](agent-harness.md) loads archives on startup and
  triggers per-room history operations after each message
- Downstream: [WebDAV Storage](webdav.md) persists `.md` archive files
- Downstream: [AI Provider](ai-provider.md) is called to generate summaries

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    MSG[New ChatMessage]
    STORE[(InMemoryHistory)]
    APPEND(AppendMessage)
    COUNT(CheckCharCount)
    THRESHOLD{ExceedsThreshold?}
    SUMMARIZE(SummarizeOldest)
    ARCHIVE(WriteArchive)
    PRUNE(PruneSummarized)
    WEBDAV[(WebDAV Archive Dir)]
    AI(AiProvider)
    LOAD(LoadRecentArchives)
    STARTUP[Bot Startup]
    ROOM_ID[Room ID]

    MSG -->|"message"| APPEND
    APPEND -->|"store"| STORE
    APPEND -->|"total chars"| COUNT
    COUNT -->|"char count"| THRESHOLD
    THRESHOLD -->|"no"| STORE
    THRESHOLD -->|"yes"| SUMMARIZE
    STORE -->|"oldest messages"| SUMMARIZE
    SUMMARIZE -->|"summary prompt"| AI
    AI -->|"summary text"| SUMMARIZE
    SUMMARIZE -->|"summary.md"| ARCHIVE
    ROOM_ID -->|"/{room_id}/memory/"| WEBDAV
    ARCHIVE -->|"PUT summary.md"| WEBDAV
    ARCHIVE -->|"done"| PRUNE
    STORE -->|"remove summarized msgs"| PRUNE
    STARTUP -->|"room_id"| LOAD
    LOAD -->|"GET *.md"| WEBDAV
    WEBDAV -->|"archive files"| LOAD
    LOAD -->|"seed history"| STORE
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    SUMMARIZE(SummarizeOldest)
    ARCHIVE(WriteArchive)
    AI(AiProvider)
    WEBDAV[(WebDAV)]
    DEFER(DeferArchive)
    TRUNCATE(TruncateOldest)
    STORE[(InMemoryHistory)]

    AI -->|"summarization failed"| DEFER
    DEFER -->|"retry next cycle"| SUMMARIZE
    ARCHIVE -->|"WebDAV PUT failed"| DEFER
    DEFER -->|"memory still over limit"| TRUNCATE
    STORE -->|"drop oldest messages"| TRUNCATE
```

### 2c. Memory Partitioning Deep Dive

Each room (channel or DM) gets an isolated memory partition with its own
in-memory history and WebDAV archive directory.

```mermaid
flowchart TD
    ROOM_A[Room: general]
    ROOM_B[Room: DM-alice]
    ROOM_C[Room: project-x]
    MEM_A[(InMemory A)]
    MEM_B[(InMemory B)]
    MEM_C[(InMemory C)]
    DAV_A["/webdav/general/memory/"]
    DAV_B["/webdav/DM-alice/memory/"]
    DAV_C["/webdav/project-x/memory/"]

    ROOM_A --> MEM_A
    ROOM_B --> MEM_B
    ROOM_C --> MEM_C
    MEM_A -->|"archive"| DAV_A
    MEM_B -->|"archive"| DAV_B
    MEM_C -->|"archive"| DAV_C
```

## 3. Data Structures

#### `ConversationHistory`

| Field          | Type                  | Notes                               |
| -------------- | --------------------- | ----------------------------------- |
| `room_id`      | `String`              | Owning room identifier              |
| `messages`     | `Vec<ChatMessage>`    | In-memory message buffer            |
| `char_count`   | `usize`               | Running character count             |
| `archive_seq`  | `u64`                 | Next archive sequence number        |

#### `ArchiveEntry`

| Field        | Type     | Notes                                       |
| ------------ | -------- | ------------------------------------------- |
| `seq`        | `u64`    | Sequence number (zero-padded for ordering)  |
| `summary`    | `String` | Markdown-formatted conversation summary     |
| `date_range` | `String` | `"2026-06-01 to 2026-06-08"`               |
| `msg_count`  | `usize`  | Number of messages summarized               |

#### Archive File Naming

```
{root}/{room_id}/memory/{seq:06}_summary.md
```

Example: `rockbot/general/memory/000001_summary.md`
