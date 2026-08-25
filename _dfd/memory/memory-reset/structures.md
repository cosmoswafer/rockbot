# Memory Reset — Shared Structures

## 1. Overview

Manages context window pressure through **LLM-based summarization** for
automatic triggers (token/byte pressure) and **hard reset** for explicit user
requests. When pressure is detected, the oldest ~60% of messages are
summarized into a compact synthetic system message by an LLM call, preserving
key context while freeing space. Explicit resets (`!reset`) still clear all
messages instantly. The former full-wipe-on-pressure and the former
`summary.md` WebDAV pipeline have both been replaced.

- Upstream: [Memory Management](../memory/retrieve-two-layers.md) — provides `ConversationHistory`
  (Layer 1) and the pressure flags
- Upstream: [AI Provider](../../ai/ai-provider/main-path.md) — returns token usage counts for the
  post-call token trigger; may return `ContextLengthExceeded` errors; used for
  summarization LLM call
- Upstream: [Configuration Management](../../infra/config/main-path.md) — provides trigger
  thresholds (`max_context_bytes`, `model_context_length`,
  `summarization_enabled`, `summarization_ratio`,
  `summarization_target_tokens`)
- Downstream: none — summarization and reset are purely in-memory (no WebDAV
  writes)

## 3. Data Structures

### `ResetResult`

Return type of `reset_room_if_needed()`. Carries the reset outcome to tests
and to the post-reply dispatch in `main.rs`.

| Field | Type | Description |
|-------|------|-------------|
| `did_reset` | `bool` | Whether any messages were actually cleared |
| `was_explicit` | `bool` | Whether reset was triggered by `explicit_reset` flag |
| `messages_cleared` | `usize` | Number of messages removed from Layer 1 |

## 4. Configuration

Fields from `ModelConfig` in [Configuration Management](../../infra/config/main-path.md):

| Field                  | Type    | Default | Notes |
| ---------------------- | ------- | ------- | ----- |
| `max_context_bytes`    | `usize` | 4_000_000 | byte-size overflow trigger (pre-LLM inline trim, flag for post-reply summarization) |
| `model_context_length` | `u32`   | 1_000_000 | Model's max context tokens. 85% threshold (`* 0.85`) triggers post-LLM summarization. Default 1M. |
| `summarization_enabled` | `bool` | `true` | If `true`, token/byte pressure triggers LLM summarization. If `false`, falls back to strip-half. |
| `summarization_ratio`  | `f64`   | 0.6 | Portion of oldest messages to summarize (0.6 = 60%). Remaining 40% are retained. |
| `summarization_target_tokens` | `usize` | 1024 | Target max tokens for the summarization LLM prompt instruction. |

## 5. Trigger Summary

All triggers are evaluated **after reply delivery**. The safety net (inline
truncation) runs pre-LLM but is not a reset trigger.

| Trigger | Evaluation Point | Condition | Action |
|---------|-----------------|-----------|--------|
| **Token near-limit** | Flag set during LLM call, checked after reply | `usage.total_tokens > model_context_length * 0.85` | Summarize & compress (oldest ~60% → LLM summary) |
| **Byte pressure** | Flag set during context assembly, checked after reply | `context_bytes > max_context_bytes` | Summarize & compress (oldest ~60% → LLM summary) |
| **User command shortcut** | Before LLM call (early return) | `clean_text == "!reset"` or `"!clearmemory"` | Hard reset (clear all L1 messages), no LLM call |
| **User request (NL)** | Flag set by `reset_memory` tool, checked after reply | Tool called by LLM (intent detection) | Hard reset (clear all L1 messages) |
| **Safety net** | Before each LLM call | `context_bytes > max_context_bytes` | Inline trim only (strip images, truncate); sets byte_pressure_flag |
| **Provider error** | During LLM call | `ContextLengthExceeded` | Hard reset + hard truncate + retry (once) |

## 6. Integration

### With Agent Harness

| Method | Return Type | When | Action |
|--------|-------------|------|--------|
| `reset_room_if_needed()` | `Result<ResetResult>` | After reply delivery (background) | Checks flags; routes explicit → hard reset, pressure → summarize |
| `summarize_and_compress()` | `Result<ResetResult>` | Called by `reset_room_if_needed()` for pressure flags | LLM summarization of oldest ~60%; fallback to strip-half |
| `call_summarization_llm()` | `Result<String>` | Called by `summarize_and_compress()` | Sends summarization prompt to provider; returns summary text |
| `check_token_pressure()` | `void` | During LLM response processing | Sets `token_pressure_flag` — does NOT block reply |
| `trim_context()` | `Vec<ChatMessage>` | Before each LLM call (safety net) | Fast in-memory trim; sets `byte_pressure_flag` |

### With Memory Manager

| Method | Purpose |
|--------|---------|
| `needs_reset(room_id)` | Returns true if any pressure or explicit flag is set |
| `message_count(room_id)` | Returns number of Layer 1 messages (used for summarization threshold) |
| `oldest_messages(room_id, count)` | Returns the oldest N messages for summarization |
| `summarize_room(room_id, count, summary_msg)` | Prunes oldest N messages, inserts summary system message at position 0 |
| `strip_half(room_id)` | Drops oldest 50% of messages (fallback when summarization fails or is disabled) |
| `clear_all_messages(room_id)` | Removes all Layer 1 messages (hard reset only) |
| `clear_pressure_flags(room_id)` | Clears all three flags |
