"""Public surface of the vendored issue-label package."""

from .gitea import GiteaClient, GiteaError, GiteaSettings, derive_remote, load_dotenv
from .labels import (
    LEGACY_NAMES,
    RETIRED_NAMES,
    STATUS_ALIASES,
    STATUS_COLORS,
    STATUS_DESCRIPTIONS,
    STATUS_EMOJI,
    STATUS_LABEL_NAMES,
    STATUS_NAMES,
    derive_status,
    has_summary,
    has_tasks,
    label_status,
    normalize_status,
    normalize_status_set,
    status_label_name,
)
from .manager import LabelError, StatusManager, derive_body_status

__version__ = "1.0.0"

__all__ = [
    "GiteaClient",
    "GiteaError",
    "GiteaSettings",
    "LabelError",
    "StatusManager",
    "derive_body_status",
    "derive_remote",
    "derive_status",
    "has_summary",
    "has_tasks",
    "label_status",
    "LEGACY_NAMES",
    "load_dotenv",
    "normalize_status",
    "normalize_status_set",
    "RETIRED_NAMES",
    "STATUS_ALIASES",
    "STATUS_COLORS",
    "STATUS_DESCRIPTIONS",
    "STATUS_EMOJI",
    "STATUS_LABEL_NAMES",
    "STATUS_NAMES",
    "status_label_name",
]
