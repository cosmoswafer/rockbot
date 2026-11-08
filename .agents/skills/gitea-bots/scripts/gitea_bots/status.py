"""Status-label writes for the staged workflow.

All label mechanics (vocabulary, label creation/renaming, set merging, legacy
migration) live in the shared ``issue_labels`` package. This module only injects
the workflow-specific projection: stage markers from the comment list become
booleans passed to the marker-agnostic ``derive_status``.
"""

from __future__ import annotations

from issue_labels.manager import LabelError
from issue_labels.manager import StatusManager as _SharedStatusManager

from .rules import derive_status as _workflow_derive

__all__ = ["LabelError", "StatusManager"]


class StatusManager(_SharedStatusManager):
    """Shared manager with the stage-marker-aware derive injected."""

    def __init__(self, config, client, logger):
        super().__init__(config, client, logger, derive=_workflow_derive)
