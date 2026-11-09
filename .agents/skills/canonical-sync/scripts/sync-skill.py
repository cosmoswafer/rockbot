#!/usr/bin/env python3
"""sync-skill.py — two-way merge of one skill between the canonical repo and its
consumer copies, so every repo ends up with a byte-identical skill directory.

Default behaviour is a dry run: it plans, checks and reports, and writes nothing.

    sync-skill.py <skill> --from <repo> [--repos a,b] [--apply] [--commit] [--push]
    sync-skill.py <skill> --from <repo> --repos projD --apply   # roll out to a repo lacking it
    sync-skill.py <skill> --from <repo> --json          # machine-readable plan

Exit codes: 0 = ok, 1 = checks failed or a repo was refused, 2 = usage error.
"""
from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

SKILL_GLOBS = (
    ".agents/skills/*/SKILL.md",
    "skills/*/SKILL.md",
    ".opencode/skills/*/SKILL.md",
    ".claude/skills/*/SKILL.md",
)
BOILERPLATE = re.compile(
    r"^(source \.env|REMOTE=|GITEA_REMOTE=|GITEA_BARE=|GITEA_HOST=|GITEA_API=|BARE=|HOST=|REPO=|"
    r"if \[\[|else\b|fi\b|curl -s|  -H \"Authorization|\| jq|```|# \d+\.)"
)

def git(repo: Path, *args: str, check: bool = False) -> str:
    r = subprocess.run(["git", "-C", str(repo), *args], text=True, capture_output=True)
    if check and r.returncode != 0:
        raise RuntimeError((r.stderr or r.stdout).strip() or f"git {' '.join(args)} failed")
    return (r.stdout or "").strip()

def blob(data: bytes) -> str:
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def head_blob(repo: Path, rel: str) -> str:
    """HEAD's blob hash for a path, or "" when the path is untracked/unknown.

    `git rev-parse` echoes its argument on stdout even when it fails, so this
    must branch on the exit code rather than reuse git().
    """
    r = subprocess.run(["git", "-C", str(repo), "rev-parse", f"HEAD:{rel}"],
                       text=True, capture_output=True)
    return r.stdout.strip() if r.returncode == 0 else ""

def find_skill_dir(repo: Path, skill: str) -> Path | None:
    for glob in SKILL_GLOBS:
        cand = repo / glob.replace("*/SKILL.md", skill)
        if (cand / "SKILL.md").is_file():
            return cand
    return None

def upstream_state(repo: Path) -> tuple[int, int]:
    """(ahead, behind) against the tracked branch; (0, 0) when there is none."""
    ref = git(repo, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}")
    if not ref:
        return 0, 0
    counts = git(repo, "rev-list", "--left-right", "--count", f"HEAD...{ref}")
    a, _, b = counts.partition("\t")
    return int(a or 0), int(b or 0)

# ---------- canonical-text checks -------------------------------------------

def check_format(skill_dir: Path) -> list[str]:
    """Frontmatter, fence balance and bash syntax of every copy about to ship."""
    text = (skill_dir / "SKILL.md").read_text(encoding="utf-8")
    problems: list[str] = []
    lines = text.splitlines()

    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        problems.append("frontmatter missing or malformed")
    else:
        fields = dict(
            (kv[0].strip(), kv[1].strip())
            for kv in (ln.split(":", 1) for ln in m.group(1).splitlines() if ":" in ln)
        )
        name = fields.get("name", "")
        if name != skill_dir.name:
            problems.append(f"frontmatter name {name!r} != directory {skill_dir.name!r}")
        desc = fields.get("description", "")
        if not desc or desc in {">-", ">"}:
            problems.append("frontmatter description missing (folded block: check its body)")
        elif len(desc) > 1024:
            problems.append(f"description too long: {len(desc)} chars")
        if name and not re.fullmatch(r"[a-z0-9]+(-[a-z0-9]+)*", name):
            problems.append(f"name not lowercase-hyphen: {name!r}")

    if len(re.findall(r"^```", text, re.M)) % 2:
        problems.append("unbalanced code fences")

    for i, (lang, body) in enumerate(re.findall(r"^```(\w+)\n(.*?)^```", text, re.S | re.M)):
        if lang in {"bash", "sh", "shell"}:
            r = subprocess.run(["bash", "-n"], input=body, text=True, capture_output=True)
            if r.returncode:
                problems.append(f"bash block #{i} does not parse: {r.stderr.strip().splitlines()[:1]}")

    if "\r" in text:
        problems.append("CRLF line endings present")
    if not lines or not text.endswith("\n"):
        problems.append("file must end with a newline")
    return problems

