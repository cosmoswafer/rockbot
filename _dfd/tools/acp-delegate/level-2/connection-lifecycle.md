# ACP Delegate — Connection Lifecycle

## 1. Purpose

Deep dive into how a single long-lived connection to a spawned ACP agent is
initialized and reused. References: [ACP Delegate — Happy Flow](../main-path.md).

## 2. Diagram

One long-lived connection task per spawned agent. `AcpClient` talks to it over
a command channel; a single session is created lazily and reused. Prompts are
serialized by a mutex (ACP sessions process one prompt turn at a time).

```mermaid
flowchart TD
    TOOL(AcpTool)
    CLIENT(AcpClient)
    SPAWN(SpawnAgentProcess)
    INIT(InitializeConnection)
    CONN[[Connection Task]]
    AGENT[ACP Agent subprocess]

    TOOL -->|"prompt (mutex-held)"| CLIENT
    CLIENT -->|"not connected"| SPAWN
    SPAWN -->|"tokio::process (env allowlist, cwd, kill_on_drop)"| AGENT
    SPAWN -->|"ByteStreams transport"| CONN
    INIT -->|"initialize (client_info=rockbot, fs/terminal off)"| CONN
    CONN <-->|"initialize request/response"| AGENT
    CLIENT -->|"AcpCommand::Prompt"| CONN
    CONN -->|"session/new (lazy, once)"| AGENT
    CONN -->|"session/prompt"| AGENT
    AGENT -->|"session/update stream"| CONN
    CONN -->|"stop_reason + aggregated text"| CLIENT
```
