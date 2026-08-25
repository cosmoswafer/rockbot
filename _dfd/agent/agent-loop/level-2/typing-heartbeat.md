# Typing Indicator Heartbeat

## 1. Purpose

Level 2 decomposition of `ToggleTyping` and the typing flows during `AgentLoop`. The bot sends an initial `typing=true` signal before the agent loop begins, then a background task refreshes it every 2 seconds while the loop runs. When the loop produces a reply (or errors out), typing is set to `false`.

**References:** Parent flow [main path](../main-path.md); failure behavior in
[error handling](error-handling.md)

## 2. Diagram

```mermaid
flowchart TD
    PLATFORM[Messaging Platform]
    MSG[IncomingMessage]
    START_TYPING(SendTypingOn)
    HEARTBEAT(RefreshTyping)
    TIMER((Every 2s))
    PROCESS(AgentLoop)
    STOP_TYPING(SendTypingOff)
    REPLY[BotReply]

    MSG -->|"dispatch"| START_TYPING
    START_TYPING -->|"typing indicator on"| PLATFORM
    START_TYPING -->|"start loop"| PROCESS
    PROCESS -->|"while processing"| HEARTBEAT
    TIMER -->|"tick"| HEARTBEAT
    HEARTBEAT -->|"typing indicator on"| PLATFORM
    PROCESS -->|"reply ready or error"| STOP_TYPING
    STOP_TYPING -->|"typing indicator off"| PLATFORM
    STOP_TYPING -->|"send"| REPLY
```

The heartbeat task is a `tokio::spawn` that runs concurrently with the agent loop, refreshing the typing indicator every 2 seconds. If the WebSocket disconnects, `sender.typing()` returns an error — the heartbeat task breaks its loop and exits silently. The main agent loop is unaffected.

Typing indicator state is intentionally not retried or persisted — it is a transient UI affordance with no durability requirements.
