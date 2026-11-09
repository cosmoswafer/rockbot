#!/usr/bin/env python3
"""Self-contained test suite for the canonical-sync scripts.

Builds a throwaway multi-repo git sandbox (each repo with its own bare remote),
then exercises scan-skills.py and sync-skill.py against it: drift detection,
coverage gaps, refusal of dirty/behind repos, whole-directory apply, commit
scope, push, blob identity and the frontmatter/format checks.

    python3 tests/sync-test.py [--root SANDBOX_PARENT] [--keep]

No network access, no repository outside the sandbox is touched. Requires git
and Python 3.10+.
"""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE.parent / "scripts"

BODY = """---
name: demo
description: Demo skill for canonical-sync integration testing. Use when testing skill sync.
license: CC0-1.0
---

# Demo

Rule one: always dry run first.

Rule two: never overwrite uncommitted work.
"""
RULE3 = "\nRule three: verify blob hashes after apply.\n"
FAILS: list[str] = []
ROOT = Path()


def sh(*args, cwd=None, check=True):
    r = subprocess.run(list(args), cwd=str(cwd or ROOT), text=True, capture_output=True)
    if check and r.returncode:
        raise SystemExit(f"{args} failed: {(r.stderr or r.stdout).strip()}")
    return r.returncode, r.stdout, r.stderr


def soft(*args, cwd=None):
    """Run without the fail-fast check, returning (rc, stdout, stderr)."""
    r = subprocess.run(list(args), cwd=str(cwd or ROOT), text=True, capture_output=True)
    return r.returncode, r.stdout, r.stderr


def scan(*args):
    return soft("python3", str(SCRIPTS / "scan-skills.py"), "--root", str(ROOT), *args)


def sync(*args):
    return soft("python3", str(SCRIPTS / "sync-skill.py"), "--root", str(ROOT), *args)


def head_blob(repo, rel="skills/demo/SKILL.md"):
    return soft("git", "rev-parse", f"HEAD:{rel}", cwd=ROOT / repo)[1].strip()


def file_bytes(repo, rel="skills/demo/SKILL.md"):
    return (ROOT / repo / rel).read_bytes()


def check(name, cond, detail=""):
    print(f"  {'PASS' if cond else 'FAIL'}  {name}" + (f"\n        {detail}" if not cond and detail else ""))
    if not cond:
        FAILS.append(name)


def build_sandbox():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    for name in ("canon", "projA", "projB", "projC", "projD"):
        repo = ROOT / name
        if name == "projD":
            # a repo that does not carry this skill at all: the roll-out target
            (repo / ".agents" / "skills" / "other").mkdir(parents=True)
            (repo / ".agents" / "skills" / "other" / "SKILL.md").write_text(
                "---\nname: other\ndescription: Another skill.\n---\n\n# other\n")
        else:
            (repo / "skills" / "demo" / "scripts").mkdir(parents=True)
            (repo / "skills" / "demo" / "scripts" / "tool.sh").write_text("# a shared helper\n")
            (repo / "skills" / "demo" / "SKILL.md").write_text(BODY + (RULE3 if name == "projA" else ""))
        sh("git", "init", "-q", ".", cwd=repo)
        sh("git", "config", "user.email", "t@example.com", cwd=repo)
        sh("git", "config", "user.name", "Tester", cwd=repo)
        sh("git", "add", "-A", cwd=repo)
        sh("git", "commit", "-qm", f"init {name}", cwd=repo)
        sh("git", "branch", "-qM", "main", cwd=repo)
        remote = ROOT / f"{name}-remote.git"
        sh("git", "init", "-q", "--bare", str(remote))
        sh("git", "symbolic-ref", "HEAD", "refs/heads/main", cwd=remote)
        sh("git", "remote", "add", "origin", f"../{name}-remote.git", cwd=repo)
        sh("git", "push", "-q", "-u", "origin", "main", cwd=repo)

    # projC: uncommitted local edit -> refused
    c = ROOT / "projC" / "skills" / "demo" / "SKILL.md"
    c.write_text(c.read_text() + "\nLocal work in progress.\n")

    # projB: its remote moved ahead with an edit to the same file -> behind, refused
    b = ROOT / "projB" / "skills" / "demo" / "SKILL.md"
    b.write_text(b.read_text() + "\nUpstream-only change.\n")
    sh("git", "commit", "-qam", "upstream edit", cwd=ROOT / "projB")
    sh("git", "push", "-q", "origin", "main", cwd=ROOT / "projB")
    sh("git", "reset", "--hard", "-q", "HEAD~1", cwd=ROOT / "projB")


