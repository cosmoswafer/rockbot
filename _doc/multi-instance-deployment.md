# Multi-instance deployment (design)

> Archived from AGENTS.md — design constraints for running multiple rockbot
> instances concurrently.

Multiple bot instances may run concurrently, each driven by its own `CONFIG_FILE`. Two instances may intentionally share the same WebDAV root so they present **one shared identity** (different LLMs, one persona) to the same DM user. Constraints of this design:

- **One soul, two brains** — both instances read/write the same `soul.md`, re-read from WebDAV on every incoming message (pull-based; no polling/push); per-bot identity must not live there.
- **No write coordination** — `edit_soul` is an unconditional PUT (last-write-wins); concurrent edits from both bots can lose a write.
- **`state_dir` must differ per instance** even when the WebDAV root is shared (Matrix SDK session stores must not collide).
- **Snapshots isolated per bot**: `{root}/{snapshot_prefix}/{bot_id}/{webdav_dir}/snapshot.json` — see `_dfd/memory/memory/partitioning.md`.
- Hostnames, accounts, config paths, restart commands = deployment info, gitignored `_doc/config-files.local.md` only — never the repo.