def coverage_gaps(targets: list[tuple[Path, Path, bool]], canonical: Path) -> list[str]:
    """Non-boilerplate lines in a target copy that the canonical text loses."""
    cflat = " ".join((canonical / "SKILL.md").read_text(encoding="utf-8").split())
    gaps: list[str] = []
    for repo, target, _creating in targets:
        if target == canonical:
            continue
        cur = target / "SKILL.md"
        if not cur.is_file():
            continue
        for line in cur.read_text(encoding="utf-8").splitlines():
            s = line.strip()
            if len(s) < 25 or BOILERPLATE.match(s):
                continue
            key = " ".join(s.split())
            if key not in cflat and key.strip("-*` ") not in cflat:
                gaps.append(f"{repo.name}: {s[:100]}")
    seen, uniq = set(), []
    for g in gaps:
        if g not in seen:
            seen.add(g)
            uniq.append(g)
    return uniq

# ---------- apply / commit / push ------------------------------------------

def sync_dir(src: Path, dst: Path, apply: bool) -> list[str]:
    """Relative paths of the files that differ (or were just written)."""
    changes: list[str] = []
    for f in sorted(p for p in src.rglob("*") if p.is_file()):
        rel = f.relative_to(src)
        if "__pycache__" in rel.parts or fnmatch.fnmatch(f.name, "*.pyc"):
            continue
        d = dst / rel
        if not d.is_file() or d.read_bytes() != f.read_bytes():
            changes.append(str(rel))
            if apply:
                d.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(f, d)
    if apply:
        dst.mkdir(parents=True, exist_ok=True)
    return changes

