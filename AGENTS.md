# AGENTS.md — rockbot

## User directives

- **Ignore all Python code.** Do not read, edit, or reason about `bot/`, `util/`, `rock.py`, `requirements.txt`, or any `.py` files.

## Project layout

```
crate-rocketchat/     # ONLY Rust crate in the workspace — standalone RocketChat client library
bot/                   # Python app (IGNORED per user directive)
_dfds/ _docs/          # Mermaid data flow diagrams and architecture docs
example.config.toml    # template for config; real config.toml is gitignored
```

The `crate-rockbot/` directory described in README.md **does not exist yet**.
The application binary is currently Python, not Rust.

## Build & test

```bash
cargo build --release          # workspace build

# Unit tests + integration tests that don't need a server:
cargo test                     # from workspace root or crate-rocketchat/

# Real integration tests (require running RocketChat server + valid config.toml):
cargo test --test integration_real -- --ignored
```

No CI, no rustfmt.toml, no clippy.toml, no rust-toolchain file.

## Code style

- **Use async Rust everywhere.** Prefer `async fn` over sync functions. Use `tokio` as the async runtime. All I/O (HTTP, WebSocket, file, subprocess) must be async.
- **Edition 2024**, MSRV **1.85**. Use modern Rust (`impl Trait` in return position allowed).

## Key facts
- `Cargo.lock` is gitignored — atypical for a binary crate.
- `crate-rocketchat/` has both `lib.rs` (public API) and `main.rs` (manual debug binary that connects to a RocketChat server and logs events).
- `config.toml` is gitignored; use `example.config.toml` as a reference. Real integration tests read `config.toml` from the workspace root.
- The `rocketchat` crate uses `thiserror` for errors, `serde`/`serde_json` for serialization, `tokio-tungstenite` with `rustls-tls-native-roots` for WebSocket TLS.

## Testing

- `tests/integration.rs` — message parsing and filtering tests (runs without a server).
- `tests/integration_real.rs` — all tests `#[ignore]`d. Require `config.toml` with `[server]` section and a running RocketChat instance.

## OpenCode skills

- `dfd-md` — Creates Data Flow Diagrams as `.md` files using Mermaid flowchart syntax.
- `mermaid-cli` — Validates Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate/fix Mermaid syntax.
