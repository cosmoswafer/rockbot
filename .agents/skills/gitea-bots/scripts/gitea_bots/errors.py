"""Shared exception types.

``GiteaError`` is re-exported from the vendored ``issue_labels`` package so the label
CLI and the bot raise the same type. ``BotFatal`` stays here: it is specific to
the watcher CLI's fatal-configuration handling.
"""

from issue_labels.gitea import GiteaError

__all__ = ["BotFatal", "GiteaError"]


class BotFatal(Exception):
    """A fatal, user-facing configuration or usage error.

    Raised by :meth:`gitea_bots.log.Logger.die`, which has already printed the
    message; :mod:`gitea_bots.cli` only maps it to exit status 1.
    """
