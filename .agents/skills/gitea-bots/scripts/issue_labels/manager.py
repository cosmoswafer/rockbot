"""Status-label writes: ``set-status`` and ``--reconcile``, in-process.

Labels are a *set-based projection* of the issue body (and optional workflow
markers). :meth:`StatusManager.reconcile` derives the expected set through the
injected ``derive`` callable and :meth:`StatusManager.apply` merges it, keeping
unrelated labels and stripping every label the label set owns (canonical,
retired and legacy) so issues migrate to the new set.
:meth:`StatusManager.migrate_labels` renames legacy repo labels in place.

The default derive only knows the generic Markdown body shape. A staged
workflow (for example ``gitea-bots``) injects a derive that also inspects its
own comment markers, so this module never depends on workflow details.
"""

from __future__ import annotations

from .gitea import GiteaError
from .labels import (
    STATUS_COLORS,
    STATUS_DESCRIPTIONS,
    STATUS_NAMES,
    derive_status,
    label_status,
    normalize_status_set,
    status_label_name,
)


class LabelError(GiteaError):
    """A status-label operation failed (unknown status or missing label)."""


def _http_5xx(exc: GiteaError) -> bool:
    """True for a server-side failure, which may still have mutated state.

    Some Gitea deployments reject the emoji display names with a 5xx (their
    label store cannot hold the 4-byte characters) yet may apply the write
    with the emoji stripped from the name. Those are recoverable by falling
    back to the bare canonical spelling; 4xx errors are real failures.
    """
    return (getattr(exc, "status", None) or 0) >= 500


def derive_body_status(body, comments, current_labels):
    """Default projection: only the Markdown body shape is inspected."""
    return derive_status(body, current_labels)