def reach_unified_state():
    """Rebuild, fold projA's rule into the canonical text, get every repo
    committed and in sync with its remote, then sync for real."""
    build_sandbox()
    promote_canon_rule3()
    sh("git", "add", "-A", cwd=ROOT / "canon")
    sh("git", "commit", "-qm", "canon gains rule three", cwd=ROOT / "canon")
    sh("git", "commit", "-qam", "projC commits its work", cwd=ROOT / "projC")
    sh("git", "pull", "-q", "--ff-only", "origin", "main", cwd=ROOT / "projB")
    return sync("demo", "--from", "canon", "--commit", "--push")


def promote_canon_rule3():
    """Fold projA's extra knowledge into the canonical text, as the workflow says."""
    skill = ROOT / "canon" / "skills" / "demo" / "SKILL.md"
    skill.write_text(skill.read_text() + RULE3)


def run_suite():
    print("1. scan reports drift across the copies")
    rc, out, _ = scan("--skill", "demo")
    check("exit code 1 (drift)", rc == 1, str(rc))
    check("4 copies, 2 committed variants, 3 counting the uncommitted edit",
          "4 copies" in out and "2 variant(s)" in out and "3 including uncommitted edits" in out,
          out.splitlines()[0])
    check("projC flagged DIRTY", "DIRTY" in out)
    js = json.loads(scan("--skill", "demo", "--json")[1])
    check("clean flag false only for projC",
          {c["repo"]: c["clean"] for c in js["demo"]} ==
          {"canon": True, "projA": True, "projB": True, "projC": False})

    print("\n2. dry run refuses dirty/behind targets and reports the knowledge gap")
    rc, out, _ = sync("demo", "--from", "canon")
    check("blocked exit code 1", rc == 1, str(rc))
    check("projA gap reported", "Rule three: verify blob hashes after apply" in out, out)
    check("projB refused as behind", "REFUSED: 1 commit(s) behind" in out)
    check("projC refused as clobber risk", "REFUSED: uncommitted edits" in out)
    check("canon reported as source", "source (never overwritten by this run)" in out)
    check("dry run wrote nothing", file_bytes("projA") != file_bytes("canon"))

    print("\n3. after the canonical text absorbs projA's rule, apply updates it")
    promote_canon_rule3()
    rc, out, _ = sync("demo", "--from", "canon", "--apply", "--repos", "projA")
    check("projA working tree now matches", file_bytes("projA") == file_bytes("canon"))
    check("projA not committed yet", head_blob("projA") != head_blob("canon"))
    check("no coverage gaps reported", "coverage gaps" not in out, out)

    print("\n4. commit and push touch only the skill directory")
    (ROOT / "projA" / "notes.txt").write_text("unrelated local file\n")
    rc, out, _ = sync("demo", "--from", "canon", "--commit", "--push",
                      "--message", "docs({skill}): sync canonical text")
    check("projA committed", "committed" in out and "docs(demo): sync canonical text" in out, out)
    check("projC refused, its own commit untouched",
          "REFUSED" in out and sh("git", "log", "-1", "--format=%s", cwd=ROOT / "projC")[1].strip() == "init projC",
          out)
    files = sh("git", "show", "--name-only", "--format=", "HEAD", cwd=ROOT / "projA")[1].split()
    check("commit contains only skill paths", files and all(f.startswith("skills/demo/") for f in files), str(files))
    check("projA nothing ahead of remote",
          sh("git", "rev-list", "--count", "origin/main..HEAD", cwd=ROOT / "projA")[1].strip() == "0")
    remote_bytes = sh("git", "show", "main:skills/demo/SKILL.md", cwd=ROOT / "projA-remote.git")[1].encode()
    check("remote holds the canonical content", remote_bytes == file_bytes("canon"))
    check("unrelated file left untracked", (ROOT / "projA" / "notes.txt").exists())

    print("\n5. whole-directory sync carries scripts too")
    (ROOT / "canon" / "skills" / "demo" / "scripts" / "tool.sh").write_text("# a shared helper v2\n")
    (ROOT / "canon" / "skills" / "demo" / "scripts" / "new.py").write_text("print('new')\n")
    rc, out, _ = sync("demo", "--from", "canon", "--apply", "--repos", "projA")
    check("changed helper detected", "scripts/tool.sh" in out, out)
    check("new file added to target", (ROOT / "projA" / "skills" / "demo" / "scripts" / "new.py").is_file())

    print("\n6. extra files in a target are reported, then pruned on request")
    (ROOT / "projA" / "skills" / "demo" / "stale.md").write_text("leftover\n")
    rc, out, _ = sync("demo", "--from", "canon", "--repos", "projA")
    check("extra file reported", "extra file(s) in target" in out, out)
    rc, out, _ = sync("demo", "--from", "canon", "--repos", "projA", "--apply", "--prune")
    check("--prune removes it", not (ROOT / "projA" / "skills" / "demo" / "stale.md").exists())

    print("\n7. full sync of clean repos yields one blob everywhere")
    rc, out, err = reach_unified_state()
    blobs = {r: head_blob(r) for r in ("canon", "projA", "projB", "projC")}
    check("all four share one blob", len(set(blobs.values())) == 1, str(blobs))
    check("scan now reports unified", "unified" in scan("--skill", "demo")[1])
    check("sync exit 0", rc == 0, out + err)

    print("\n8. --json plan shape")
    rc, out, _ = sync("demo", "--from", "canon", "--json")
    data = json.loads(out)
    check("top-level keys",
          {"skill", "source", "repos", "blocked", "source_checks", "coverage_gaps"} <= set(data))
    check("per-repo keys",
          all({"ahead", "behind", "dirty", "actions"} <= set(v) for v in data["repos"].values()))

    print("\n9. format checks reject a broken canonical text")
    skill = ROOT / "canon" / "skills" / "demo" / "SKILL.md"
    skill.write_text("---\nname: wrong\ndescription: x\n---\n\n# demo\n\n```bash\nif true\n```\n")
    rc, out, _ = sync("demo", "--from", "canon")
    check("name/dir mismatch flagged", "canonical text failed its own format checks" in out, out)
    check("bad bash flagged", "does not parse" in out)
    skill.write_text("---\nname: demo\ndescription: Demo skill for testing.\n---\n\n# demo\n\n```bash\nls\n")
    rc, out, _ = sync("demo", "--from", "canon")
    check("unbalanced fence flagged", "unbalanced code fences" in out, out)

    print("\n10. roll-out into a repo that has no copy of the skill yet")
    build_sandbox()
    promote_canon_rule3()
    rc, out, _ = sync("demo", "--from", "canon", "--repos", "projD")
    check("dry run plans a new skill dir", "would write (new skill dir)" in out, out)
    check("no clobber refusal for a roll-out", "REFUSED" not in out.split("projD")[1], out)
    rc, out, _ = sync("demo", "--from", "canon", "--repos", "projD", "--apply", "--commit", "--push")
    rolled = ROOT / "projD" / ".agents" / "skills" / "demo"
    check("created under the repo's own skills root", (rolled / "SKILL.md").is_file())
    check("whole dir carried", (rolled / "scripts" / "tool.sh").is_file())
    check("rolled-out content is canonical",
          (rolled / "SKILL.md").read_bytes() == file_bytes("canon"))
    check("committed in projD", "committed" in out and
          sh("git", "log", "-1", "--format=%s", cwd=ROOT / "projD")[1].strip().startswith("docs(demo)"), out)
    check("projD nothing ahead of remote",
          sh("git", "rev-list", "--count", "origin/main..HEAD", cwd=ROOT / "projD")[1].strip() == "0")

    print("\n11. an untracked copy counts as content, not as a committed variant")
    reach_unified_state()
    (ROOT / "projE").mkdir(parents=True)
    sh("git", "init", "-q", ".", cwd=ROOT / "projE")
    e = ROOT / "projE" / ".agents" / "skills" / "demo"
    e.mkdir(parents=True)
    (e / "SKILL.md").write_text(BODY + RULE3 + "\nNever committed copy.\n")
    rc, out, _ = scan("--skill", "demo")
    check("committed variants not inflated by the untracked copy",
          "1 variant(s)" in out and "2 including uncommitted edits" in out, out.splitlines()[0])
    check("still reported as DRIFT", "DRIFT" in out)

    print("\n12. usage errors")
    rc, out, err = sync("nope", "--from", "canon")
    check("missing skill => 2", rc == 2 and "not found" in err)
    rc, out, err = sync("demo", "--from", "canon", "--repos", "ghostrepo")
    check("unknown target repo => 2", rc == 2 and "no such repo" in err, err)


def main() -> int:
    global ROOT
    ap = argparse.ArgumentParser(description="canonical-sync test suite")
    ap.add_argument("--root", type=Path, default=Path("/tmp/opencode/canonical-sync-sandbox"),
                    help="where to build the sandbox; use a repo-local tmp dir where /tmp is banned")
    ap.add_argument("--keep", action="store_true", help="leave the sandbox in place afterwards")
    args = ap.parse_args()
    ROOT = args.root.expanduser().resolve()
    for tool in ("git",):
        if shutil.which(tool) is None:
            raise SystemExit(f"{tool} is required")
    try:
        build_sandbox()
        run_suite()
    finally:
        if not args.keep and ROOT.exists():
            shutil.rmtree(ROOT, ignore_errors=True)
    print(f"\n{'ALL PASS' if not FAILS else 'FAILURES: ' + ', '.join(FAILS)}")
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
