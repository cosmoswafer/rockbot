# Error Handling & Fallbacks

## 1. Purpose

Descends from [the agent loop](../agent-loop.md): how API errors, tool execution
errors, and iteration-limit breaches lead to context reset/repair retries or a
fallback error reply. It also covers **HTTP 200 but unusable completions** — a
`finish_reason = "length"` (truncated/empty, e.g. reasoning tokens consuming the
whole output budget) or `content_filter` / `insufficient_system_resource`
completion is retried once with a larger budget, then answered with a distinct
failure reply and **not** persisted as an assistant turn (issue #117).

**References:**
- [Agent Loop (Main Success Path)](../agent-loop.md) — parent Level 1 flow
- [Context-Length-Exceeded Retry](./context-length-retry.md) — `ContextLengthExceeded` reset/retry path
- [Tool-Call JSON Parse Error Recovery](./tool-call-repair.md) — tool-call args parse error repair path

## 2. Diagram

```mermaid
flowchart TD
    AI[AiProvider]
    ASSESS(AssessFinish)
    RETRY_BUDGET(RetryWithBiggerBudget)
    EMPTY_FALLBACK(SendCompletionFailureReply)
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

    AI -->|"completion result"| ASSESS
    ASSESS -.->|"length/empty<br/>(first attempt)"| RETRY_BUDGET
    RETRY_BUDGET -.->|"max_tokens_retry +<br/>thinking disabled"| RETRY
    ASSESS -.->|"length after retry,<br/>content_filter,<br/>insufficient resource"| EMPTY_FALLBACK
    EMPTY_FALLBACK -->|"failure reply<br/>(assistant turn not persisted)"| REPLY
    ASSESS -.->|"usable text / tool calls"| TOOL_EXEC
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

### Finish-reason assessment

| `finish` | Text | Action |
| -------- | ---- | ------ |
| `length` | any (first attempt) | Retry once with `max_tokens_retry` (clamped to 3/4 of `model_context_length`) and `thinking: disabled` when the provider supports it; nothing is persisted from the discarded attempt. |
| `length` | non-empty (after retry) | Accept the reply with a `warn!` "truncated reply accepted" log. |
| `length` | empty (after retry) | Budget-exhaustion reply; no assistant turn is persisted. |
| `content_filter` / `insufficient_system_resource` | any | Distinct failure reply; no assistant turn is persisted. |
| `stop` / `error` | empty | One retry, then the generic "incomplete response" reply. |
| `tool_calls` + `length` | — | `warn!` (arguments may be clamped); tool-call repair still applies. |
