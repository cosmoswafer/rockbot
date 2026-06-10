# Agent Harness

## 1. Purpose

The operational environment that wraps the agent loop — the invariant core
cycle of `LLM → tools → LLM → ...`. The harness layers Tools, Knowledge, and
Context around this loop without modifying it.

### 1a. Micro Harness Scope

rockbot implements a **micro harness**: a minimal harness with only the
mechanisms needed for a single-agent, single-channel chatbot. Three of the six
standard harness mechanisms are present:

| Mechanism   | Coverage | Details |
|-------------|----------|---------|
| **Tools**   | Full     | `web_search`, `web_fetch`, `vision`, `webdav`, image generation — each tool has its own DFD |
| **Knowledge** | Full  | On-demand context via [Memory Management](base/memory.md): archive loading, conversation history, system prompt assembly |
| **Context** | Full     | Per-room history, loop iteration limits, truncation/summarization, room state routing |

Intentionally absent — not needed for rockbot's scope:

| Mechanism       | Reason |
|-----------------|--------|
| **Permissions** | Single-user bot — no sandbox or approval flows |
| **Extensions**  | No plugin/hook system — tools are statically registered |
| **Coordination**| Single agent — no subagents, teams, or worktrees |

- Upstream: [Agent Loop](agent-loop.md) feeds `IncomingMessage`
  into the loop and consumes `BotReply`
- Downstream: [AI Provider](base/ai-provider.md) receives `ChatRequest` and returns
  `CompletionResult` with tool calls or final text
- Downstream: [Memory Management](base/memory.md) provides `ConversationHistory` per
  room and receives new messages for archival
- Downstream: [WebDAV Storage](base/webdav.md) persists generated image assets

## 2. Diagram

### 2a. Agent Loop (Main Success Path)

```mermaid
flowchart TD
    RC[RocketChat]
    ROUTE(RouteByRoom)
    CTX(BuildContext)
    MEM[(ConversationHistory)]
    TOOLS_DEF[(ToolRegistry)]
    INTERACT(InteractWithAi)
    AI[AiProvider]

    RC -->|"incoming message"| ROUTE
    ROUTE -->|"routed message"| CTX
    MEM -->|"history for room"| CTX
    TOOLS_DEF -->|"tool definitions"| CTX
    CTX -->|"chat request"| INTERACT
    INTERACT -->|"chat request"| AI
    AI -->|"completion result"| INTERACT
    INTERACT -->|"bot reply"| RC
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    AI[AiProvider]
    TOOL_EXEC(ExecuteTool)
    LOOP_LIMIT(CheckMaxIterations)
    FALLBACK(SendFallbackReply)
    TRUNC(TruncateAndSummarize)
    REPLY[BotReply]

    AI -.->|"api error response"| FALLBACK
    TOOL_EXEC -.->|"tool execution error"| FALLBACK
    FALLBACK -->|"error reply text"| REPLY
    LOOP_LIMIT -.->|"overflow context"| TRUNC
    TRUNC -->|"summarized reply"| REPLY
```

### 2c. Agent Loop Deep Dive

Level 2 decomposition of the invariant agent loop (`while True: LLM → tools →
LLM`): queries the AI provider, executes any tool calls, feeds results back, and
loops until a final text reply is produced.

```mermaid
flowchart TD
    CTX[BuildContext]
    AI[AiProvider]
    ASSESS(AssessCompletion)
    EXEC(ExecuteTool)
    APPEND(AppendToolResult)
    LIMIT(CheckIterationLimit)
    TRUNC(TruncateAndSummarize)
    REPLY_OUT[BotReply]

    CTX -->|"chat request"| AI
    AI -->|"completion result"| ASSESS
    ASSESS -->|"tool calls"| EXEC
    ASSESS -->|"final reply text"| REPLY_OUT
    EXEC -->|"tool result"| APPEND
    APPEND -->|"updated messages"| CTX
    CTX -->|"iteration count"| LIMIT
    LIMIT -.->|"overflow context"| TRUNC
    TRUNC -->|"summarized messages"| CTX
    EXEC -.->|"tool execution error"| APPEND
    AI -.->|"api error"| REPLY_OUT
```

### 2d. Tool Execution Deep Dive

