# Agent Orchestration

## 1. Purpose

Core agentic loop that receives an `IncomingMessage` and per-room context from
the harness, assembles context (system prompt + conversation history + tool
definitions), queries the AI provider, executes any tool calls, feeds results
back, and loops until a final text reply is produced. Each room/DM has its own
conversation context.

- Upstream: [Agent Harness](agent-harness.md) provides `IncomingMessage` and
  per-room `AgentContext`
- Upstream: [Memory Management](memory.md) provides conversation history
- Downstream: [AI Provider](ai-provider.md) receives `ChatRequest` and returns
  `CompletionResult`
- Downstream: [Tools](#2c-tool-execution-deep-dive) execute and return results

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    HARNESS[Agent Harness]
    CTX(BuildContext)
    MEM[MemoryManager]
    HIST[ConversationHistory]
    TOOLS_DEF[ToolRegistry]
    REQ(ChatRequest)
    AI(AiProvider)
    CHECK{HasToolCalls?}
    EXEC(ExecuteTool)
    TOOL_RESULT[ToolResult]
    APPEND(AppendToolResult)
    REPLY(BotReply)

    HARNESS -->|"message + room_id"| CTX
    MEM -->|"history for room"| CTX
    TOOLS_DEF -->|"tool definitions"| CTX
    CTX -->|"system prompt + history + tools"| REQ
    REQ -->|"ChatRequest"| AI
    AI -->|"CompletionResult"| CHECK
    CHECK -->|"no tool calls"| REPLY
    REPLY -->|"BotReply"| HARNESS
    CHECK -->|"tool_calls[]"| EXEC
    EXEC -->|"ToolResult"| TOOL_RESULT
    TOOL_RESULT -->|"result message"| APPEND
    APPEND -->|"updated history"| REQ
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    EXEC(ExecuteTool)
    AI(AiProvider)
    ERR_TOOL(ToolErrorResult)
    ERR_AI(ErrorReply)
    MAX_LOOP{MaxIterations?}
    TRUNC(TruncateAndSummarize)
    HARNESS[Agent Harness]
    REQ(ChatRequest)

    EXEC -->|"tool execution failed"| ERR_TOOL
    ERR_TOOL -->|"error as tool result"| REQ
    AI -->|"API error"| ERR_AI
    ERR_AI -->|"fallback message"| HARNESS
    REQ -->|"loop count"| MAX_LOOP
    MAX_LOOP -->|"exceeded"| TRUNC
    TRUNC -->|"summarized context"| REQ
```

### 2c. Tool Execution Deep Dive

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

### 2d. Tool Definitions

| Tool Name     | Description                                      | Arguments                          |
| ------------- | ------------------------------------------------ | ---------------------------------- |
| `web_search`  | Search the web using Exa                         | `query: string`                    |
| `web_fetch`   | Fetch a URL, optionally as markdown              | `url: string, markdown: bool`      |
| `vision`      | Describe or analyze an image                     | `url: string, prompt: string`      |
| `infograph`   | _(planned)_ Generate an infographic image        | `prompt: string`                   |
| `anime`       | _(planned)_ Generate a Japanese anime-style image | `prompt: string`                  |

### 2e. Image Generation Pipeline

Both `infograph` and `anime` share the same pipeline; only the system prompt
and style prefix differ.

```mermaid
flowchart TD
    PROMPT[prompt]
    STYLE{Style Prefix}
    INFO["infographic: " + prompt]
    ANI["japanese anime style: " + prompt]
    API(Image Generation API)
    BYTES[image bytes]
    NAME(GenerateFilename)
    PUT(PUT to WebDAV)
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
