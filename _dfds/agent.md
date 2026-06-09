# Agent Orchestration

## 1. Purpose

Core agentic loop that receives an `IncomingMessage`, assembles context (system
prompt + conversation history + tool definitions), queries the AI provider,
executes any tool calls, feeds results back, and loops until a final text reply
is produced. Each room/DM has its own conversation context.

- Upstream: [RocketChat Connection](rocketchat.md) provides `IncomingMessage`
- Upstream: [Memory Management](memory.md) provides conversation history
- Downstream: [AI Provider](ai-provider.md) receives `ChatRequest` and returns
  `CompletionResult`
- Downstream: [Tools](#2c-tool-execution-deep-dive) execute and return results

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    MSG[IncomingMessage]
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
    RC_SEND[RocketChat SendReply]

    MSG -->|"message + room_id"| CTX
    MEM -->|"history for room"| CTX
    TOOLS_DEF -->|"tool definitions"| CTX
    CTX -->|"system prompt + history + tools"| REQ
    REQ -->|"ChatRequest"| AI
    AI -->|"CompletionResult"| CHECK
    CHECK -->|"no tool calls"| REPLY
    REPLY -->|"BotReply"| RC_SEND
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
    RC_SEND[RocketChat SendReply]
    REQ(ChatRequest)

    EXEC -->|"tool execution failed"| ERR_TOOL
    ERR_TOOL -->|"error as tool result"| REQ
    AI -->|"API error"| ERR_AI
    ERR_AI -->|"fallback message"| RC_SEND
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
    RESULT[ToolResult]
    EXA_API[Exa API]
    WEB_URL[Remote URL]
    WEBDAV_IMG[WebDAV Image]

    CALL -->|"name"| REG
    REG -->|"web_search"| EXA
    REG -->|"web_fetch"| FETCH
    REG -->|"vision"| VISION
    EXA -->|"search query"| EXA_API
    EXA_API -->|"results"| EXA
    EXA -->|"formatted results"| RESULT
    FETCH -->|"HTTP GET"| WEB_URL
    WEB_URL -->|"HTML"| FETCH
    FETCH -->|"markdown text"| RESULT
    VISION -->|"download image"| WEBDAV_IMG
    WEBDAV_IMG -->|"image bytes"| VISION
    VISION -->|"image description"| RESULT
```

### 2d. Tool Definitions

| Tool Name     | Description                           | Arguments                    |
| ------------- | ------------------------------------- | ---------------------------- |
| `web_search`  | Search the web using Exa              | `query: string`              |
| `web_fetch`   | Fetch a URL, optionally as markdown   | `url: string, markdown: bool`|
| `vision`      | Describe or analyze an image          | `url: string, prompt: string`|

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
