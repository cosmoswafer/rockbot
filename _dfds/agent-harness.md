# Agent Loop

## 1. Purpose

The agent loop — the core event-driven pipeline that receives incoming messages
from RocketChat, builds per-room context (system prompt + conversation history +
tool definitions), interacts with the AI provider in a tool-calling loop, and
returns replies. This IS the agent: a single loop that routes messages, maintains
per-room state, and orchestrates LLM interaction.

- Downstream: [RocketChat Connection](rocketchat.md) produces `IncomingMessage`
  events and consumes `BotReply` for delivery
- Downstream: [Memory Management](memory.md) provides `ConversationHistory` per
  room and receives new messages for archival
- Downstream: [AI Provider](ai-provider.md) receives `ChatRequest` and returns
  `CompletionResult` with tool calls or final text
- Downstream: [WebDAV Storage](webdav.md) persists generated image assets

## 2. Diagram

### 2a. Agent Loop (Main Success Path)

```mermaid
flowchart TD
    RC[RocketChat]
    EVT[IncomingMessage]
    ROUTE(RouteByRoom)
    CTX(BuildContext)
    MEM[(ConversationHistory)]
    TOOLS_DEF[(ToolRegistry)]
    INTERACT(InteractWithAi)
    AI[AiProvider]
    REPLY[BotReply]

    RC -->|"DM / @mention"| EVT
    EVT -->|"message + room_id"| ROUTE
    ROUTE -->|"routed message"| CTX
    MEM -->|"history for room"| CTX
    TOOLS_DEF -->|"tool definitions"| CTX
    CTX -->|"ChatRequest"| INTERACT
    INTERACT -->|"ChatRequest"| AI
    AI -->|"CompletionResult"| INTERACT
    INTERACT -->|"BotReply"| REPLY
    REPLY -->|"reply"| RC
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    INIT(InitializeSystems)
    RC_CONNECT[RocketChat Connection]
    RECONNECT(ReconnectWithBackoff)
    SHUTDOWN(GracefulShutdown)
    SIG[OS Signals]
    AI[AiProvider]
    FALLBACK(FallbackReply)
    REPLY[BotReply]

    INIT -->|"connection failed"| RECONNECT
    RC_CONNECT -->|"WebSocket closed"| RECONNECT
    RECONNECT -->|"exponential backoff"| RC_CONNECT
    RECONNECT -->|"max retries"| SHUTDOWN
    SIG -->|"SIGTERM / SIGINT"| SHUTDOWN
    AI -->|"API error"| FALLBACK
    FALLBACK -->|"error message"| REPLY
```

### 2c. LLM Interaction Deep Dive

Level 2 decomposition of `InteractWithAi`: the tool-calling loop that queries the
AI provider, executes any tool calls, feeds results back, and loops until a final
text reply is produced.

```mermaid
flowchart TD
    REQ[ChatRequest]
    AI[AiProvider]
    CHECK{HasToolCalls?}
    EXEC(ExecuteTool)
    RESULT[ToolResult]
    APPEND(AppendToolResult)
    REPLY[BotReply]
    MAX_LOOP{MaxIterations?}
    TRUNC(TruncateAndSummarize)

    REQ -->|"messages + tools"| AI
    AI -->|"CompletionResult"| CHECK
    CHECK -->|"no tool calls"| REPLY
    REPLY -->|"BotReply"| INTERACT
    CHECK -->|"tool_calls[]"| EXEC
    EXEC -->|"ToolResult"| RESULT
    RESULT -->|"tool result message"| APPEND
    APPEND -->|"updated messages"| REQ
    REQ -->|"loop count"| MAX_LOOP
    MAX_LOOP -->|"exceeded"| TRUNC
    TRUNC -->|"summarized context"| REQ
    EXEC -.->|"execution failed"| RESULT
    AI -.->|"API error"| REPLY
```

### 2d. Tool Execution Deep Dive

```mermaid
flowchart TD
    CALL[ToolCall]
    REG{ToolRegistry}
    EXA(ExaSearch)
    FETCH(WebFetch)
    VISION(VisionAnalyze)
    INFOGRAPH(InfographGen)
    ANIME(AnimeGen)
    RESULT[ToolResult]
    EXA_API[Exa API]
    WEB_URL[Remote URL]
    WEBDAV_IMG[WebDAV Image]
    IMG_API[Image Generation API]
    WEBDAV_STORE[WebDAV images/]

    CALL -->|"name"| REG
    REG -->|"web_search"| EXA
    REG -->|"web_fetch"| FETCH
    REG -->|"vision"| VISION
    REG -->|"infograph"| INFOGRAPH
    REG -->|"anime"| ANIME
    EXA -->|"search query"| EXA_API
    EXA_API -->|"results"| EXA
    EXA -->|"formatted results"| RESULT
    FETCH -->|"HTTP GET"| WEB_URL
    WEB_URL -->|"HTML"| FETCH
    FETCH -->|"markdown text"| RESULT
    VISION -->|"download image"| WEBDAV_IMG
    WEBDAV_IMG -->|"image bytes"| VISION
    VISION -->|"image description"| RESULT
    INFOGRAPH -->|"infograph prompt"| IMG_API
    IMG_API -->|"image bytes"| INFOGRAPH
    INFOGRAPH -->|"PUT image.png"| WEBDAV_STORE
    WEBDAV_STORE -->|"image URL"| INFOGRAPH
    INFOGRAPH -->|"image URL"| RESULT
    ANIME -->|"anime prompt"| IMG_API
    IMG_API -->|"image bytes"| ANIME
    ANIME -->|"PUT image.png"| WEBDAV_STORE
    WEBDAV_STORE -->|"image URL"| ANIME
    ANIME -->|"image URL"| RESULT
```

