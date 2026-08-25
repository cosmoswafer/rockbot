# Happy Flow (Main Success Path)

## 1. Purpose

The connection lifecycle has two phases.  **Setup** (authenticate, subscribe) is
a single sequential path.  Once the subscription is confirmed, the runtime
splits into **two concurrent data paths** — the event stream (incoming messages)
and the ping keepalive — both sharing the same WebSocket.

## 2. Diagram

```mermaid
flowchart TD
    CFG[ServerConfig]
    AUTH(Authenticate DDP)
    CONNECT(Connect WebSocket)
    SUB(Subscribe Stream)
    READY(Confirm Subscription)
    STREAM(Stream Events)
    PING(Ping Keepalive)
    PARSE(Parse Event)
    FILTER(Filter Mention Or DM)
    DISPATCH(Dispatch Message)
    SEND(Send Reply)
    HARNESS[Agent Loop]
    RC_DDP[RocketChat DDP over WebSocket]

    CFG -->|"credentials"| AUTH
    CONNECT -->|"DDP connect"| RC_DDP
    RC_DDP -->|"connected"| AUTH
    AUTH -->|"login + sha256"| RC_DDP
    RC_DDP -->|"auth result"| AUTH
    AUTH -->|"subscription request"| SUB
    SUB -->|"sub message"| RC_DDP
    RC_DDP -->|"subscription ready"| READY

    READY -->|"subscription ready"| PING
    READY -->|"subscription ready"| STREAM

    PING -->|"ping frame"| RC_DDP
    RC_DDP -->|"incoming frames"| STREAM
    STREAM -->|"raw event"| PARSE
    AUTH -->|"bot user id"| FILTER
    PARSE -->|"parsed event"| FILTER
    FILTER -->|"IncomingMessage"| DISPATCH
    DISPATCH -->|"filtered message"| HARNESS
    HARNESS -->|"reply payload"| SEND
    SEND -->|"outgoing frame"| RC_DDP
```

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
