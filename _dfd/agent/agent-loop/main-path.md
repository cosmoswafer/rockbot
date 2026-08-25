# Main Success Path

## 1. Purpose

The main success path of the agent loop: the messaging platform delivers an
incoming message with a per-message `PlatformSender` handle, the loop toggles
the typing indicator on, strips the bot's mention prefix, resolves the room's
display name, and hands the message to the agent harness (conversation history,
tool definitions, AI config) which sends the chat request and returns the
bot reply. Every reply marks the room state dirty, so the periodic persister
flushes `snapshot.json` for dirty rooms and evicts stale rooms. Shared data
structures and full subsystem wiring are documented in
[structures.md](structures.md).

## 2. Diagram

```mermaid
flowchart TD
    PLATFORM[Messaging Platform<br/>RocketChat or Matrix]
    AI[AI Provider API]
    DAV[(NextCloud WebDAV)]
    EXA[Search Web API]
    TIMER[Evict Timer]
    DISPATCH(ReceiveMessage)
    TYPING(ToggleTyping)
    STRIP(StripMentionPrefix)
    RESOLVE(ResolveDisplayName)
    LOOP(AgentLoop)
    DIRTY(MarkSnapshotDirty)
    SNAPSHOT(FlushSnapshots)
    RESET(ResetMemory)

    CFG[(AppConfig)]
    HISTORY[(ConversationHistory)]
    TOOLS[(ToolRegistry)]
    ROOMS[(RoomStateMap)]

    PLATFORM -->|"incoming message + PlatformSender"| DISPATCH
    ROOMS -->|"room state"| DISPATCH
    CFG -->|"app config"| DISPATCH
    DISPATCH -->|"incoming message + sender"| TYPING
    TYPING -->|"typing on"| PLATFORM
    TYPING -->|"incoming message + sender"| STRIP
    STRIP -->|"non-DM: clean text<br/>DM: raw text"| RESOLVE
    RESOLVE -->|"room_name + display_name"| LOOP
    CFG -->|"ai config"| LOOP
    HISTORY -->|"conversation history"| LOOP
    TOOLS -->|"tool definitions"| LOOP
    LOOP -->|"chat request"| AI
    AI -->|"completion result"| LOOP
    LOOP -->|"typing off"| PLATFORM
    LOOP -->|"bot reply"| PLATFORM
    LOOP -->|"reply produced<br/>(every response)"| DIRTY
    DIRTY -->|"dirty flag"| ROOMS
    PLATFORM -->|"reply delivered"| RESET
    RESET -->|"clear Layer 1<br/>(no LLM call)"| HISTORY
    RESET -->|"also marks dirty"| DIRTY
    LOOP -->|"updated room state"| ROOMS
    TIMER -->|"every persist_interval_secs"| SNAPSHOT
    ROOMS -->|"dirty rooms"| SNAPSHOT
    SNAPSHOT -->|"snapshot.json<br/>(L1 history, bot-internal<br/>under {snapshot_prefix}/{bot_id}/{wd})"| DAV
    TIMER -->|"every persist_interval_secs"| EVICT_ROOMS
    ROOMS -->|"all rooms"| EVICT_ROOMS
    EVICT_ROOMS -->|"snapshot.json for stale rooms"| DAV
    EVICT_ROOMS -->|"remove stale entries"| ROOMS
```
