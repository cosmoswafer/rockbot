# Shortcut Fast Path — Pre-LLM Detection

## 1. Purpose

When the user sends a literal `!reset` or `!clearmemory` command, the harness
detects it before any LLM call and returns a canned reply. No token cost.

- References: [Pre-LLM Shortcut](../../memory/memory-reset/pre-llm-shortcut.md)

## 2. Diagram

```mermaid
flowchart TD
    USER["User: !reset<br/>or !clearmemory"]
    CHECK{"clean_text ==<br/>!reset or !clearmemory?"}
    SET_FLAG["Set explicit_reset<br/>flag on room"]
    REPLY["Return canned reply<br/>(Memory cleared.)"]
    POST["Post-reply:<br/>reset_room_if_needed()"]
    CLEAR["Clear ALL Messages<br/>(Layer 1 → 0)"]
    DIRTY[Mark Snapshot Dirty]

    USER -->|"exact command"| CHECK
    CHECK -->|"yes"| SET_FLAG
    SET_FLAG --> REPLY
    REPLY -->|"bot reply (instant)"| USER
    REPLY -->|"after reply sent"| POST
    POST --> CLEAR
    CLEAR --> DIRTY
```

No LLM call, no tool dispatch. The `reset_memory` tool registration is still
needed for natural-language reset requests handled by the LLM (§2a).

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
