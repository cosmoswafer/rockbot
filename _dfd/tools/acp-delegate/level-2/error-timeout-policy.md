# ACP Delegate — Error Handling, Timeout & Permission Policy

## 1. Purpose

Deep dive into prompt timeouts, subprocess death, spawn failures, and the
permission policy. References: [ACP Delegate — Happy Flow](../main-path.md).

## 2. Diagram

```mermaid
flowchart TD
    CONN[[Connection Task]]
    AGENT[ACP Agent subprocess]
    CANCEL(SendCancel)
    RESPAWN(RespawnOnce)
    PERM(PermissionPolicy)
    ERR[Tool Error Result]

    CONN -.->|"prompt timeout exceeded"| CANCEL
    CANCEL -->|"session/cancel notification"| AGENT
    CANCEL -->|"timeout error"| ERR
    CONN -.->|"transport closed (subprocess died)"| RESPAWN
    RESPAWN -.->|"retry prompt once on fresh connection"| CONN
    RESPAWN -.->|"second failure"| ERR
    CONN -.->|"spawn / initialize failure"| ERR
    AGENT -->|"session/request_permission"| PERM
    PERM -->|"auto_approve = true: first allow-once/allow-always option (fallback: first option)"| AGENT
    PERM -->|"auto_approve = false (default): Cancelled"| AGENT
```

Note: when the agent advertises `authMethods` in `initialize`, RockBot logs a
warning and continues unauthenticated — no credential flow exists yet. Agents
that require auth surface a protocol error, which becomes a tool error result.

Note: on timeout the prompt response is discarded even if the agent later
answers; the turn is considered `cancelled`.
