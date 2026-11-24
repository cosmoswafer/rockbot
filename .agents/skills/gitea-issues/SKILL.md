---
name: gitea-issues
description: "Use when working with Gitea issues: list, view, create, comment, edit, tick task checklists, apply labels and milestones, review a milestone, or call any raw issue endpoint. Ships a stdlib-Python CLI (scripts/gitea.py) plus bash/curl recipes and a read-only doctor; the server, owner/repo and token are derived from the git remote and .env, never hardcoded. Never closes an issue on its own initiative — it comments and lets a human close — and refuses `close` without --yes."
license: CC0-1.0
compatibility: "Python 3.8+ (standard library only — no pip, no requests) or git + curl + jq for the bash path. Needs GITEA_TOKEN in .env and an `origin` remote. No database, no project code."
---

# gitea-issues — Gitea issue management

Issues, comments, labels, milestones and the raw API, from one command:

```bash
python3 .agents/skills/gitea-issues/scripts/gitea.py <command> [options]
```

Prefer the CLI over hand-written `curl`: every invocation re-reads `.env` and
re-derives the server from the remote by itself, so the classic failure — a
token that only existed in the shell where you set it — cannot happen. The bash
and `curl` recipes stay in
[references/curl-recipes.md](references/curl-recipes.md) for one-off requests,
and `scripts/gitea-env.sh` is the sourceable setup for them.

## Files in this skill

| Path                                     | What it is                                                                                           |
| ---------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `scripts/gitea.py`                       | the CLI entry point (`--help` for the live list)                                                     |
| `scripts/gitea_issues/`                  | its modules: `env`, `client`, `template`, `tasks`, `labels`, `milestones`, `issues`, `render`, `cli` |
| `scripts/gitea-env.sh`                   | sourceable setup plus `gitea_get`/`gitea_post` helpers for the bash path                             |
| `scripts/run-tests.py`, `scripts/tests/` | the offline suite (a routing fake, no network)                                                       |
| `references/cli-reference.md`            | every command, argument and default                                                                  |
| `references/curl-recipes.md`             | the no-Python path, recipe by recipe                                                                 |
| `references/api-notes.md`                | endpoint rules, gotchas and troubleshooting in full                                                  |

## First move: check the setup

```bash
python3 .agents/skills/gitea-issues/scripts/gitea.py doctor
```

```text
== gitea-issues diagnostics ==
PASS  python 3.12.3 at /usr/bin/python3
PASS  git: /usr/bin/git
PASS  env file: /path/to/repo/.env
PASS  GITEA_TOKEN: set (40 characters, value hidden)
INFO  remote: origin https://git.example.com/team/repo.git
INFO  api:    https://git.example.com/api/v1
INFO  repo:   team/repo
PASS  repo access: team/repo
PASS  labels readable: 12 item(s)
PASS  milestones readable: 2 item(s)
WARN  token user: bot (inferred from the newest issue — the token has no read:user scope)
```

Every `FAIL` names the fix. A `WARN` on the last line is harmless: issue
endpoints work without `read:user`. The token value is never printed by any
command.

### Setup — how the connection is resolved

| Thing        | Source                                         | Escape hatch                                            |
| ------------ | ---------------------------------------------- | ------------------------------------------------------- |
| API base     | `origin` remote host, HTTPS, port dropped      | `--api https://host:8443/api/v1`, `GITEA_API` in `.env` |
| `owner/repo` | path part of the `origin` remote               | `--repo owner/repo`, `GITEA_OWNER_REPO`                 |
| Token        | process env, then root `.env`, then cwd `.env` | `--env-file PATH`, `GITEA_ENV_FILE`                     |
| Which remote | `origin`                                       | `--remote upstream`, `GITEA_REMOTE`                     |

Never hardcode a host or an `owner/repo`. The SSH port in a remote belongs to
git; the API is served over HTTPS.

## Everyday commands

