# Running Tests

## Core tests (408 inline `#[cfg(test)]` in `src/`)

Fine-grained unit tests against single modules and DFDs. Run with plain `cargo test` — no credentials or servers required.

| Crate | Count | Key modules |
|-------|-------|-------------|
| rocketchat | 40 | ddp.rs (18), types.rs (17), rest.rs (5) |
| rockbot | 330 | memory.rs (35), harness.rs (49), provider/ (80), tools/ (132), knowledge.rs (10), types.rs (9) |
| webdav | 38 | client.rs (20), path.rs (10), calendar.rs (8) |

## User tests (164 in `tests/` — wiremock + in-memory)

Multi-DFD integration tests. Also run with `cargo test` — wiremock mocks HTTP; no live services.

| Test file | Crate | Tests | Mocked | Covers |
|-----------|-------|-------|--------|--------|
| `tests/integration.rs` | rocketchat | 30 | — | DDP messages, MessageFilter, config, BotReply |
| `tests/integration_rest.rs` | rocketchat | 9 | wiremock | REST API: get_rooms, send_message, resolve_fname |
| `tests/integration_mock.rs` | rockbot | 61 | wiremock | DeepSeek, OpenRouter, Fal, OpenRouterImage, WebDavTool, MemoryManager |
| `tests/provider_tests.rs` | rockbot | 46 | — | Config parsing, type serde, error handling, provider construction |
| `tests/integration.rs` | webdav | 10 | — | Client construction, WebDavPath helpers |
| `tests/config_tests.rs` | webdav | 7 | — | Config deserialization, URL path building |
| `tests/knowledge_real.rs` | rockbot | 1 | — | Public API accessibility (compile-time) |

## Real tests (22 integration + 1 flaky unit = 23 `#[ignore]` — live servers)

Connect to live RocketChat, NextCloud WebDAV, and fal.ai. Requires real credentials.

| Test file | Crate | Tests | Services | Credentials |
|-----------|-------|-------|----------|-------------|
| `tests/integration_real.rs` | rocketchat | 7 | DDP, REST, WebDAV | `config.toml` |
| `tests/integration_dual.rs` | rocketchat | 3 | DDP | `config.toml` |
| `tests/fal_real.rs` | rockbot | 1 | fal.ai | `config.toml` |
| `tests/image_gen_real.rs` | rockbot | 2 | fal.ai, WebDAV | `config.toml` or env vars |
| `tests/knowledge_real.rs` | rockbot | 2 | WebDAV | env vars or `config.toml` |
| `tests/integration_real.rs` | webdav | 7 | WebDAV | env vars or `config.toml` |
| `src/ddp.rs` (inline) | rocketchat | 1 | — | *(flaky unit test, not a real integration)* |

### Real test credentials

| Source | Priority | Tests using it |
|--------|----------|----------------|
| `config.toml` `[rocketchat.server]` | 1st | `integration_real.rs`, `integration_dual.rs` |
| `config.toml` `[webdav]` | 1st | `knowledge_real.rs`, `integration_real.rs` (webdav) |
| `config.toml` `[[image_providers]]` (fal) | 1st | `fal_real.rs` |
| `WEBDAV_URL` / `WEBDAV_USERNAME` / `WEBDAV_PASSWORD` / `WEBDAV_ROOT` env vars | 1st/fallback | `integration_real.rs` (webdav), `knowledge_real.rs` |
| `CONFIG_FILE` env var | Override | All |

### Run real tests

```bash
# All real integration tests
cargo test -- --ignored

# Per crate
cargo test -p rocketchat -- --ignored
cargo test -p rockbot -- --ignored
cargo test -p webdav -- --ignored

# Single test file
cargo test --test integration_real -- --ignored
cargo test --test integration_dual -- --ignored
cargo test --test fal_real -- --ignored
cargo test --test image_gen_real -- --ignored
cargo test --test knowledge_real -- --ignored
cargo test -p webdav --test integration_real -- --ignored

# Single test
cargo test -p webdav --test integration_real test_real_exists -- --ignored

# With logging
RUST_LOG=debug cargo test -p rocketchat --test integration_real test_connect_and_receive_events -- --ignored --nocapture
```
