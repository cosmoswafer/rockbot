# UUIDv5 Generation on Load (Deterministic)

## 1. Purpose

Details how each `secrets.toml` entry becomes a deterministic UUIDv5 —
`uuid::Uuid::new_v5(UUID_NAMESPACE, format!("{}:{}", host, key))` — after host
normalization, so UUIDs are stable across messages, rooms, and bot restarts.
Linked from the main flow:
[Happy Flow — UUIDv5 Generation + Host-Scoped Injection](../uuidv5-scoped-injection.md).

## 2. Diagram

```mermaid
flowchart LR
    TOML["secrets.toml<br/>host + key + value"]
    PARSE[Parse TOML → Vec<SecretEntry>]
    NORM["Normalize host<br/>trim trailing '/'<br/>'https://host/' → 'https://host'"]
    UUID["uuid::Uuid::new_v5(UUID_NAMESPACE,<br/>format!('{}:{}', host, key))<br/>deterministic per entry"]
    MAP["ResolvedSecret<br/>{ uuid, key, host, value }"]
    REGISTRY["SecretRegistry<br/>Vec<ResolvedSecret><br/>stable UUIDs across<br/>messages and restarts"]

    TOML --> PARSE
    PARSE --> NORM
    NORM --> UUID
    UUID --> MAP
    MAP --> REGISTRY
```
