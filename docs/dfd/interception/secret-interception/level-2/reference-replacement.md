# Secret Reference Replacement (Per-String, UUID-Based)

## 1. Purpose

Shows the single-pass, string-level replacement of `secret:<uuid>` tokens
against the host-scoped map; unknown UUIDs and key labels pass through
unresolved. Called from the main flow's [ResolveSecretRefsDeep](../uuidv5-scoped-injection.md) —
see [Deep Argument Traversal — All Injection Points](../deep-traversal.md) for the full walk of
argument fields.

## 2. Diagram

```mermaid
flowchart LR
    INPUT["Any string value in args<br/>'token secret:550e8400-e29b-...'"]
    SCAN[Scan for secret: prefix]
    EXTRACT["Extract UUID<br/>chars: a-f0-9-"]
    LOOKUP{UUID in HostSecretMap?}
    REPLACE["Replace secret:uuid<br/>with actual value"]
    KEEP["Keep secret:uuid<br/>log warning"]
    OUTPUT["Resolved string<br/>e.g. 'token abc123'"]

    INPUT --> SCAN
    SCAN -->|"found"| EXTRACT
    SCAN -->|"not found"| OUTPUT
    EXTRACT --> LOOKUP
    LOOKUP -->|"yes"| REPLACE
    LOOKUP -->|"no"| KEEP
    REPLACE --> OUTPUT
    KEEP --> OUTPUT
```
