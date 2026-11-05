# Probes (data collection)

Probes are live-data collection artifacts — not tests. They talk to real
servers/APIs to measure or observe, but carry **no assertions**: they print
findings (e.g. char/token ratios, response shapes) for use in DFD Phase 1
design or calibration.

## Convention

| Suffix | Purpose | Assertions | In suite inventory |
|--------|---------|------------|--------------------|
| `*_real.rs` | Validate implemented behavior against live servers | Yes (pass/fail) | Yes (`real.md`) |
| `*_probe.rs` | Collect measurements for design/calibration | No | No |

- Probes are never counted in the suite totals (595 tests) and are not listed
  in [real.md](real.md), [running.md](running.md) or [user.md](user.md).
- They run with `cargo test --test <name> -- --ignored` like real tests, but
  "passing" means only "ran without error".

## History

- `crate-rockbot/tests/provider_token_probe.rs` — measured DeepSeek/OpenRouter
  char/token ratios for memory token-pressure calibration. Removed after
  serving its purpose; do not resurrect without a concrete need.
- Probe code can also live as throwaway scripts under `./tmp/` (see AGENTS.md).
