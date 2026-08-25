# Error Handling & Fallbacks

## 1. Purpose

Descends from [the agent loop](../agent-loop.md): how API errors, tool execution
errors, and iteration-limit breaches lead to context reset/repair retries or a
fallback error reply.

**References:**
- [Agent Loop (Main Success Path)](../agent-loop.md) — parent Level 1 flow
- [Context-Length-Exceeded Retry](./context-length-retry.md) — `ContextLengthExceeded` reset/retry path
- [Tool-Call JSON Parse Error Recovery](./tool-call-repair.md) — tool-call args parse error repair path

## 2. Diagram

```mermaid
flowchart TD
    AI[AiProvider]
    TOOL_EXEC(ExecuteTool)
    LOOP_LIMIT(CheckMaxIterations)
    APPEND(AppendToolResult)
    FALLBACK(SendFallbackReply)
    CHECK{ContextLength<br/>Exceeded?}
    RESET(ResetHistory<br/>clear Layer 1)
    REBUILD(HardTruncate<br/>keep system prefix + last 2 msgs)
    RETRY(Retry LLM Call)
    PARSE_ERR{Tool call args<br/>JSON parse error?}
    REPAIR(Repair tool call args<br/>in history)
    REPLY[BotReply]

    AI -.->|"api error response"| CHECK
    CHECK -.->|"yes (first time)"| RESET
    CHECK -.->|"no"| PARSE_ERR
    PARSE_ERR -.->|"yes (first time)"| REPAIR
    PARSE_ERR -.->|"no"| FALLBACK
    RESET -.->|"empty history"| REBUILD
    REBUILD -.->|"retry"| RETRY
    REPAIR -.->|"rebuilt messages"| RETRY
    RETRY -.-> AI
    TOOL_EXEC -.->|"tool execution error"| APPEND
    LOOP_LIMIT -.->|"max iterations exceeded"| FALLBACK
    FALLBACK -->|"error reply text"| REPLY
```
