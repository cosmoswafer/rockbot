# Test Suite Summary

Total: **558 tests** across 3 crates (376 core + 161 user + 21 real)

## Test Layers

| Layer | Description | Count | Docs |
|-------|-------------|-------|------|
| **Core** | Fine-grained unit tests (inline `#[cfg(test)]` in `src/`) | 376 | [core.md](core.md) |
| **User** | Integration/scenario tests in `tests/` (wiremock + in-memory) | 161 | [user.md](user.md) |
| **Real** | Live server tests (`#[ignore]`) requiring credentials | 21 | [real.md](real.md) |

## By Crate

| Crate | Core | User | Real | Total |
|-------|------|------|------|-------|
| rocketchat | 35 | 39 | 10 | **84** |
| rockbot | 303 | 105 | 3 | **411** |
| webdav | 38 | 17 | 7 | **62** |
| **Total** | **376** | **161** | **21** | **558** |

## Test Characteristics

| Feature | Count |
|---------|-------|
| Async (`#[tokio::test]`) | ~120 |
| Ignored (`#[ignore]`) | 21 |
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