### 2e. Image Generation Pipeline

Both `infograph` and `anime` share the same pipeline; only the system prompt
and style prefix differ.

```mermaid
flowchart TD
    PROMPT[prompt]
    STYLE{Style Prefix}
    INFO["infographic: " + prompt]
    ANI["japanese anime style: " + prompt]
    API[Image Generation API]
    BYTES[image bytes]
    NAME(GenerateFilename)
    PUT(PutToWebDAV)
    DAV["/{root}/{room_id}/images/{name}.png"]
    URL[WebDAV public URL]
    RESULT[ToolResult]

    PROMPT --> STYLE
    STYLE -->|"infograph"| INFO
    STYLE -->|"anime"| ANI
    INFO -->|"styled prompt"| API
    ANI -->|"styled prompt"| API
    API -->|"PNG bytes"| BYTES
    BYTES --> NAME
    NAME -->|"{tool}_{timestamp}.png"| PUT
    DAV -->|"destination"| PUT
    PUT -->|"201 Created"| URL
    URL -->|"markdown image link"| RESULT
```

### 2f. Per-Room State Routing

Each room maintains independent state — conversation history, agent context, and
WebDAV archive path. The agent routes incoming messages to the correct room's
pipeline.

```mermaid
flowchart TD
    RC[IncomingMessage]
    ROOM_MAP[(RoomStateMap)]
    GET_EXIST{GetOrCreate}
    NEW_ROOM(NewRoomState)
    EXIST_ROOM[RoomState]
    MEM[(InMemoryHistory)]
    INACT(InteractWithAi)
    AREPLY[BotReply]
    DAV["{room_id}/memory/"]
    DAV_IMG["{room_id}/images/"]

    RC -->|"room_id"| GET_EXIST
    ROOM_MAP -->|"lookup"| GET_EXIST
    GET_EXIST -->|"not found"| NEW_ROOM
    GET_EXIST -->|"found"| EXIST_ROOM
    NEW_ROOM -->|"load archives"| DAV
    DAV -->|"archive files"| NEW_ROOM
    NEW_ROOM -->|"seed"| MEM
    NEW_ROOM -->|"store"| ROOM_MAP
    EXIST_ROOM -->|"history"| MEM
    MEM -->|"history"| INACT
    INACT -->|"reply"| AREPLY
    INACT -->|"generated image"| DAV_IMG
```

### 2g. Startup Sequence

```mermaid
flowchart TD
    START["main()"]
    CFG(LoadConfig)
    MIGRATE{Legacy JSON?}
    TOML["config.toml"]
    JSON["config.json"]
    VALIDATE(ValidateConfig)
    LOGIN(LoginRocketChat)
    CONNECT(ConnectWebSocket)
    DAV[(WebDAV)]
    LIST_MEM(ListMemoryArchives)
    SEED(SeedAllRooms)
    LOOP[Agent Loop]
    CFG_STORE[(AppConfig)]

    START -->|"config path"| CFG
    CFG -->|"load TOML"| TOML
    CFG -->|"check legacy"| MIGRATE
    MIGRATE -->|"found"| JSON
    JSON -->|"migrate"| TOML
    TOML -->|"raw config"| VALIDATE
    VALIDATE -->|"AppConfig"| CFG_STORE
    CFG_STORE -->|"server section"| LOGIN
    LOGIN -->|"auth token"| CONNECT
    CONNECT -->|"connected"| DAV
    CFG_STORE -->|"WebDAV credentials"| DAV
    DAV -->|"PROPFIND"| LIST_MEM
    LIST_MEM -->|"archive list"| SEED
    SEED -->|"ready"| LOOP
```

### 2h. Tool Definitions

| Tool Name     | Description                                      | Arguments                          |
| ------------- | ------------------------------------------------ | ---------------------------------- |
| `web_search`  | Search the web using Exa                         | `query: string`                    |
| `web_fetch`   | Fetch a URL, optionally as markdown              | `url: string, markdown: bool`      |
| `vision`      | Describe or analyze an image                     | `url: string, prompt: string`      |
| `infograph`   | _(planned)_ Generate an infographic image        | `prompt: string`                   |
| `anime`       | _(planned)_ Generate a Japanese anime-style image | `prompt: string`                  |

## 3. Data Structures

#### `HarnessState`

| Field       | Type                       | Notes                                       |
| ----------- | -------------------------- | ------------------------------------------- |
| `config`    | `Arc<AppConfig>`           | Immutable configuration shared across subsystems |
| `rooms`     | `HashMap<String, RoomState>` | Per-room state map (room_id → state)     |
| `client`    | `rocketchat::Client`       | RocketChat connection handle                |
| `memory`    | `MemoryManager`            | Per-room conversation history               |
| `webdav`    | `WebDavClient`             | WebDAV handle for persistent storage        |

#### `RoomState`

| Field      | Type                | Notes                                      |
| ---------- | ------------------- | ------------------------------------------ |
| `room_id`  | `String`            | RocketChat room/channel identifier         |
| `is_dm`    | `bool`              | True if direct message room                |
| `history`  | `ConversationHistory`| In-memory message buffer for this room     |
| `webdav_root` | `String`         | `/{root}/{room_id}/` path prefix           |

#### `LifecycleSignal`

| Variant    | Fields             | Notes                                      |
| ---------- | ------------------ | ------------------------------------------ |
| `Startup`  | —                  | Bot is initializing                        |
| `Running`  | —                  | Main event loop active                     |
| `Shutdown` | `exit_code: i32`   | Graceful shutdown triggered                |
| `Reconnect`| `attempt: u32`     | WebSocket reconnection in progress         |

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
