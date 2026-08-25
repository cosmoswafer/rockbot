# Agent Loop Deep Dive

## 1. Purpose

Level 2 decomposition of the invariant agent loop (`while True: LLM → tools →
LLM`): queries the AI provider, executes any tool calls, feeds results back, and
loops until a final text reply is produced.

**References:**
- [Agent Loop (Main Success Path)](../agent-loop.md) — parent Level 1 flow
- [Tool Execution Deep Dive](../tool-dispatch.md) — `ExecuteTool` dispatch and injection rules

## 2. Diagram

```mermaid
flowchart TD
    CTX[BuildContext]
    AI[AiProvider]
    ASSESS(AssessCompletion)
    EXEC(ExecuteTool)
    APPEND(AppendToolResult)
    LIMIT(CheckIterationLimit)
    RESET(ResetHistory<br/>clear Layer 1)
    CTX_ERR{ContextLength<br/>Exceeded?}
    REBUILD(HardTruncate<br/>keep system prefix + last 2 msgs)
    REPLY_OUT[BotReply]

    CTX -->|"chat request"| AI
    AI -->|"completion result"| ASSESS
    ASSESS -->|"tool calls"| EXEC
    ASSESS -->|"final reply text"| REPLY_OUT
    EXEC -->|"tool result"| APPEND
    APPEND -->|"updated messages"| CTX
    CTX -->|"context byte size"| LIMIT
    EXEC -.->|"tool execution error"| APPEND
    AI -.->|"api error"| CTX_ERR
    CTX_ERR -.->|"yes (first time)"| RESET
    CTX_ERR -.->|"no (other error)"| REPLY_OUT
    RESET -.->|"empty history"| REBUILD
    REBUILD -.->|"retry request"| CTX
```
