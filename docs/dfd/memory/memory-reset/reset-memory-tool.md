# Explicit Reset — reset_memory Tool

## 1. Purpose

When the user says `!reset` or explicitly asks to clear memory, the LLM
invokes the `reset_memory` tool. The tool sets the `explicit_reset` flag on
the room and returns an acknowledgment. After the reply is delivered,
`reset_room_if_needed()` picks up the flag and clears Layer 1 instantly.

References: [Hard Reset Deep Dive](level-2/hard-reset.md),
[Pre-LLM Shortcut](pre-llm-shortcut.md).

## 2. Diagram

```mermaid
flowchart TD
    USER["User: !reset<br/>or clear memory"]
    AI[AiProvider]
    TOOL["reset_memory Tool<br/>(set flag, return ack)"]
    FLAG["Set explicit_reset<br/>flag on room"]
    LLM_REPLY["LLM generates reply<br/>(full context intact)"]
    POST["Post-reply:<br/>reset_room_if_needed"]
    CLEAR["Clear Layer 1<br/>(zero messages)"]

    USER -->|"explicit request"| AI
    AI -->|"tool_call: reset_memory"| TOOL
    TOOL -->|"room_id"| FLAG
    FLAG -->|"ack"| LLM_REPLY
    LLM_REPLY -->|"bot reply (no delay)"| USER
    LLM_REPLY -->|"after reply"| POST
    POST --> CLEAR
```

The user receives the bot's reply immediately. Reset runs after the reply is
delivered (silent — no follow-up message). See
[reset-memory.md](../../tools/reset-memory/flag-driven.md) for the full tool flow.
