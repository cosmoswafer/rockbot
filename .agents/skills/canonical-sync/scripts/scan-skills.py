#!/usr/bin/env python3
"""scan-skills.py — inventory one skill across every git repo in a workspace and
report drift, so the canonical copy can be found before any merge.

Usage:
    scan-skills.py [--root DIR] [--skill NAME] [--only-drifted] [--json]

Exit codes: 0 = no drift (or nothing scanned), 1 = drift found, 2 = usage error.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

# Skill roots tried inside each repo, in the order agents expect them.
SKILL_GLOBS = (
    ".agents/skills/*/SKILL.md",
    "skills/*/SKILL.md",
    ".opencode/skills/*/SKILL.md",
    ".claude/skills/*/SKILL.md",
)


def git(repo: Path, *args: str) -> str:
    r = subprocess.run(["git", "-C", str(repo), *args], text=True, capture_output=True)
    return r.stdout.strip() if r.returncode == 0 else ""


def blob_hash(repo: Path, rel: str) -> tuple[str, str]:
    """(HEAD blob hash, working-tree hash) — equal when the file is committed clean."""
    head = git(repo, "rev-parse", f"HEAD:{rel}")
    try:
        work = hashlib.sha1(
            b"blob %d\0" % len((repo / rel).read_bytes()) + (repo / rel).read_bytes()
        ).hexdigest()
    except OSError:
        work = ""
    return head, work


def drifted(copies: list[dict]) -> bool:
    """True when committed copies differ, or the contents differ once uncommitted
    and untracked copies are counted."""
    return len(committed_variants(copies)) > 1 or len({c["work_blob"] for c in copies}) > 1


def committed_variants(copies: list[dict]) -> set[str]:
    """Distinct committed contents. An untracked copy has no HEAD blob, so it must
    not be counted as its own committed variant — it only shows up in the
    working-tree count."""
    return {c["head_blob"] for c in copies if c["head_blob"]}


def scan(root: Path) -> dict[str, list[dict]]:
    skills: dict[str, list[dict]] = {}
    for repo in sorted(p for p in root.iterdir() if (p / ".git").exists()):
        for pattern in SKILL_GLOBS:
            for match in sorted(repo.glob(pattern)):
                rel = str(match.relative_to(repo))
                name = match.parent.name
                head, work = blob_hash(repo, rel)
                commit = git(repo, "log", "-1", "--format=%ad|%h|%s", "--date=short", "--", rel)
                parts = commit.split("|", 2) if commit else ["", "", ""]
                cdate, chash = parts[0], parts[1]
                cmsg = parts[2] if len(parts) > 2 else ""
                skills.setdefault(name, []).append(
                    {
                        "repo": repo.name,
                        "path": rel,
                        "root": str(match.parent.parent.relative_to(repo)),
                        "bytes": match.stat().st_size,
                        "head_blob": head,
                        "work_blob": work,
                        "clean": head == work and head != "",
                        "mtime": match.stat().st_mtime,
                        "commit_date": cdate,
                        "commit": chash,
                        "subject": cmsg,
                    }
                )
    return skills


def main() -> int:
    ap = argparse.ArgumentParser(description="inventory a skill across workspace repos")
    ap.add_argument("--root", default=str(Path.home() / "Workspaces"), type=Path)
    ap.add_argument("--skill", help="only this skill name")
    ap.add_argument("--only-drifted", action="store_true", help="print skills whose copies differ")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    if not args.root.is_dir():
        print(f"root not found: {args.root}", file=sys.stderr)
        return 2

    skills = scan(args.root)
    if args.skill:
        skills = {k: v for k, v in skills.items() if k == args.skill}
        if not skills:
            print(f"skill not found in any repo: {args.skill}", file=sys.stderr)
            return 2
    if args.only_drifted:
        skills = {k: v for k, v in skills.items() if drifted(v)}

    if args.json:
        print(json.dumps(skills, indent=2, ensure_ascii=False))
    else:
        for name, copies in sorted(skills.items()):
            committed = len(committed_variants(copies))
            actual = len({c["work_blob"] for c in copies})
            note = "" if committed == actual else f"  ({actual} including uncommitted edits)"
            print(f"\n{name} — {len(copies)} copies, {committed} variant(s)"
                  + ("   DRIFT" if committed > 1 or actual > 1 else "   unified") + note)
            for c in sorted(copies, key=lambda x: x["commit_date"], reverse=True):
                flag = "clean" if c["clean"] else "DIRTY"
                print(f"  {c['repo']:<28} {c['commit_date']} {c['bytes']:>6}B "
                      f"{c['head_blob'][:8]} {flag:<5} {c['path']}")
                if c["subject"]:
                    print(f"  {'':<28} {c['commit']} {c['subject'][:74]}")
    return 1 if any(drifted(v) for v in skills.values()) else 0


if __name__ == "__main__":
    sys.exit(main())
