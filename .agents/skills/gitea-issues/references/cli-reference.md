# gitea.py — command reference

The full surface of `scripts/gitea.py`. Run `gitea.py <command> --help` for the
live version. Every command also takes the global options below.

Convention: results go to **stdout** (text, or JSON with `--json`); hints,
warnings, dry-run plans and errors go to **stderr**. Exit codes: `0` ok, `1`
failure, `2` bad usage, `3` the close guard refused.

## Global options

| Option              | Meaning                                                 |
| ------------------- | ------------------------------------------------------- |
| `--repo OWNER/REPO` | override the `owner/repo` derived from the remote       |
| `--remote NAME`     | which git remote to derive from (default `origin`)      |
| `--api URL`         | override the API base, e.g. `https://host:8443/api/v1`  |
| `--env-file PATH`   | read `GITEA_TOKEN` from this file instead of `.env`     |
| `--timeout SECONDS` | HTTP timeout (default 25)                               |
| `--insecure`        | skip TLS verification — self-signed servers only        |
| `--dry-run`         | print the writes on stderr, send none (reads still run) |
| `--json`            | machine-readable result on stdout                       |
| `-q`, `--quiet`     | no prose                                                |

## Reading

| Command             | Arguments and options                                                                                                                                                                                                                        |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `doctor`            | no arguments. Read-only checks: python, git, `.env`, token, remote, repo access, labels, milestones, token user. Exit `1` on any `FAIL`                                                                                                      |
| `me`                | no arguments. Prints the token user's login and where it came from (`/user` or the newest issue)                                                                                                                                             |
| `list` (alias `ls`) | `--state open\|closed\|all` (default `open`), `--label NAME` (repeatable), `--milestone REF`, `--assignee LOGIN`, `--author LOGIN`, `--search TEXT`, `--include-pulls`, `--limit N` (default 30), `--all` (walk every page), `--columns K,K` |
| `view NUMBER`       | `--comments` (include the thread), `--limit N` (max comments, default 50)                                                                                                                                                                    |
| `comments NUMBER`   | `--limit N` (default 50)                                                                                                                                                                                                                     |
| `tasks NUMBER`      | no action lists the checklist with its index numbers                                                                                                                                                                                         |
| `api METHOD PATH`   | raw call; `PATH` omits `/api/v1`, e.g. `repos/o/r/issues/687/timeline`; `--data JSON`, `--input PATH`, `--param K=V` (repeatable)                                                                                                            |

Column keys for `--columns`: `number`, `state`, `tasks`, `labels`, `milestone`,
`comments`, `title` (default) plus `assignees`, `author`, `created`, `updated`,
`url`.

## Writing

| Command                         | Arguments and options                                                                                                                                                                                                                                                                                                                                                                                              |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `create`                        | `--title` (required); body from `--body TEXT` / `--body -` (stdin) / `--body-file PATH`, **or** generated from `--summary TEXT` + `--task TEXT` (repeatable) + `--related N` (repeatable); `--milestone REF` with `--create-milestone` and `--due YYYY-MM-DD`; `--label NAME` (repeatable) with `--create-labels`; `--assign LOGIN` (repeatable) or `--assign-me`; `--force` files a body that breaks the template |
| `edit NUMBER`                   | `--title`, `--body`/`--body-file`, `--related N` (append to `## Related`), `--milestone REF` / `--no-milestone`, `--assign LOGIN` / `--unassign LOGIN` (both repeatable), `--label-add NAME` / `--label-remove NAME` with `--create-labels`, `--force`                                                                                                                                                             |
| `comment NUMBER`                | `--body` / `--body -` / `--body-file PATH`; optionally `--label NAME` (repeatable) and `--status TOKEN` (repeatable) in the same call                                                                                                                                                                                                                                                                              |
| `tasks NUMBER ACTION [TARGET…]` | `ACTION` is `show` (default), `tick`, `untick` or `drop`. `TARGET` is `all`, an index (`3`), a list (`1,4`), a range (`2-5`), or a phrase matching exactly one task. `--drop HEADING` (repeatable) deletes a whole section; with `drop` the targets are the headings                                                                                                                                               |
| `close NUMBER`                  | **refuses to run without `--yes`** (exit `3`), which stands for an explicit human request; `--note TEXT` / `--note-file PATH` posts a comment first; `--status TOKEN` applies status labels                                                                                                                                                                                                                        |
| `reopen NUMBER`                 | `--note TEXT` / `--note-file PATH`                                                                                                                                                                                                                                                                                                                                                                                 |

