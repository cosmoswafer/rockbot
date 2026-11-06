# Shortcut Fast Path — Pre-LLM Detection

## 1. Purpose

When the user sends a literal `!reset` or `!clearmemory` command, the harness
detects it before any LLM call, clears Layer 1, persists the empty snapshot, and
only then returns a canned reply. No token cost, no restart window.

- References: [Pre-LLM Shortcut](../../memory/memory-reset/pre-llm-shortcut.md)

## 2. Diagram

```mermaid
flowchart TD
    USER["User: !reset<br/>or !clearmemory"]
    CHECK{"clean_text ==<br/>!reset or !clearmemory?"}
    RESET["reset_room_if_needed()<br/>(clear Layer 1 now)"]
    PERSIST["Write empty snapshot<br/>(archive_seq >= 1)"]
    REPLY["Return canned reply<br/>(Memory cleared.)"]

    USER -->|"exact command"| CHECK
    CHECK -->|"yes"| RESET
    RESET --> PERSIST
    PERSIST --> REPLY
    REPLY -->|"bot reply (instant)"| USER
```

No LLM call, no tool dispatch. The wipe and the WebDAV write happen **before**
the canned reply, so a crash or restart cannot resurrect the cleared history.
The `reset_memory` tool registration is still needed for natural-language reset
requests handled by the LLM (§2a).

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
