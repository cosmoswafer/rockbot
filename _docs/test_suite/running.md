# Running Tests

## Unit + mock-integration tests (always run)

All unit tests (inline `#[cfg(test)]` modules in `src/`) and mock-based integration tests run with plain `cargo test` — no credentials, servers, or network required.

| Test file | Crate | Covers |
| --------- | ----- | ------ |
| `src/**.rs` (inline) | all | Unit tests for each module |
| `tests/integration.rs` | rocketchat | Message parsing, filtering, config deserialization |
| `tests/integration.rs` | webdav | Client construction, `WebDavPath` helpers |
| `tests/config_tests.rs` | webdav | Config deserialization, base URL construction |
| `tests/provider_tests.rs` | rockbot | Config parsing, types serde, error handling, provider construction |
| `tests/integration_mock.rs` | rockbot | `wiremock`-based HTTP mock tests for DeepSeek and OpenRouter providers |

## Real-integration tests (`#[ignore]`)

These tests connect to live services and require real credentials. Run with `cargo test -- --ignored`.

### `crate-rocketchat/tests/integration_real.rs` — requires `config.toml` + running RocketChat server

| Test | What it does | Credentials needed |
| ---- | ------------ | ------------------ |
| `test_config_toml_exists_and_parses` | Reads `config.toml` and verifies the `[rocketchat]` section | `url`, `username`, `password` in `config.toml` |
| `test_connect_and_receive_events` | Opens a WebSocket, subscribes to `stream-room-messages`, logs incoming messages (30s timeout) | Same + running RocketChat instance |
| `test_send_message_and_verify` | Validates WebSocket URI and host extraction from config | `config.toml` only (no server needed for URI parsing) |

### `crate-webdav/tests/integration_real.rs` — requires WebDAV env vars + running NextCloud

| Test | What it does | Credentials needed |
| ---- | ------------ | ------------------ |
| `test_real_ensure_directory_and_list` | Creates a directory, lists it with PROPFIND, then cleans up | `WEBDAV_URL`, `WEBDAV_USERNAME`, `WEBDAV_PASSWORD` |
| `test_real_write_and_read_file` | Writes a file, reads it back, verifies content, cleans up | Same |
| `test_real_write_file_auto_mkcol` | Writes to a deeply-nested path with `X-NC-WebDAV-AutoMkcol`, reads back, cleans up | Same |
| `test_real_exists` | Checks a path doesn't exist, creates it, verifies exist check, cleans up | Same |

Env vars for WebDAV tests:

| Variable | Required | Example |
| -------- | -------- | ------- |
| `WEBDAV_URL` | Yes | `https://nextcloud.example.com` |
| `WEBDAV_USERNAME` | Yes | `bot` |
| `WEBDAV_PASSWORD` | Yes | `app-password-xxxx` |
| `WEBDAV_ROOT` | No (defaults to `rockbot-test`) | `rockbot` |

Run individual real-integration suites:
```bash
cargo test -p rocketchat --test integration_real -- --ignored
cargo test -p webdav --test integration_real -- --ignored
```
