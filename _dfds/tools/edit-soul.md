# Edit Soul

## 1. Purpose

Manages the bot's permanent per-room "soul" memory — a single `soul.md` file
stored on WebDAV under `{room}/memory/soul.md`. Supports three operations:
`append` (add a new `## Section`), `replace` (update an existing section's
content), and `delete_section` (remove a section entirely).

- Upstream: [Configuration Management](../base/config.md) provides WebDAV
  credentials for file access
- Upstream: [Agent Harness](../agent-harness.md) invokes `EditSoulTool` with
  action, section_header, and optional content
- Downstream: [WebDAV Tool](webdav.md) performs GET/PUT operations against
  the soul.md file
- Downstream: [Memory Management](../base/memory.md) — soul.md lives alongside
  other per-room memory archives under `{room}/memory/`

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent Harness]
    CFG[(WebDavConfig)]
    SOUL(EditSoulTool)
    GET(ReadSoulMd)
    PARSE(ParseSoulSection)
    APPEND(DoAppend)
    REPLACE(DoReplace)
    DELETE(DoDeleteSection)
    PUT(WriteSoulMd)
    HTTP(HttpClient)
    DAV[(NextCloud WebDAV)]
    AI[AiProvider]

    AGENT -->|"action + section_header + content"| SOUL
    CFG -->|"root + credentials"| SOUL
    SOUL -->|"GET memory/soul.md"| GET
    GET -->|"GET /{room}/memory/soul.md"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"200 soul.md body"| GET
    GET -->|"existing content"| PARSE
    SOUL -->|"action=append"| APPEND
    SOUL -->|"action=replace"| REPLACE
    SOUL -->|"action=delete_section"| DELETE
    PARSE -->|"section marker found"| APPEND
    PARSE -->|"existing section located"| REPLACE
    PARSE -->|"existing section located"| DELETE
    APPEND -->|"## Section header + body"| PUT
    REPLACE -->|"modified content"| PUT
    DELETE -->|"section removed"| PUT
    PUT -->|"PUT soul.md"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"204 / 201"| PUT
    PUT -->|"confirmation message"| AGENT
    AGENT -->|"tool result"| AI
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    SOUL(EditSoulTool)
    GET(ReadSoulMd)
    PARSE(ParseSoulSection)
    PUT(WriteSoulMd)
    HTTP(HttpClient)
    DAV[(NextCloud WebDAV)]
    ERR_NOT_FOUND[Error: Section Not Found]
    ERR_READ[Error: WebDAV Read Failed]
    ERR_WRITE[Error: WebDAV Write Failed]
    ERR_ACTION[Error: Unknown Action]
    NEW_FILE(CreateFreshSoulMd)
    RETRY(RetryWrite)
    AGENT[Agent Harness]

    GET -.->|"404 (no soul.md yet)"| NEW_FILE
    NEW_FILE -->|"empty skeleton"| PUT
    REPLACE -.->|"section not in content"| ERR_NOT_FOUND
    DELETE -.->|"section not in content"| ERR_NOT_FOUND
    SOUL -.->|"action != append/replace/delete_section"| ERR_ACTION
    HTTP -.->|"connection error / timeout"| ERR_READ
    PUT -.->|"write failure"| RETRY
    RETRY -.->|"still fails"| ERR_WRITE
    ERR_NOT_FOUND -->|"error string"| AGENT
    ERR_ACTION -->|"error string"| AGENT
    ERR_READ -->|"error string"| AGENT
    ERR_WRITE -->|"error string"| AGENT
```

### 2c. Append vs Replace Deep Dive

```mermaid
flowchart TD
    CALL[ToolCall]
    EXIST{File exists?}
    EMPTY[Create skeleton]
    FM[Read full content]
    PARSE2{Section exists?}
    APPEND_OP[Append ## Section + body]
    SKIP[Skip header in existing]
    REPLACE_OP["Locate section bounds (next ## or EOF)"]
    SWAP[Replace section body]
    RESULT[Write result]

    CALL --> EXIST
    EXIST -->|"no"| EMPTY
    EXIST -->|"yes"| FM
    EMPTY -->|"# Soul Memory skeleton"| APPEND_OP
    FM --> PARSE2

    PARSE2 -->|"append: section may or may not exist"| APPEND_OP
    APPEND_OP -->|"append section content"| RESULT
    PARSE2 -->|"append with existing section"| SKIP
    SKIP -->|"skip duplicate"| APPEND_OP
    PARSE2 -->|"replace: section must exist"| REPLACE_OP
    REPLACE_OP -->|"extract section body"| SWAP
    SWAP -->|"inject new content"| RESULT
    PARSE2 -->|"delete_section: section must exist"| REPLACE_OP
    REPLACE_OP -->|"extract bounds"| SWAP
    SWAP -->|"remove section content"| RESULT
```

## 3. Data Structures

#### `EditSoulParams`

| Field            | Type     | Notes                                                    |
| ---------------- | -------- | -------------------------------------------------------- |
| `action`         | `string` | `"append"`, `"replace"`, or `"delete_section"`           |
| `section_header` | `string` | Target `## Section` name (without `## ` prefix)          |
| `content`        | `string` | New body text (required for append/replace)              |
| `webdav_dir`     | `string` | Room WebDAV directory key (injected automatically)       |

#### Soul File Format

Stored at `/{root}/{webdav_dir}/memory/soul.md`:

```markdown
# Soul Memory

## Preferences
Prefer concise responses with code examples.

## Identity
You are a helpful bot assistant.
```

#### Soul Operations

| Operation        | Inputs                    | Behavior                                                    |
| ---------------- | ------------------------- | ----------------------------------------------------------- |
| `append`         | section_header, content   | Adds a new `## {header}` section at the end of the file     |
| `replace`        | section_header, content   | Finds existing `## {header}` and replaces its body          |
| `delete_section` | section_header            | Finds existing `## {header}` and removes it entirely        |
