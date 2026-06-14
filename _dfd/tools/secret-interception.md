# Secret Interception

## 1. Purpose

The harness transparently replaces `secret:<key>` references in `web_fetch`
header values with actual secret values loaded from a global `secrets.toml`
file on WebDAV. The LLM never observes real secret values — it references
them by key name only.

This enables the LLM to authenticate against external APIs (Gitea, GitHub,
etc.) without exposing API tokens in the conversation history or LLM context.

- Upstream: [Agent Harness](../agent-harness.md) runs the interception inside
  `process_message()` — secrets are loaded once per tool-call batch and
  injected before `execute_by_name()` dispatch
- Upstream: [WebDAV Tool](webdav.md) provides the `read_file_to_string`
  transport for loading `secrets.toml`
- Downstream: [Web Fetch](web-fetch.md) receives the modified arguments with
  resolved header values — the tool is unaware of the interception
- Downstream: [AI Provider](../base/ai-provider.md) never observes real secret
  values — only the `secret:<key>` references appear in the conversation
  history

### Non-Functional Requirements

- **Graceful degradation**: When WebDAV is not configured, `secrets.toml` does
  not exist, or the file fails to parse, the tool arguments pass through
  unchanged. Secret interception is never a hard dependency.
- **No caching across batches**: Secrets are loaded once per tool-call batch
  within `process_message()`, not cached across agent turns. This ensures
  updated secrets take effect on the next message without restart.
- **Single-pass replacement**: Resolved secret values are not re-scanned for
  `secret:` references — no recursive expansion.

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent Harness<br/>process_message]
    LOAD(LoadSecretsFromWebDav)
    DAV[(NextCloud WebDAV)]
    TOML[(secrets.toml)]
    MAP[(SecretMap<br/>HashMap key→value)]
    CALL[web_fetch ToolCall<br/>headers with secret:key]
    INJECT(InjectSecretsIntoHeaders<br/>replace secret:key tokens)
    EXEC(ExecuteByName)
    FETCH[WebFetchTool]

    AGENT -->|"tool_calls non-empty"| LOAD
    LOAD -->|"GET secrets.toml"| DAV
    DAV -->|"file content"| TOML
    TOML -->|"[secrets] table"| MAP
    CALL -->|"raw arguments"| INJECT
    MAP -->|"key → value lookups"| INJECT
    INJECT -->|"resolved arguments"| EXEC
    EXEC -->|"arguments"| FETCH
```

### 2b. Error Handling & Graceful Degradation

```mermaid
flowchart TD
    LOAD(LoadSecretsFromWebDav)
    DAV[(NextCloud WebDAV)]
    NO_DAV[Skip: No WebDAV]
    NOT_FOUND[Skip: File not found]
    PARSE_ERR[Warn: TOML parse error]
    EMPTY[Skip: Empty secrets table]
    INJECT(InjectSecretsIntoHeaders)
    KEY_MISS[Warn: Key not found]
    PASS[Passthrough: original value]

    LOAD -.->|"webdav is None"| NO_DAV
    LOAD -.->|"NotFound error"| NOT_FOUND
    LOAD -.->|"invalid TOML"| PARSE_ERR
    LOAD -.->|"secrets table empty"| EMPTY
    NO_DAV -->|"return None"| PASS
    NOT_FOUND -->|"return None"| PASS
    PARSE_ERR -->|"return None"| PASS
    EMPTY -->|"return None"| PASS
    INJECT -.->|"secret:missing_key"| KEY_MISS
    KEY_MISS -->|"keep original secret:missing_key"| PASS
```

### 2c. Secret Reference Replacement

```mermaid
flowchart LR
    INPUT["Header value string<br/>e.g. 'token secret:gitea_token'"]
    SCAN[Scan for secret: prefix]
    EXTRACT["Extract key<br/>chars: a-zA-Z0-9_-"]
    LOOKUP{Key in SecretMap?}
    REPLACE["Replace secret:key<br/>with actual value"]
    KEEP["Keep secret:key<br/>log warning"]
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

## 3. Data Structures

### `SecretsToml` (parsed TOML root)

| Field     | Type                    | Notes                                    |
|-----------|-------------------------|------------------------------------------|
| `secrets` | `HashMap<String, String>` | Flat key-value table. `#[serde(default)]` handles absent table as empty map. |

### Secrets TOML File Format

```toml
[secrets]
gitea_token = "abc123"
github_api_key = "sk-xyz789"
```

Stored at WebDAV root path `secrets.toml` (not inside any room directory).
Global scope — shared across all rooms and conversations.

### Secret Reference Format

Header values containing the substring `secret:<key>` where `<key>` is a
contiguous sequence of `[a-zA-Z0-9_-]` characters. The `secret:<key>` token
is replaced in-place, preserving surrounding text.

| Input                                    | Secrets Map                         | Output                   |
| ---------------------------------------- | ----------------------------------- | ------------------------ |
| `"token secret:gitea_token"`             | `{"gitea_token": "abc123"}`        | `"token abc123"`         |
| `"secret:api_key"`                       | `{"api_key": "sk-xyz"}`            | `"sk-xyz"`               |
| `"Bearer secret:tok extra"`             | `{"tok": "real"}`                   | `"Bearer real extra"`    |
| `"secret:missing"`                       | `{"other": "val"}`                  | `"secret:missing"` (warn)|

## 4. Key Functions

| Function | Location | Role |
|----------|----------|------|
| `load_secrets_from_webdav` | `harness.rs` | Async: reads `secrets.toml` from WebDAV root, parses TOML, returns `Option<HashMap<String, String>>` |
| `inject_secrets_into_headers` | `harness.rs` | Sync: parses arguments JSON, iterates headers object, replaces `secret:<key>` in string values |
| `replace_secret_refs` | `harness.rs` | Sync: single-pass string replacement of `secret:<key>` tokens against the secrets map |
