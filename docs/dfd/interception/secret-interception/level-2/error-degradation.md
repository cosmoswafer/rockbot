# Error Handling & Graceful Degradation

## 1. Purpose

Covers the failure branches of secret loading: WebDAV not configured, file
missing, TOML parse error, empty secrets table, no secrets for the target
host, and unknown UUIDs — interception degrades gracefully instead of failing
the message. Embedded in the main flow:
[Happy Flow — UUIDv5 Generation + Host-Scoped Injection](../uuidv5-scoped-injection.md).

## 2. Diagram

```mermaid
flowchart TD
    SUB(process_message)
    LOAD(LoadSecretsFromWebDav)
    DAV[(NextCloud WebDAV)]
    NO_DAV[Skip: No WebDAV]
    NOT_FOUND[Skip: File not found<br/>in room directory]
    PARSE_ERR[Warn: TOML parse error]
    EMPTY[Skip: Empty secrets table]
    BUILD("Build system prompt<br/>without UUID section<br/>(no secrets available)")
    LLM("LLM call<br/>no secret:uuid tokens<br/>in system prompt")
    GEN["Generate UUID<br/>per entry"]
    FILTER(FilterSecretsByHost)
    NO_HOST[Skip: No secrets for target host]
    RESOLVE["ResolveSecretRefsDeep<br/>replace secret:uuid in all string values"]
    UUID_MISS[Warn: UUID not found]
    PASS[Passthrough: original value]

    SUB --> LOAD
    LOAD -->|"GET {room_dir}/secrets.toml"| DAV
    LOAD -.->|"webdav is None"| NO_DAV
    LOAD -.->|"NotFound error"| NOT_FOUND
    LOAD -.->|"invalid TOML"| PARSE_ERR
    LOAD -.->|"secrets table empty"| EMPTY
    NO_DAV -->|"return None"| BUILD
    NOT_FOUND -->|"return None"| BUILD
    PARSE_ERR -->|"return None"| BUILD
    EMPTY -->|"return None"| BUILD
    BUILD -->|"no UUID section"| LLM
    LLM -->|"LLM uses non-UUID values<br/>or reads dummy data from webdav"| PASS
    GEN -->|"uuid assigned"| FILTER
    FILTER -.->|"host not in any entry"| NO_HOST
    NO_HOST -->|"return None"| PASS
    RESOLVE -.->|"secret:unknown-uuid"| UUID_MISS
    UUID_MISS -->|"keep original secret:uuid"| PASS
```
