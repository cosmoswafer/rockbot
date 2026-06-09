# Memory Management

## 1. Purpose

Per-room in-memory conversation store with character-count threshold monitoring.
When the local history exceeds the configured maximum, the oldest messages are
summarized via the AI provider into a compressed `.md` file and archived to the
room's WebDAV directory. On startup, recent archives are loaded back to seed
context.

- Upstream: [Configuration Management](config.md) provides `MemoryConfig`
- Upstream: [Agent Loop](agent-harness.md) loads archives on startup and
  triggers per-room history operations after each message
- Downstream: [WebDAV Storage](webdav.md) persists `.md` archive files
- Downstream: [AI Provider](ai-provider.md) is called to generate summaries

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    MSG[ChatMessage]
    STORE[(InMemoryHistory)]
    APPEND(AppendMessage)
    COUNT(CheckCharCount)
    THRESHOLD(AssessThreshold)
    SUMMARIZE(SummarizeOldest)
    ARCHIVE(WriteArchive)
    PRUNE(PruneSummarized)
    WEBDAV[(WebDAV Archive Dir)]
    AI[AiProvider]
    LOAD(LoadRecentArchives)
    INIT(Initialize)

    MSG -->|"chat message"| APPEND
    APPEND -->|"stored message"| STORE
    APPEND -->|"updated char count"| COUNT
    COUNT -->|"char count + threshold config"| THRESHOLD
    THRESHOLD -->|"overflow trigger + oldest messages"| SUMMARIZE
    STORE -->|"oldest messages"| SUMMARIZE
    SUMMARIZE -->|"summary prompt"| AI
    AI -->|"summary text"| SUMMARIZE
    SUMMARIZE -->|"summary.md content"| ARCHIVE
    ARCHIVE -->|"summary.md + room path"| WEBDAV
    ARCHIVE -->|"archive confirmation"| PRUNE
    STORE -->|"pruned message ids"| PRUNE
    INIT -->|"room id"| LOAD
    LOAD -->|"get *.md request"| WEBDAV
    WEBDAV -->|"archive files"| LOAD
    LOAD -->|"archived messages"| STORE
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    SUMMARIZE(SummarizeOldest)
    ARCHIVE(WriteArchive)
    AI[AiProvider]
    WEBDAV[(WebDAV)]
    DEFER(DeferArchive)
    TRUNCATE(TruncateOldest)
    STORE[(InMemoryHistory)]

    AI -.->|"error: summarization failed"| DEFER
    DEFER -.->|"retry signal"| SUMMARIZE
    ARCHIVE -.->|"error: webdav put failed"| DEFER
    DEFER -.->|"truncation trigger"| TRUNCATE
    STORE -->|"oldest messages to drop"| TRUNCATE
```

### 2c. Memory Partitioning Deep Dive

Each room (channel or DM) gets an isolated memory partition with its own
in-memory history and WebDAV archive directory.

```mermaid
flowchart TD
    ROOM_A[general]
    ROOM_B[dm-alice]
    ROOM_C[project-x]
    MEM_A[(InMemory A)]
    MEM_B[(InMemory B)]
    MEM_C[(InMemory C)]
    DAV_A[(WebDAV general/memory)]
    DAV_B[(WebDAV dm-alice/memory)]
    DAV_C[(WebDAV project-x/memory)]

    ROOM_A -->|"messages"| MEM_A
    ROOM_B -->|"messages"| MEM_B
    ROOM_C -->|"messages"| MEM_C
    MEM_A -->|"archive files"| DAV_A
    MEM_B -->|"archive files"| DAV_B
    MEM_C -->|"archive files"| DAV_C
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