## Labels

| Command                      | Arguments and options                                                                                                                                                                                                                                                    |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `label list`                 | repo labels: id, name, colour                                                                                                                                                                                                                                            |
| `label show NUMBER`          | the labels on one issue                                                                                                                                                                                                                                                  |
| `label create NAME`          | `--color HEX6` (default `ededed`), `--description TEXT`; idempotent                                                                                                                                                                                                      |
| `label bootstrap`            | create the five canonical status labels (`👀 seeing`, `💬 responded`, `🔨 working`, `❓ question`, `✅ committed`) with their colours and descriptions                                                                                                                   |
| `label add NUMBER NAME…`     | adds labels, keeping the rest; `--create` makes missing labels first                                                                                                                                                                                                     |
| `label remove NUMBER NAME…`  | removes labels, keeping the rest                                                                                                                                                                                                                                         |
| `label set NUMBER NAME…`     | replaces the whole set; no names clears all labels                                                                                                                                                                                                                       |
| `label status NUMBER TOKEN…` | applies the status vocabulary and drops any other status label (current or legacy spelling), keeping every unrelated label. Tokens: `seeing`, `responded`, `working`, `question`, `committed`, `none`, or the legacy `inspect`, `plan`, `implement`, `questions`, `done` |
| `label clear NUMBER`         | strips every status label, unrelated labels stay                                                                                                                                                                                                                         |

All of these write through `PUT /issues/{n}/labels` with the full numeric ID set
— an issue `PATCH` ignores `labels`.

## Milestones

| Command                            | Arguments and options                                                                                                              |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `milestone list`                   | `--state open\|closed\|all` (default `open`); shows ids, titles, open/closed issue counts, due date                                |
| `milestone latest`                 | the newest open milestone by creation date                                                                                         |
| `milestone create TITLE`           | `--due YYYY-MM-DD`, `--description TEXT`; idempotent — an existing title is reported, not duplicated                               |
| `milestone set NUMBER REF`         | assigns by title, unique title fragment, or numeric ID; `--create-milestone` and `--due` open the milestone when it does not exist |
| `milestone unset NUMBER`           | clears the milestone (`milestone: 0`)                                                                                              |
| `milestone issues REF`             | issues in a milestone; `--state open\|closed\|all`                                                                                 |
| `milestone report REF`             | per-issue checklist progress and labels — the review board; `--state`                                                              |
| `milestone state REF open\|closed` | open or close a milestone                                                                                                          |

A reference that matches nothing lists the open milestones instead of failing
silently, and a fragment matching several is refused.

## Templates

`template` prints the standard issue body; `template --out PATH` writes it to a
file (refuses an existing file without `--force`). `create` checks every body
against the same rules and explains what is wrong.

## Typical sequences

File a planned issue and mark it picked up:

```bash
python3 $Gitea create --title "Ring: stale total" --summary "…" \
  --task "Recompute on delete" --task "Add a test" \
  --milestone v3.7.10 --label bug --assign-me
python3 $Gitea label status 900 seeing
```

Finish work and hand it to a human:

```bash
python3 $Gitea tasks 900 tick all
python3 $Gitea comment 900 --body-file ./tmp/completion.md --status responded committed
```

Review a milestone:

```bash
python3 $Gitea milestone report v3.7.10
python3 $Gitea tasks 900 tick all --drop "Acceptance criteria"   # implemented only
python3 $Gitea create --title "Follow-up: …" --summary "…" --task "…" --related 900
```

Never tick tasks for work that is not implemented, and never close: a human does
that after review.
