"""gitea-issues helper CLI — a dependency-free Gitea issue toolkit.

Modules, in dependency order (each one only looks inward):

- :mod:`env`      — where the API base, the ``owner/repo`` and the token come from
- :mod:`client`   — the only code that touches the network
- :mod:`template` — the standard issue body and its validation rules
- :mod:`tasks`    — Markdown checklist editing inside an issue body
- :mod:`labels`   — repo labels, issue labels and the shared status vocabulary
- :mod:`milestones` — milestone lookup, creation and assignment
- :mod:`issues`   — issue payloads and the human-readable views
- :mod:`render`   — table/JSON output helpers
- :mod:`cli`      — argument parsing and composition root

Only the Python standard library is used, so the scripts run anywhere
``python3`` does — no ``pip``, no ``requests``, no ``jq``.
"""

__all__ = ["cli", "client", "env", "issues", "labels", "milestones", "render",
           "tasks", "template"]
