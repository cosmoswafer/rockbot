# Test Suite Summary

Total: **569 tests** across 3 crates (384 core + 161 user + 24 real)

## Test Layers

| Layer | Description | Count | Docs |
|-------|-------------|-------|------|
| **Core** | Fine-grained unit tests (inline `#[cfg(test)]` in `src/`) | 384 | [core.md](core.md) |
| **User** | Integration/scenario tests in `tests/` (wiremock + in-memory) | 161 | [user.md](user.md) |
| **Real** | Live server tests (`#[ignore]`) requiring credentials | 24 | [real.md](real.md) |

## By Crate

| Crate | Core | User | Real | Total |
|-------|------|------|------|-------|
| rocketchat | 39 | 39 | 11 | **89** |
| rockbot | 307 | 105 | 6 | **418** |
| webdav | 38 | 17 | 7 | **62** |
| **Total** | **384** | **161** | **24** | **569** |

## Test Characteristics

| Feature | Count |
|---------|-------|
| Async (`#[tokio::test]`) | ~120 |
| Ignored (`#[ignore]`) | 24 |
| Mock-based (wiremock) | 62 |
| Mock-based (inline MockProvider/MockTool) | 47 |
| In-memory / no I/O | ~375 |

## Running

```bash
# All core + user tests (no network)
cargo test

# All tests including real (needs credentials)
cargo test -- --ignored

# Single crate
cargo test -p webdav
cargo test -p rocketchat
cargo test -p rockbot

# Specific test files
cargo test -p rocketchat --test integration_real -- --ignored
cargo test -p webdav --test integration_real -- --ignored
```
