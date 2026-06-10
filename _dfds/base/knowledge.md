# Knowledge Management

## 1. Purpose

Extracts remembered facts about users and rooms from conversation history and
stores them as indexed `.md` files on WebDAV. Two extraction triggers: explicit
`!remember` commands and frequency-based pattern detection. On room
initialization, knowledge entries are loaded into the agent context to inform
the LLM about user preferences, project context, and past decisions.

- Upstream: [Memory Management](memory.md) provides `ConversationHistory` as
  extraction source
- Upstream: [Configuration Management](config.md) provides `KnowledgeConfig`
- Downstream: [WebDAV Tool](../tools/webdav.md) persists `.md` files and
  `index.md`
- Downstream: [AI Provider](ai-provider.md) is called for extraction
  synthesis
- Downstream: [Agent Harness](../agent-harness.md) loads knowledge entries into
  `BuildContext` on room init

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    MSG[User Message]
    CMD(DetectMemorizeCommand)
    BG(PeriodicBackgroundScan)
    MEM[(ConversationHistory)]
    EXTRACT(ExtractKnowledgeEntry)
    AI[AiProvider]
    SYNTH(SynthesizeEntry)
    WRITE(WriteKnowledgeFile)
    UPDATE(UpdateIndex)
    IDX[(KnowledgeIndex)]
    DAV[(WebDAV knowledge/)]
    LOAD(LoadRoomKnowledge)
    CTX(BuildContext)
    INIT(RoomInitialization)

    MSG -->|"message text"| CMD
    CMD -->|"explicit memorize trigger"| EXTRACT
    MEM -->|"recent messages"| BG
    BG -->|"frequency trigger"| EXTRACT
    MEM -->|"conversation segment"| EXTRACT
    EXTRACT -->|"extraction prompt"| AI
    AI -->|"extracted facts"| SYNTH
    SYNTH -->|"knowledge entry"| WRITE
    WRITE -->|"knowledge.md file"| DAV
    WRITE -->|"entry metadata"| UPDATE
    IDX -->|"current index"| UPDATE
    UPDATE -->|"updated index"| IDX
    UPDATE -->|"index.md file"| DAV
    INIT -->|"room id"| LOAD
    LOAD -->|"get knowledge/ dir"| DAV
    DAV -->|"index + entry files"| LOAD
    LOAD -->|"knowledge entries"| CTX
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    AI[AiProvider]
    EXTRACT(ExtractKnowledgeEntry)
    DAV[(WebDAV)]
    WRITE(WriteKnowledgeFile)
    UPDATE(UpdateIndex)
    LOAD(LoadRoomKnowledge)
    ERR_EXTRACT[Error: Extraction Failed]
    ERR_WRITE[Error: Write Failed]
    ERR_LOAD[Error: Knowledge Unavailable]
    SKIP(SkipEntry)
    RETRY(RetryLater)
    WARN(WarnEmptyContext)

    AI -.->|"api error"| ERR_EXTRACT
    ERR_EXTRACT -->|"log + skip"| SKIP
    WRITE -.->|"webdav put failed"| ERR_WRITE
    ERR_WRITE -->|"defer"| RETRY
    UPDATE -.->|"webdav put failed"| ERR_WRITE
    LOAD -.->|"webdav get failed"| ERR_LOAD
    ERR_LOAD -->|"proceed without knowledge"| WARN