class StatusManager:
    def __init__(self, config, client, logger, derive=derive_body_status):
        self.config = config
        self.client = client
        self.log = logger
        self.derive = derive
        self._ctx = None  # cached (map, strip-ids) for a run; labels rarely change
        # A server that answers an emoji display-name write with a 5xx cannot
        # store those names; the first such failure flips this flag and the
        # whole run degrades to the bare canonical spelling.
        self._emoji_ok = True

    def _display(self, status: str) -> str:
        """Target label name: emoji display, or bare once the server rejected it."""
        return status_label_name(status) if self._emoji_ok else status

    def _path(self, suffix: str) -> str:
        return f"/repos/{self.config.repo}{suffix}"

    # ------------------------------------------------------------------ reads
    def fetch_comments(self, num):
        return self.client.get_all(self._path(f"/issues/{num}/comments"))

    def _get_labels(self):
        existing = self.client.get(self._path("/labels"), params={"limit": 100})
        return existing if isinstance(existing, list) else []

    @staticmethod
    def _index(existing):
        return {
            label.get("name"): label
            for label in existing if isinstance(label, dict)
        }

    @staticmethod
    def _match_status(existing, name):
        """The repo label already resolving to this status, if any.

        Prefers the exact bare spelling carrying the canonical metadata, then
        any exact spelling, then any label whose name resolves to the status
        (legacy or emoji-prefixed). The metadata preference keeps a Gitea
        default label that merely shares the canonical name (e.g. the stock
        ``question``) from being hijacked while the real one exists too.
        """
        dicts = [
            label for label in existing
            if isinstance(label, dict) and label.get("id") is not None
        ]
        exact = [c for c in dicts if c.get("name") == name]
        return next(
            (
                c for c in exact
                if c.get("color") == STATUS_COLORS[name]
                and c.get("description") == STATUS_DESCRIPTIONS[name]
            ),
            next(
                (c for c in exact if c.get("name") == name),
                next(
                    (c for c in dicts if label_status(c.get("name")) == name),
                    None,
                ),
            ),
        )

    def _label_context(self):
        """Return ``(status->id, strip-ids)``, creating/renaming canonical labels.

        A repo label is reused when it already resolves to the status, even if
        it still carries the old bare spelling: it is renamed in place, so its
        id and every issue assignment survive the emoji migration. A label whose
        colour or description drifted from the canonical metadata is patched in
        place too, so corrections reach labels that already exist in the repo.

        A server that answers the emoji display-name writes with a 5xx (its
        label store cannot hold 4-byte characters) is degraded to the bare
        canonical spelling instead of failing every status-label write.
        """
        if self._ctx is not None:
            return self._ctx
        existing = self._get_labels()
        by_name = self._index(existing)
        resolved = {}
        for name in STATUS_NAMES:
            display = self._display(name)
            label = by_name.get(display) if self._emoji_ok else None
            if label is None:
                label = self._match_status(existing, name)
            try:
                if label is None:
                    label = self._create_label(name, display, existing, by_name)
                else:
                    label = self._repair_label(label, name, display, existing, by_name)
            except GiteaError as exc:
                if not _http_5xx(exc):
                    raise
                label = self._adopt_bare(name, existing, by_name, cause=exc)
            resolved[name] = label["id"]

        # Strip every label the set owns (canonical + retired + legacy, bare or
        # emoji-prefixed) before merging the derived/wanted set.
        strip_ids = {
            label.get("id") for label in existing
            if isinstance(label, dict)
            and label.get("id") is not None
            and label_status(label.get("name")) is not None
        }
        self._ctx = (resolved, strip_ids)
        return self._ctx

    def _create_label(self, name, target, existing, by_name):
        """POST one canonical label under ``target`` and register it locally."""
        created = self.client.post(
            self._path("/labels"),
            payload={
                "name": target,
                "color": STATUS_COLORS[name],
                "description": STATUS_DESCRIPTIONS[name],
            },
        )
        label_id = created.get("id") if isinstance(created, dict) else None
        if label_id is None:
            raise LabelError(f"could not create label '{target}'")
        self.log.warn(f"set-status: created label '{target}' (id {label_id})")
        label = {
            "id": label_id,
            "name": target,
            "color": STATUS_COLORS[name],
            "description": STATUS_DESCRIPTIONS[name],
        }
        existing.append(label)
        by_name[target] = label
        return label

    def _repair_label(self, label, name, target, existing, by_name):
        """Rename/repair one repo label in place toward the canonical form.

        Renaming in place keeps the label id (and its issue assignments); a
        drifted colour/description is fixed on the same write so corrections
        reach labels that already exist in the repo.
        """
        renamed = label.get("name") != target
        drifted = (
            label.get("color") != STATUS_COLORS[name]
            or label.get("description") != STATUS_DESCRIPTIONS[name]
        )
        if not (renamed or drifted):
            return label
        self.client.patch(
            self._path(f"/labels/{label['id']}"),
            payload={
                "name": target,
                "color": STATUS_COLORS[name],
                "description": STATUS_DESCRIPTIONS[name],
            },
        )
        if renamed:
            self.log.warn(
                f"set-status: renamed label '{label.get('name')}' → '{target}'"
            )
        else:
            self.log.warn(f"set-status: refreshed metadata for label '{target}'")
        by_name.pop(label.get("name"), None)
        updated = dict(
            label,
            name=target,
            color=STATUS_COLORS[name],
            description=STATUS_DESCRIPTIONS[name],
        )
        try:
            existing[existing.index(label)] = updated
        except ValueError:
            existing.append(updated)
        by_name[target] = updated
        return updated

    def _adopt_bare(self, name, existing, by_name, cause):
        """Settle for the bare spelling after the server rejected the emoji name.

        A 5xx label write may still have stored the label with the emoji
        stripped, so re-read the list before creating anything — that keeps a
        half-succeeded create from becoming a duplicate. Then create/repair
        under the bare canonical name, which such servers accept.
        """
        self.log.warn(
            f"set-status: server rejected the emoji label name for '{name}' "
            f"({cause}) — using the bare spelling"
        )
        self._emoji_ok = False
        fresh = self._get_labels()
        existing[:] = fresh
        by_name.clear()
        by_name.update(self._index(fresh))
        label = self._match_status(existing, name)
        if label is None:
            return self._create_label(name, name, existing, by_name)
        return self._repair_label(label, name, name, existing, by_name)

    # ----------------------------------------------------------------- writes
    def apply(self, num, want_raw):
        """Merge the wanted status **label set**; return ``(changed, text)``.

        Idempotent: an unchanged set produces no API write. Unrelated labels are
        preserved; every label the set owns is stripped before the wanted set is
        added, which migrates issues off the old skill-specific names.
        """
        want = normalize_status_set(want_raw)
        if want is None:
            raise LabelError(
                f"unknown status: '{want_raw}' (expected none|seeing|responded|"
                "working|question|committed, or legacy aliases inspect|plan|"
                "implement|questions|done)"
            )
        label_text = ",".join(sorted(want)) or "none"
        name_to_id, strip_ids = self._label_context()
        issue = self.client.get(self._path(f"/issues/{num}"))
        current = [
            label.get("id") for label in (issue.get("labels") or [])
            if isinstance(label, dict) and label.get("id") is not None
        ]
        merged = [label_id for label_id in current if label_id not in strip_ids]
        merged.extend(name_to_id[name] for name in sorted(want))
        if sorted(merged) == sorted(current):
            return False, f"#{num} already '{label_text}'"
        # EditIssueOption.labels is a silent no-op on Gitea; use the dedicated
        # labels endpoint (PUT replaces, so we pass the full merged set).
        self.client.put(
            self._path(f"/issues/{num}/labels"), payload={"labels": merged}
        )
        return True, f"#{num} → {label_text}"

    def reconcile(self, num):
        """Derive the status **set** from the body and repair drift.

        Returns ``(changed, text, expect)`` where ``expect`` is a frozenset of
        canonical labels. Emits an informational NOTE for an unmarked body plan
        (``## Tasks`` with no plan-comment marker).
        """
        issue = self.client.get(self._path(f"/issues/{num}"))
        body = issue.get("body") or ""
        current_labels = [
            label.get("name") for label in (issue.get("labels") or [])
            if isinstance(label, dict)
        ]
        comments = self.fetch_comments(num)
        expect, note = self.derive(body, comments, current_labels)
        if note:
            self.log.warn(
                f"set-status: NOTE #{num} has an unmarked body plan "
                "(## Tasks without a plan-comment marker) — reconciling label "
                "set to responded; post a plan comment marker to make it "
                "canonical"
            )
        changed, text = self.apply(num, expect)
        return changed, text, expect

    # ------------------------------------------------------------- migration
    def migrate_labels(self):
        """One-time, idempotent in-place rename of legacy repo labels.

        Renames the legacy labels via ``PATCH /labels/{id}`` so their ids and
        existing issue assignments survive, then resolves/creates every
        canonical label with its emoji display name (falling back to the bare
        spelling on servers that reject emoji names with a 5xx). A rename is
        skipped when the target canonical label already exists (avoids
        colliding with the retired `done`/`questions` labels). Per-issue
        migration — stripping the legacy labels and adding the derived set —
        is handled by :meth:`reconcile` regardless of whether this command is
        run.

        Returns a short human-readable summary.
        """
        existing = self.client.get(self._path("/labels"), params={"limit": 100})
        existing = existing if isinstance(existing, list) else []
        by_name = {
            label.get("name"): label
            for label in existing if isinstance(label, dict)
        }
        renames = {
            "inspect": "seeing",
            "plan": "responded",
            "implement": "committed",
        }
        changed = []
        for old, new in renames.items():
            src = by_name.get(old)
            display = self._display(new)
            if not src or src.get("id") is None or display in by_name:
                continue
            try:
                self._repair_label(src, new, display, existing, by_name)
            except GiteaError as exc:
                if not _http_5xx(exc):
                    raise
                self._adopt_bare(new, existing, by_name, cause=exc)
            changed.append(f"{old}→{new}")
        # Resolve/rename/create every canonical label (notably the new
        # `question`, and any target that could not be renamed in place).
        self._ctx = None
        self._label_context()
        return ("renamed " + ", ".join(changed)) if changed else "nothing to rename"
