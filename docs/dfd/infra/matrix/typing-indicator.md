# Typing Indicator

## 1. Purpose

Matrix typing indicators are sent as ephemeral events to the room.

## 2. Diagram

```mermaid
flowchart TD
    HARNESS[Agent Loop]
    TYPING(SendTypingState)
    MATRIX[Matrix Homeserver]

    HARNESS -->|"room_id + typing=true"| TYPING
    TYPING -->|"PUT /_matrix/client/v3/rooms/{roomId}/typing/{userId}<br/>{typing: true, timeout: 5000}"| MATRIX
    HARNESS -->|"room_id + typing=false"| TYPING
    TYPING -->|"PUT /_matrix/client/v3/rooms/{roomId}/typing/{userId}<br/>{typing: false}"| MATRIX
```

The typing timeout is set to 5000ms per the Matrix spec recommendation. The
heartbeat task in the agent loop refreshes it every 2 seconds, matching the
RocketChat behavior.