```bash
Gitea=.agents/skills/gitea-issues/scripts/gitea.py

python3 $Gitea list --limit 20                      # open issues, with task progress
python3 $Gitea list --milestone v3.7.10 --label working --state open
python3 $Gitea view 687 --comments                  # one issue, body and thread
python3 $Gitea create --title "Ring: stale total" \
    --summary "The ring keeps the old total after a delete." \
    --task "Recompute on delete" --task "Add a regression test" \
    --milestone v3.7.10 --label bug --related 615 --assign-me
python3 $Gitea comment 687 --body-file ./tmp/note.md
python3 $Gitea tasks 687                            # numbered checklist
python3 $Gitea tasks 687 tick 1,3                   # tick specific tasks
python3 $Gitea tasks 687 tick all --drop "Acceptance criteria"
python3 $Gitea label status 687 responded committed # the shared status vocabulary
python3 $Gitea milestone report v3.7.10             # progress across a milestone
python3 $Gitea api GET repos/OWNER/REPO/issues/687/timeline   # anything uncovered
```

Useful global flags: `--json` (stdout is pure JSON, safe to pipe), `--dry-run`
(print the writes on stderr, send none), `-q/--quiet`, `--repo`, `--api`. Every
command answers `<command> --help`, and
[references/cli-reference.md](references/cli-reference.md) lists the whole
surface: arguments, defaults, and the typical sequences. Exit codes: `0` ok, `1`
failure, `2` bad usage, `3` the close guard refused.

| Command                                                                | What it does                                                                         |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `doctor`, `me`, `template`                                             | diagnostics; resolve the token user; print the issue body template                   |
| `list`, `view`, `comments`                                             | read issues, filtered by state, label, milestone, assignee, author, text             |
| `create`, `edit`, `comment`                                            | file an issue (template-checked), change fields/body/labels/assignees, add a comment |
| `tasks N tick\|untick\|drop`                                           | rewrite the `- [ ]` checklist in the body — the only progress record Gitea has       |
| `label list\|create\|add\|remove\|set\|show\|status\|clear\|bootstrap` | repo labels and issue labels, including the five status labels                       |
| `milestone list\|latest\|create\|set\|unset\|issues\|report\|state`    | milestone lookup, creation, assignment, review                                       |
| `close`, `reopen`                                                      | state changes — `close` needs `--yes`, see the policy below                          |
| `api METHOD PATH`                                                      | raw call for endpoints the CLI does not wrap                                         |

## The issue template (required for every new issue)

`create` refuses a body that breaks these rules unless you pass `--force`. Print
the template with `template` (or `template --out ./tmp/issue.md`).

```markdown
## Summary

<!-- 1–3 sentences: what is wrong / what should change, and why. -->

## Tasks

- [ ] <concrete, verifiable task>
- [ ] <concrete, verifiable task>

## Related

- <!-- related issues / PRs (#123), files, screenshots, references -->
```

- `## Tasks` is **required**, with at least one `- [ ]` item. Gitea renders the
  checklist as a progress counter (`2/5`) in the issue list, so the body _is_
  the progress record — there is no checkbox API. One task is one commit or one
  other reviewable change.
- Tick a task by rewriting the body (`tasks N tick …`). Never edit the body by
  hand when a plan tool owns it: where the `gitea-bots` skill exists, mirror a
  plan with `sync-tasks.py <N> <plan-file>`.
- **No `## Acceptance criteria` section.** Put observable conditions in
  `## Tasks` as verifiable items. Acceptance criteria only copy the tasks and
  add unworkable rows to the progress counter; the milestone review drops the
  section anyway.
- Headings may be translated (`## 待辦` for `## Tasks`); keep the checklist
  syntax intact.
- Omit `## Related` only when it adds nothing.

## Hard rules

- **Never close an issue on your own initiative.** A human decides when an issue
  is done. Post a comment with what changed and let them close it. The CLI
  enforces this: `close` exits `3` without `--yes`, and `--yes` is only for a
  human's explicit request in this conversation.
- **No auto-closing keywords in commits.** `close #415`, `fixes #415`,
  `resolves #415` make Gitea close the issue on push. Reference work with the
  plain `#<number>` form instead — Gitea links it either way:

  ```text
  feat(ring): recompute the total after a delete (#687)
  ```

  ```text
  fix(camera): use server time, not the photo EXIF date

  Ref: #687
  ```

  `# 687`, `issue 687` and `Ref: 687` are **not** recognized — the `#` must
  directly precede the number. One message may reference several issues.
- **Never print the token.** The scripts read it silently and keep it out of
  every message and error. Do not add `set -x` to the bash recipes, and never
  `echo $GITEA_TOKEN`.

## Milestone review

When asked to review a milestone (say `v3.7.10`):

