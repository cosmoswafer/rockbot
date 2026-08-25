# Main Success Path

## 1. Purpose

Happy flow (main success path) of the Matrix platform: `MatrixServerConfig`
drives client creation and login, the sync loop dispatches incoming
`m.room.message` events to the message filter, which forwards `IncomingMessage`
structs to the agent loop; bot replies are sent back to the homeserver.

- References: [Shared structures](structures.md), [Agent Loop](../../agent/agent-loop/main-path.md)

## 2. Diagram

```mermaid
flowchart TD
    CFG[MatrixServerConfig]
    CLIENT(CreateMatrixClient)
    LOGIN(LoginToHomeserver)
    SYNC(StartSyncLoop)
    DISPATCH(DispatchRoomMessage)
    FILTER(FilterMessage)
    HARNESS[Agent Loop]
    REPLY(SendReply)
    MATRIX[Matrix Homeserver]

    CFG -->|"homeserver + credentials"| CLIENT
    CLIENT -->|"Client::new(homeserver_url)"| LOGIN
    LOGIN -->|"login(user_id, password)"| MATRIX
    MATRIX -->|"session token"| LOGIN
    LOGIN -->|"authenticated client"| SYNC
    SYNC -->|"sync loop started"| MATRIX
    MATRIX -->|"m.room.message event"| SYNC
    SYNC -->|"SyncEvent::Room timeline"| DISPATCH
    DISPATCH -->|"SyncRoomEvent"| FILTER
    FILTER -->|"IncomingMessage"| HARNESS
    HARNESS -->|"BotReply text"| REPLY
    REPLY -->|"RoomMessageEventContent"| MATRIX
```