```

### 2c. Knowledge Extraction Deep Dive

Two extraction triggers — explicit user command (`!remember`) and periodic
frequency scan — feed the same synthesis pipeline.

```mermaid
flowchart TD
    MSG[User Message]
    PARSE(ParseMemorizeCommand)
    DETECT(DetectFrequencyPatterns)
    MEM[(ConversationHistory)]
    SCOPE(SelectConversationSegment)
    PROMPT(BuildExtractionPrompt)
    AI[AiProvider]
    PARSE_RESULT(ParseAiResponse)
    ENTRY[KnowledgeEntry]
    NOISE(DiscardNoise)
    MERGE(MergeWithExisting)
    EXISTING[(Existing Entry)]

    MSG -->|"!remember / please remember"| PARSE
    PARSE -->|"content to memorize"| SCOPE
    MEM -->|"recent messages"| DETECT
    DETECT -->|"repeated topic + threshold"| SCOPE
    MEM -->|"full conversation"| SCOPE
    SCOPE -->|"selected messages"| PROMPT
    PROMPT -->|"extraction prompt"| AI
    AI -->|"structured facts json"| PARSE_RESULT
    PARSE_RESULT -->|"valid entry"| MERGE
    PARSE_RESULT -->|"nothing worth keeping"| NOISE
    EXISTING -->|"previous entry"| MERGE
    MERGE -->|"updated entry"| ENTRY
```

### 2d. Knowledge Index Structure

The `index.md` file maps topics to individual knowledge `.md` files, enabling
selective loading without scanning the entire directory.

```mermaid
flowchart TD
    IDX[(knowledge/index.md)]
    PREFS[user_preferences.md]
    PROJ[project_context.md]
    TOPICS[frequent_topics.md]
    MEMOS[explicit_memos.md]
    DECIS[decisions.md]
    ROOT[(WebDAV knowledge/)]

    IDX -->|"preferences"| PREFS
    IDX -->|"context"| PROJ
    IDX -->|"topics"| TOPICS
    IDX -->|"memos"| MEMOS
    IDX -->|"decisions"| DECIS
    ROOT -->|"contains"| IDX
    ROOT -->|"contains"| PREFS
    ROOT -->|"contains"| PROJ
    ROOT -->|"contains"| TOPICS
    ROOT -->|"contains"| MEMOS
    ROOT -->|"contains"| DECIS
```

## 3. Data Structures

### `KnowledgeEntry`

Single `.md` file containing extracted facts on one topic.

| Field      | Type         | Notes                                     |
| ---------- | ------------ | ----------------------------------------- |
| `id`       | `String`     | Unique entry identifier (slug)            |
| `room_id`  | `String`     | Owning room (`"global"` for cross-room)   |
| `topic`    | `String`     | Human-readable topic title                |
| `content`  | `String`     | Markdown body with extracted facts        |
| `source`   | `SourceType` | `"explicit"` or `"frequency"`             |
| `created`  | `String`     | ISO 8601 timestamp                        |
| `updated`  | `String`     | ISO 8601 timestamp                        |

### `KnowledgeIndex`

Single `index.md` file listing all entries in a room's knowledge directory.

| Field      | Type               | Notes                         |
| ---------- | ------------------ | ----------------------------- |
| `entries`  | `Vec<IndexEntry>`  | Descriptors for every entry   |
| `updated`  | `String`           | Last modification timestamp   |

### `IndexEntry`

| Field      | Type         | Notes                                    |
| ---------- | ------------ | ---------------------------------------- |
| `id`       | `String`     | Matches `KnowledgeEntry.id`              |
| `filename` | `String`     | `{topic_slug}.md`                        |
| `topic`    | `String`     | Human-readable topic                     |
| `tags`     | `Vec<String>`| Searchable tags for context injection    |
| `source`   | `SourceType` | How the entry was created                |

### `KnowledgeConfig`

Appended to `AppConfig` in [Configuration Management](config.md).

| Field                 | Type   | Notes                                     |
| --------------------- | ------ | ----------------------------------------- |
| `knowledge_enabled`   | `bool` | Enable knowledge extraction               |
| `frequency_threshold` | `usize`| Mention count before auto-extraction      |
| `scan_interval`       | `u64`  | Seconds between periodic scans            |

### File Layout

```
{root}/{room_id}/knowledge/index.md
{root}/{room_id}/knowledge/{topic_slug}.md
```

Example:

```
rockbot/general/knowledge/index.md
rockbot/general/knowledge/user_preferences.md
rockbot/general/knowledge/project_context.md
```
