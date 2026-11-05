# Error Handling & Fallbacks

## 1. Purpose

Models the error paths of config loading: TOML parse failures surface as hard
errors, while a missing file falls back to empty defaults before validation,
and validation failures report missing providers or empty server credentials.
Complements the happy path in [../main-path.md](../main-path.md).

## 2. Diagram

```mermaid
flowchart TD
    LOAD_USR(DeserializeConfig\nwith serde defaults)
    ERR_PARSE[Error: TOML Parse]
    ERR_VALID[Error: Validation]
    VALIDATE(ValidateConfig)

    LOAD_USR -->|"error: parse failure"| ERR_PARSE
    LOAD_USR -->|"error: file not found\n(empty defaults used)"| VALIDATE
    VALIDATE -->|"error: provider not found"| ERR_VALID
    VALIDATE -->|"error: server credentials empty"| ERR_VALID
```