```bash
python3 $Gitea milestone report v3.7.10   # per-issue checklist progress and labels
```

For each issue, decide whether the codebase implements it:

1. **Check for the implementation.** Grep the files and symbols named in
   `## Tasks` / `## Related`, and look for the commit that landed it:
   `git log --all --oneline --grep "<issue number>"`. Missing code means the
   issue is not implemented.
2. **Implemented → tick everything and drop the acceptance criteria.**
   `tasks N tick all --drop "Acceptance criteria"`, then `comment N` pointing at
   the commit(s) and the verification performed. A fully ticked body means
   _implemented, ready for review, waiting to close_. Do **not** close it.
3. **Review found problems → open a follow-up issue** that references the
   original (`--related N`); do not reopen or silently widen the old one. A
   human decides what happens to the original.
4. **Not implemented → leave the boxes empty.** Say nothing beyond what the
   empty checkboxes already show.

Status labels while work moves (`label status`, shared with the `issue-labels`
and `gitea-bots` skills): `seeing` → `responded` → `working` →
`responded committed`. Filing an issue writes no label; `seeing` marks a real
pickup.

## API rules that bite

The scripts already obey these; they matter when you hand-write a request. The
full list, with symptoms and fixes, is in
[references/api-notes.md](references/api-notes.md).

- Auth header is `Authorization: token <value>` — the literal word `token`.
  `Bearer` fails even with a valid token.
- Milestones: the key is `milestone` with a **numeric ID**. `milestone_id` and
  milestone names are ignored silently by `POST` and `PATCH`. `milestone: 0`
  clears it.
- Labels: an issue `PATCH` ignores `labels` silently. Use
  `PUT /repos/{owner}/{repo}/issues/{n}/labels` with the **full** set of numeric
  label IDs — it replaces, so keep the unrelated labels you read first.
- The issues endpoint returns pull requests too (a PR has a non-null
  `pull_request`). `list` hides them unless `--include-pulls`.
- Repo slugs are case-sensitive (`WeightManagement-frontend` ≠
  `weightmanagement-frontend`), and a missing token on a private repo answers
  **404**, not 401.
- Pagination: 30 per page by default; the CLI paginates with `--all`.

## When Python is not available

Use the bash path: source `scripts/gitea-env.sh` in the _same_ shell call as the
`curl`, then call the helpers — they set `GITEA_API`, `GITEA_OWNER_REPO`,
`GITEA_TOKEN` and define `gitea_get`, `gitea_post`, `gitea_patch`, `gitea_put`,
`gitea_code`, `gitea_selfcheck`, `gitea_login`:

```bash
source .agents/skills/gitea-issues/scripts/gitea-env.sh
gitea_get "/issues?state=open&limit=50" | jq '.[] | {number, title}'
```

More recipes (create with a templated body, label writes, milestones, raw
payloads) are in [references/curl-recipes.md](references/curl-recipes.md).

## Testing these scripts

The whole toolkit is covered by an offline suite — a routing fake replaces the
network, so it asserts the exact payloads without touching a real tracker:

```bash
python3 .agents/skills/gitea-issues/scripts/run-tests.py        # whole suite
python3 .agents/skills/gitea-issues/scripts/run-tests.py -v
python3 .agents/skills/gitea-issues/scripts/run-tests.py tests.test_cli
```

Run the suite after any change to `scripts/gitea_issues/`. For live checks,
prefer read-only commands (`doctor`, `list`, `view`, `milestone report`) and
`--dry-run` for writes, so nothing is posted to a real repository unless the
user asked for it.

## Troubleshooting at a glance

| Symptom                                                 | Cause and fix                                                                                    |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `invalid username, password or token`                   | `Bearer` used instead of `Authorization: token …`                                                |
| `404 page not found` / `not found`                      | wrong host or `owner/repo` — run `doctor`; fix the remote with `git remote set-url origin <url>` |
| `user redirect does not exist`                          | wrong casing of the repo slug                                                                    |
| `token does not have at least one of required scope(s)` | no `read:user`: `me` falls back to the newest issue's creator; assignees still work              |
| bare `401`/`404` from bash snippets                     | the token was not in that shell — one `source …/gitea-env.sh` per call, or use the CLI           |
| milestone or label silently unchanged                   | wrong key/type: see [references/api-notes.md](references/api-notes.md)                           |
