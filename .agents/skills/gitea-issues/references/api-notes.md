# Gitea issue API notes

The behaviour that silently breaks work, what it looks like, and what the CLI
does about it. Read this when a request returns something surprising, or when
you need to hand-write a call the CLI does not make.

## Endpoint map (as the CLI uses it)

| Action               | Call                                                                                                                                                                                                              |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| list issues          | `GET /repos/{o}/{r}/issues?state=&type=&assignee=&author=&q=&limit=&page=` — label and milestone filters run **locally**, because `labels=`/`milestones=` (numeric IDs) are not equally supported across versions |
| one issue            | `GET /repos/{o}/{r}/issues/{n}`                                                                                                                                                                                   |
| create               | `POST /repos/{o}/{r}/issues`                                                                                                                                                                                      |
| edit fields          | `PATCH /repos/{o}/{r}/issues/{n}`                                                                                                                                                                                 |
| comments             | `GET`/`POST /repos/{o}/{r}/issues/{n}/comments`                                                                                                                                                                   |
| issue labels (read)  | `GET /repos/{o}/{r}/issues/{n}/labels`                                                                                                                                                                            |
| issue labels (write) | `PUT /repos/{o}/{r}/issues/{n}/labels`                                                                                                                                                                            |
| repo labels          | `GET`/`POST /repos/{o}/{r}/labels`, `PATCH`/`DELETE /repos/{o}/{r}/labels/{id}`                                                                                                                                   |
| milestones           | `GET`/`POST /repos/{o}/{r}/milestones`, `PATCH /repos/{o}/{r}/milestones/{id}`                                                                                                                                    |
| token user           | `GET /user`                                                                                                                                                                                                       |
| timeline             | `GET /repos/{o}/{r}/issues/{n}/timeline`                                                                                                                                                                          |

`{o}/{r}` is the `owner/repo` derived from the remote; `/api/v1` is the prefix.

## Gotchas

### Authentication

- The header is `Authorization: token <value>` — the literal word **`token`**.
  `Bearer` answers 401 / `invalid username, password or token` even with a valid
  token.
- A missing or empty token on a **private** repository answers **404**, not 401:
  Gitea hides what you cannot see. This is why the CLI refuses to send anything
  before a token is loaded, and why `doctor` checks the token first.
- Scopes are per endpoint. Issue-scoped tokens (`write:issue`,
  `write:repository`) work for everything here but get 403 from `/user`. `me`
  and `doctor` fall back to the creator of the newest issue — the token user
  filed the issues it created — and say so in the output.

### Field names and types

| You want                   | Correct                                           | Silently wrong                                 |
| -------------------------- | ------------------------------------------------- | ---------------------------------------------- |
| milestone on an issue      | `"milestone": 87` (numeric ID)                    | `"milestone_id": 87`, `"milestone": "v3.7.10"` |
| clear a milestone          | `"milestone": 0`                                  | `null`, `""`                                   |
| labels on an issue         | `PUT …/issues/{n}/labels` with `{"labels":[ids]}` | `labels` inside the issue `PATCH`              |
| label payload values       | numeric label IDs                                 | label names (JSON unmarshal error)             |
| label colour               | `0e8a16`                                          | `#0e8a16`                                      |
| milestone due date (write) | `"due_date": "2026-12-31"`                        | `"due_on"` (that is the read field)            |
| assignees                  | login names in `"assignees": [...]`               | display names, emails                          |

`POST …/issues` also accepts `"labels": [ids]` on recent servers and ignores it
on older ones; `create_issue` checks the response and re-asserts the labels
through the labels endpoint when something is missing.

### Pull requests share the issues endpoint

`GET /repos/{o}/{r}/issues` returns issues **and** pull requests. A PR has a
non-null `pull_request` field. `type=issues` filters server-side on newer Gitea;
the CLI additionally filters locally, so the result is the same on every
version, and `--include-pulls` keeps them.

### The body is the only progress record

There is no checkbox API. `- [ ]` / `- [x]` lines under `## Tasks` are the
progress counter Gitea shows in lists, and changing one means `PATCH`ing the
whole body. Consequences the CLI handles:

- fenced code blocks are skipped, so an example `- [ ]` in a ```md block is
  never ticked;
- a phrase target must match exactly one task — two matches raise instead of
  guessing, because a wrongly ticked box is a false progress record;
- the rest of the body is copied verbatim, never reformatted.

### Pagination

Default page size is 30. `limit` caps a request (max 100); `page` walks the
rest. The CLI uses `get_all`, which stops on a short page or after `max_pages`.

### Case sensitivity and hosts

- Repo slugs are case-sensitive: `WeightManagement-frontend` ≠
  `weightmanagement-frontend`; the latter answers `user redirect does not exist`
  or 404.
- The API host comes from the `origin` remote. The port in an `ssh://` remote
  belongs to git, never to the API — the CLI drops it and uses HTTPS 443, and
  `--api https://host:8443/api/v1` covers a server on a non-standard port.

## Troubleshooting

1. **`{"message":"invalid username, password or token"}`** — wrong auth format
   (see above), or an expired/revoked token.
2. **`not found` / `404 page not found`** — wrong host or `owner/repo`. Run
   `doctor`: it prints the derived values and the repo's `full_name`. If the
   clone used the wrong remote, fix it (`git remote set-url origin <url>`), then
   re-run. Never assume `gitea.com`; that public instance is almost never this
   project's server.
3. **`user redirect does not exist`** — casing or owner in `owner/repo`. Copy it
   verbatim from the web UI.
4. **`token does not have at least one of required scope(s)`** — the endpoint
   needs a scope the token lacks. Everything except `/user` still works; use
   `me`'s fallback or create the issue without assignees and PATCH them later.
5. **Empty or unexpected response** — in the bash path, `.env` was not sourced
   in _that_ call. Print `API=` and `REPO=` first (`gitea_selfcheck`). Never
   print the token.
6. **A 401/404 on an endpoint that worked a moment ago** — the token is empty in
   this shell. Environment does not survive between shell calls: re-source
   `.env` in the same call as the request, or use the CLI, which resolves
   everything per invocation.
7. **HTTP 5xx when creating a label whose name has an emoji** — some Gitea
   installations cannot store 4-byte characters in label names. Retry with the
   bare name (`seeing`) and keep the canonical colour and description; read and
   write whichever spelling the repo actually stores. The CLI says this in the
   error it raises.
8. **Writes that "succeed" but change nothing** — the field name or type was
   silently ignored (see the table above). Check with
   `api GET repos/{o}/{r}/issues/{n}` what the server stored.
9. **`jq: command not found`** — drop the `| jq …` pipe; the raw JSON is still
   readable, or use the CLI, which needs neither `jq` nor `curl`.

## CLI internals worth knowing

- **Streams.** The result goes to stdout (text, or JSON with `--json`); hints,
  warnings, dry-run plans and errors go to stderr. Piping `--json` into `jq` is
  therefore always safe.
- **Exit codes.** `0` ok · `1` configuration, API or domain failure · `2` bad
  usage · `3` the close guard refused (no `--yes`).
- **`--dry-run`.** Reads run for real (so label and milestone names resolve
  against the server); writes are printed as `DRY-RUN METHOD path payload` on
  stderr and answered with a canned response so the same handler formats the
  plan. IDs in a dry-run result are placeholders, not real numbers.
- **No installs.** Standard library only: `urllib` for HTTP, `json` for bodies,
  `re` for the checklist. `python3` is the only requirement.
- **Tests.** `scripts/run-tests.py` runs the offline suite (`scripts/tests/`),
  where a routing fake replaces the network and asserts the exact payloads
  above.
