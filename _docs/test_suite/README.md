# Test Suite Summary

Total: **588 tests** across 3 crates (391 core + 174 user + 23 real)

## Test Layers

| Layer | Description | Count | Docs |
|-------|-------------|-------|------|
| **Core** | Fine-grained unit tests (inline `#[cfg(test)]` in `src/`) | 391 | [core.md](core.md) |
| **User** | Integration/scenario tests in `tests/` (wiremock + in-memory) | 174 | [user.md](user.md) |
| **Real** | Live server tests (`#[ignore]`) requiring credentials | 23 | [real.md](real.md) |

## By Crate

| Crate | Core | User | Real | Total |
|-------|------|------|------|-------|
| rocketchat | 39 | 39 | 11 | **89** |
| rockbot | 314 | 111 | 5 | **430** |
| webdav | 38 | 24 | 7 | **69** |
| **Total** | **391** | **174** | **23** | **588** |

## Test Characteristics

| Feature | Count |
|---------|-------|
| Async (`#[tokio::test]`) | ~170 |
| Ignored (`#[ignore]`) | 23 |
| Mock-based (wiremock) | 63 |
| Mock-based (inline MockProvider/MockTool) | 49 |
| In-memory / no I/O | ~395 |

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
