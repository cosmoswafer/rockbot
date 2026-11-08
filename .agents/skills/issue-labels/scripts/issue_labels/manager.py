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

    def _path(self, suffix: str) -> str:
        return f"/repos/{self.config.repo}{suffix}"

    # ------------------------------------------------------------------ reads
    def fetch_comments(self, num):
        return self.client.get_all(self._path(f"/issues/{num}/comments"))

    def _label_context(self):
        """Return ``(status->id, strip-ids)``, creating/renaming canonical labels.

        A repo label is reused when it already resolves to the status, even if
        it still carries the old bare spelling: it is renamed in place, so its
        id and every issue assignment survive the emoji migration. A label whose
        colour or description drifted from the canonical metadata is patched in
        place too, so corrections reach labels that already exist in the repo.
        """
        if self._ctx is not None:
            return self._ctx
        existing = self.client.get(self._path("/labels"), params={"limit": 100})
        existing = existing if isinstance(existing, list) else []
        by_name = {
            label.get("name"): label
            for label in existing if isinstance(label, dict)
        }
        resolved = {}
        for name in STATUS_NAMES:
            display = status_label_name(name)
            label = by_name.get(display)
            if label is None:
                label = next(
                    (
                        candidate for candidate in existing
                        if isinstance(candidate, dict)
                        and label_status(candidate.get("name")) == name
                    ),
                    None,
                )
            if label is None:
                created = self.client.post(
                    self._path("/labels"),
                    payload={
                        "name": display,
                        "color": STATUS_COLORS[name],
                        "description": STATUS_DESCRIPTIONS[name],
                    },
                )
                label_id = created.get("id") if isinstance(created, dict) else None
                if label_id is None:
                    raise LabelError(f"could not create label '{display}'")
                self.log.warn(f"set-status: created label '{display}' (id {label_id})")
                label = {"id": label_id, "name": display}
                existing.append(label)
                by_name[display] = label
            else:
                # Rename in place so the label id (and its assignments) survive,
                # and repair drifted colour/description so an existing repo label
                # picks up the canonical metadata on the next run.
                renamed = label.get("name") != display
                drifted = (
                    label.get("color") != STATUS_COLORS[name]
                    or label.get("description") != STATUS_DESCRIPTIONS[name]
                )
                if renamed or drifted:
                    self.client.patch(
                        self._path(f"/labels/{label['id']}"),
                        payload={
                            "name": display,
                            "color": STATUS_COLORS[name],
                            "description": STATUS_DESCRIPTIONS[name],
                        },
                    )
                    if renamed:
                        self.log.warn(
                            f"set-status: renamed label '{label.get('name')}' "
                            f"→ '{display}'"
                        )
                    else:
                        self.log.warn(
                            f"set-status: refreshed metadata for label '{display}'"
                        )
                    by_name.pop(label.get("name"), None)
                    label = dict(
                        label,
                        name=display,
                        color=STATUS_COLORS[name],
                        description=STATUS_DESCRIPTIONS[name],
                    )
                    by_name[display] = label
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
        canonical label with its emoji display name. A rename is skipped when
        the target canonical label already exists (avoids colliding with the
        retired `done`/`questions` labels). Per-issue migration — stripping the
        legacy labels and adding the derived set — is handled by
        :meth:`reconcile` regardless of whether this command is run.

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
            display = status_label_name(new)
            if not src or src.get("id") is None or display in by_name:
                continue
            self.client.patch(
                self._path(f"/labels/{src['id']}"),
                payload={
                    "name": display,
                    "color": STATUS_COLORS[new],
                    "description": STATUS_DESCRIPTIONS[new],
                },
            )
            by_name.pop(old, None)
            by_name[display] = dict(src, name=display)
            changed.append(f"{old}→{new}")
        # Resolve/rename/create every canonical label (notably the new
        # `question`, and any target that could not be renamed in place).
        self._ctx = None
        self._label_context()
        return ("renamed " + ", ".join(changed)) if changed else "nothing to rename"
