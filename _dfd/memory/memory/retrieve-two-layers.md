# Retrieve from Two Layers

## 1. Purpose

On each interaction, data from both layers is retrieved (with configurable
limits) and injected into the agent context. Write flows (persist, soul edit)
are shown in separate sub-diagrams.

## 2. Diagram

```mermaid
flowchart TD
    L2[(Layer 2<br/>Soul)]
    WEBDAV[(NextCloud WebDAV)]
    L1[(Layer 1<br/>Chat History)]
    KNOWLEDGE[(Knowledge<br/>Entries)]
    BUILD[BuildContext]

    subgraph "Load from stores"
        L2 -->|"truncated to max_soul_chars"| SOUL_OUT[Soul Content]
        WEBDAV -->|"GET soul.md"| L2
        L1 -->|"last max_history_size"| HIST_OUT[History Messages]
    end

    SOUL_OUT -->|"1. merge"| BUILD
    KNOWLEDGE -->|"1.5 merge"| BUILD
    HIST_OUT -->|"2. inject"| BUILD
    BUILD -->|"single leading system msg<br/>+ history"| CONTEXT[Agent Context]

    MSG[Incoming Message] -->|"append"| L1
```

**Single leading system message invariant**: `BuildContext` emits **exactly
one** system message at index 0. The system prompt, soul block, knowledge
index summary, and any leading `Role::System` summary message from history
(the `[Conversation Summary …]` output of LLM summarization) are merged into
that single message, joined by `\n\n`. This is required by strict chat
templates (e.g. Qwen3.5/3.6-derived, used by Bonsai-27B) that reject any
system message not at index 0 with a 400 error — see Gitea issue #77.

Layer 1 is populated by incoming messages. Layer 2 is populated by the
[Soul Editing](soul-editing.md) tool. The [Persist & Evict
Flow](persist-evict.md) provides crash recovery for Layer 1 and TTL-based room
eviction.

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
