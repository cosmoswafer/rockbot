# Token-Based Trigger (Post-LLM Call → Checked After Reply)

## 1. Purpose

The token trigger uses the provider's actual token count: during each LLM
call the harness inspects `response.usage.total_tokens`, and if it exceeds
85% of the configured `model_context_length`, a `token_pressure_flag` is set
for that room and checked **after the reply is delivered**, triggering LLM
summarization — the user never waits.

References: [Post-Reply Decision](../post-reply-decision.md).

## 2. Diagram

```mermaid
flowchart TD
    LLM_CALL[LLM Provider Call]
    RESP["Response<br/>(CompletionResult)"]
    USAGE["usage.total_tokens"]
    CHECK{"total_tokens<br/>> 85% of<br/>model_context_length?"}
    SET_FLAG["Set token_pressure_flag<br/>for this room"]
    CONTINUE["Continue Normal Flow<br/>(reply to user first)"]

    LLM_CALL -->|"chat/completions"| RESP
    RESP -->|"extract usage"| USAGE
    USAGE --> CHECK
    CHECK -->|"yes: near context limit"| SET_FLAG
    CHECK -->|"no"| CONTINUE
    SET_FLAG --> CONTINUE

    CONTINUE -->|"reply delivered"| POST[After Reply:<br/>check flags → summarize]
```

**Provider support**: all major providers return `usage` in responses. If
`usage` is absent or `total_tokens` is 0, the flag is not set (graceful
degradation).