```mermaid
flowchart TD
    CALL[ToolCall]
    REG[(ToolRegistry)]
    EXEC(ExecuteToolByName)
    EXA[Exa API]
    WEB_URL[Remote URL]
    WEBDAV_IMG[(WebDAV Image)]
    IMG_API[Image Generation API]
    WEBDAV_STORE[(WebDAV images)]
    RESULT[ToolResult]

    CALL -->|"tool name + args"| EXEC
    REG -->|"tool definitions"| EXEC
    EXEC -->|"search query"| EXA
    EXA -->|"search results"| EXEC
    EXEC -->|"http get"| WEB_URL
    WEB_URL -->|"html"| EXEC
    EXEC -->|"download image"| WEBDAV_IMG
    WEBDAV_IMG -->|"image bytes"| EXEC
    EXEC -->|"generation prompt"| IMG_API
    IMG_API -->|"image bytes"| EXEC
    EXEC -->|"image asset"| WEBDAV_STORE
    WEBDAV_STORE -->|"image url"| EXEC
    EXEC -->|"formatted result"| RESULT
```

### 2e. Image Generation Pipeline

Both `infograph` and `anime` share the same pipeline; only the system prompt
and style prefix differ.

```mermaid
flowchart TD
    PROMPT[Prompt]
    FORMAT(FormatStyledPrompt)
    API[Image Generation API]
    BYTES(ReceiveImageBytes)
    NAME(GenerateFilename)
    PUT(PutToWebDAV)
    DAV[(WebDAV images)]
    RESULT[ToolResult]

    PROMPT -->|"prompt"| FORMAT
    FORMAT -->|"styled prompt"| API
    API -->|"png bytes"| BYTES
    BYTES -->|"image bytes"| NAME
    NAME -->|"filename"| PUT
    DAV -->|"storage target"| PUT
    PUT -->|"webdav image url"| RESULT
```

### 2f. Per-Room State Routing

Each room maintains independent state — conversation history, agent context, and
WebDAV archive path. The agent routes incoming messages to the correct room's
pipeline.

```mermaid
flowchart TD
    RC[IncomingMessage]
    ROOM_MAP[(RoomStateMap)]
    RESOLVE(ResolveRoomState)
    NEW_ROOM(InitializeRoom)
    MEM[(InMemoryHistory)]
    INACT(InteractWithAi)
    REPLY[BotReply]
    DAV[(WebDAV room memory)]
    DAV_IMG[(WebDAV room images)]

    RC -->|"room id"| ROOM_MAP
    ROOM_MAP -->|"room state or not found"| RESOLVE
    RESOLVE -->|"new room context"| NEW_ROOM
    RESOLVE -->|"existing room context"| INACT
    NEW_ROOM -->|"load archives request"| DAV
    DAV -->|"archive files"| NEW_ROOM
    NEW_ROOM -->|"archived messages"| MEM
    NEW_ROOM -->|"initialized state"| ROOM_MAP
    MEM -->|"conversation history"| INACT
    INACT -->|"bot reply"| REPLY
    INACT -->|"generated image"| DAV_IMG
```

## 3. Data Structures

#### `AgentContext`

| Field           | Type                  | Notes                              |
| --------------- | --------------------- | ---------------------------------- |
| `system_prompt` | `String`              | Bot personality and instructions   |
| `history`       | `Vec<ChatMessage>`    | Conversation history for room      |
| `tools`         | `Vec<ToolDef>`        | Registered tool definitions        |
| `room_id`       | `String`              | Source room/DM identifier          |

#### `ToolResult`

| Field      | Type     | Notes                                      |
| ---------- | -------- | ------------------------------------------ |
| `call_id`  | `String` | Matches `ToolCall.id`                      |
| `name`     | `String` | Tool name                                  |
| `content`  | `String` | Result text (returned to LLM as tool msg)  |
| `is_error` | `bool`   | True if tool execution failed              |

#### `ToolRegistry`

| Field      | Type                    | Notes                          |
| ---------- | ----------------------- | ------------------------------ |
| `tools`    | `HashMap<String, Box<dyn Tool>>` | Name → implementation |

#### `ToolDef`

| Field        | Type     | Notes                                   |
| ------------ | -------- | --------------------------------------- |
| `name`       | `String` | Function name                           |
| `description`| `String` | Human-readable description for the LLM  |
| `parameters` | `Value`  | JSON Schema for arguments               |

#### Registered Tools

| Tool Name     | Description                                      | Arguments                          |
| ------------- | ------------------------------------------------ | ---------------------------------- |
| `web_search`  | Search the web using Exa                         | `query: string`                    |
| `web_fetch`   | Fetch a URL, optionally as markdown              | `url: string, markdown: bool`      |
| `vision`      | Download an image and report metadata _(true vision — sending image data to AI provider — is planned)_ | `url: string, prompt: string`      |
| `infograph`   | _(planned)_ Generate an infographic image        | `prompt: string`                   |
| `anime`       | _(planned)_ Generate a Japanese anime-style image | `prompt: string`                  |
