# Test Suite Summary

Total: **594 tests** across 3 crates (408 core + 164 user + 22 real + 1 flaky unit)

## Test Layers

| Layer | Description | Count | Docs |
|-------|-------------|-------|------|
| **Core** | Fine-grained unit tests (inline `#[cfg(test)]` in `src/`) | 408 | [core.md](core.md) |
| **User** | Integration/scenario tests in `tests/` (wiremock + in-memory) | 164 | [user.md](user.md) |
| **Real** | Live server tests (`#[ignore]` in `tests/`) requiring credentials | 22 | [real.md](real.md) |

## By Crate

| Crate | Core | User | Real | Total |
|-------|------|------|------|-------|
| rocketchat | 40 | 39 | 10 | **89** |
| rockbot | 330 | 108 | 5 | **443** |
| webdav | 38 | 17 | 7 | **62** |
| **Total** | **408** | **164** | **22** | **594** |

## Test Characteristics

| Feature | Count |
|---------|-------|
| Async (`#[tokio::test]`) | ~144 |
| Ignored (`#[ignore]`) | 23 |
| Mock-based (wiremock) | ~70 |
| Mock-based (inline MockProvider/MockTool) | ~105 |
| In-memory / no I/O | ~420 |

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