def main() -> int:
    ap = argparse.ArgumentParser(description="two-way sync one skill across repos")
    ap.add_argument("skill")
    ap.add_argument("--root", default=str(Path.home() / "Workspaces"), type=Path)
    ap.add_argument("--from", dest="src_repo", required=True, help="repo holding the canonical text")
    ap.add_argument("--repos", help="comma-separated target repos; naming a repo that lacks the skill"
                                    " rolls it out there (default: every repo that has it)")
    ap.add_argument("--apply", action="store_true", help="write the files")
    ap.add_argument("--commit", action="store_true", help="commit in each changed repo (implies --apply)")
    ap.add_argument("--push", action="store_true", help="push each commit (implies --commit)")
    ap.add_argument("--message", default="docs({skill}): adopt the cross-repo canonical skill text")
    ap.add_argument("--allow-dirty", action="store_true", help="overwrite uncommitted edits to the skill")
    ap.add_argument("--force-behind", action="store_true", help="sync a repo that trails its own remote")
    ap.add_argument("--prune", action="store_true", help="delete target files absent from the canonical dir")
    ap.add_argument("--ignore-coverage", action="store_true", help="ship despite dropped lines")
    ap.add_argument("--fetch", action="store_true", help="refresh remote refs before the ahead/behind check")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    root = args.root.expanduser()
    if not root.is_dir():
        print(f"root not found: {root}", file=sys.stderr)
        return 2
    src_repo = root / args.src_repo
    src_dir = find_skill_dir(src_repo, args.skill)
    if not src_dir:
        print(f"{args.skill} not found in {src_repo}", file=sys.stderr)
        return 2

    wanted = [s.strip() for s in args.repos.split(",")] if args.repos else None
    targets: list[tuple[Path, Path, bool]] = []
    for repo in sorted(p for p in root.iterdir() if (p / ".git").exists()):
        if wanted and repo.name not in wanted and repo.name != src_repo.name:
            continue
        d = find_skill_dir(repo, args.skill)
        if d:
            targets.append((repo, d, False))
        elif wanted and repo.name in wanted:
            # naming a repo that lacks the skill rolls it out there, into that
            # repo's own skills root, or the source's relative location if none
            roots = [c for c in (".agents/skills", ".opencode/skills", "skills", ".claude/skills")
                     if (repo / c).is_dir()]
            candidate = roots[0] if roots else str(src_dir.relative_to(src_repo).parent)
            targets.append((repo, repo / candidate / args.skill, True))

    if wanted:
        absent = [n for n in wanted if n not in {r.name for r, _, _ in targets}
                  and not (root / n / ".git").exists()]
        if absent:
            print(f"no such repo under {root}: {', '.join(absent)}", file=sys.stderr)
            return 2

    report: dict[str, object] = {"skill": args.skill, "source": str(src_dir), "repos": {}, "blocked": []}
    problems = check_format(src_dir)
    report["source_checks"] = problems
    if problems:
        report["blocked"].append("canonical text failed its own format checks")

    gaps = coverage_gaps(targets, src_dir)
    report["coverage_gaps"] = gaps
    if gaps and not args.ignore_coverage:
        report["blocked"].append("canonical text drops lines the targets still carry")

    do_apply = args.apply or args.commit or args.push
    for repo, target, creating in targets:
        rel = str(target.relative_to(repo))
        entry: dict[str, object] = {"path": rel, "actions": []}
        if creating:
            entry["actions"].append("roll-out: this repo does not have the skill yet")
        head = head_blob(repo, f"{rel}/SKILL.md")
        exists = (target / "SKILL.md").is_file()
        work = blob((target / "SKILL.md").read_bytes()) if exists else ""
        canonical = blob((src_dir / "SKILL.md").read_bytes())
        # A roll-out target has nothing of its own to clobber: it is not "dirty",
        # it just does not have the skill yet.
        dirty = not creating and (not exists or head == "" or head != work)
        # Foreign edits are the ones we must not overwrite: uncommitted, and not
        # already the canonical content (our own --apply result is safe to ship).
        would_clobber = dirty and work != canonical
        ahead, behind = upstream_state(repo)
        if args.fetch:
            git(repo, "fetch", "--quiet", "origin")
            ahead, behind = upstream_state(repo)
        entry["ahead"], entry["behind"], entry["dirty"] = ahead, behind, dirty
        allowed = True

        if repo.name == src_repo.name and not creating:
            entry["actions"].append("source (never overwritten by this run)")
            if dirty:
                entry["actions"].append("note: canonical copy is uncommitted here")
        elif would_clobber and not args.allow_dirty:
            entry["actions"].append("REFUSED: uncommitted edits that are not the canonical text"
                                    " (use --allow-dirty)")
            report["blocked"].append(f"{repo.name}: would clobber local edits")
            allowed = False
        elif behind and not args.force_behind:
            entry["actions"].append(f"REFUSED: {behind} commit(s) behind its remote (use --force-behind)")
            report["blocked"].append(f"{repo.name}: behind remote")
            allowed = False
        else:
            changed = sync_dir(src_dir, target, do_apply)
            verb = ("wrote" if do_apply else "would write") + (" (new skill dir)" if creating else "")
            entry["actions"].append(verb + ": " + (", ".join(changed) or "already identical"))
            extra = [
                q for q in sorted(target.rglob("*"))
                if q.is_file() and not (src_dir / q.relative_to(target)).exists()
            ] if exists else []
            if extra:
                entry["actions"].append(f"{len(extra)} extra file(s) in target: "
                                        + ", ".join(str(p.relative_to(target)) for p in extra[:3]))
                if args.prune and do_apply:
                    for p in extra:
                        p.unlink()
                    entry["actions"].append("pruned them")

        if args.commit and allowed:
            pending = git(repo, "status", "--porcelain", "--", rel)
            if not pending:
                entry["actions"].append("commit: nothing changed")
            else:
                git(repo, "add", "--", rel)
                git(repo, "commit", "-q", "-m", args.message.format(skill=args.skill), "--", rel)
                entry["actions"].append("committed " + git(repo, "log", "-1", "--format=%h %s"))
                if args.push:
                    a2, b2 = upstream_state(repo)
                    if b2:
                        entry["actions"].append(f"push skipped: {b2} commit(s) behind")
                        report["blocked"].append(f"{repo.name}: cannot push, behind remote")
                    else:
                        r = subprocess.run(["git", "-C", repo, "push", "origin", "HEAD"],
                                           text=True, capture_output=True)
                        tail = (r.stderr or r.stdout).strip().splitlines()[-1:] or ["ok"]
                        entry["actions"].append(("pushed: " if r.returncode == 0 else "push FAILED: ") + tail[0])
                        if r.returncode:
                            report["blocked"].append(f"{repo.name}: push failed")
        report["repos"][repo.name] = entry

    # final identity check: all distributed copies must be one blob
    if do_apply:
        hashes = {repo.name: blob((target / "SKILL.md").read_bytes())
                  for repo, target, _ in targets if (target / "SKILL.md").is_file()}
        report["blob_hashes"] = hashes
        if len(set(hashes.values())) > 1:
            report["blocked"].append(
                "copies are not byte-identical: " + ", ".join(f"{k}={v[:8]}" for k, v in hashes.items())
            )

    if args.json:
        print(json.dumps(report, indent=2, ensure_ascii=False))
    else:
        print(f"skill: {args.skill}\nsource: {src_dir}  ({'ok' if not problems else 'PROBLEMS'})")
        for p in problems:
            print(f"  format: {p}")
        if gaps:
            print(f"\ncoverage gaps ({len(gaps)} lines absent from the canonical text):")
            for g in gaps[:20]:
                print(f"  {g}")
            if len(gaps) > 20:
                print(f"  … {len(gaps) - 20} more")
        print()
        for name, entry in report["repos"].items():  # type: ignore[union-attr]
            print(f"{name}  [{entry['path']}]  ahead={entry['ahead']} behind={entry['behind']} dirty={entry['dirty']}")
            for a in entry["actions"]:
                print(f"   {a}")
        if report["blocked"]:
            print("\nBLOCKED:")
            for b in report["blocked"]:  # type: ignore[union-attr]
                print(f"  {b}")
            return 1
        print(f"\n{'applied' if do_apply else 'dry run'}: {len(report['repos'])} repo(s) in scope")  # type: ignore[arg-type]
    return 0

if __name__ == "__main__":
    sys.exit(main())
