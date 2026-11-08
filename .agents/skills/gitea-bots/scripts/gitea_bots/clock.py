"""UTC wall-clock helpers. Pure: no I/O beyond reading the system clock."""

from __future__ import annotations

from datetime import datetime, timezone

FORMAT = "%Y-%m-%dT%H:%M:%SZ"
COMPACT_FORMAT = "%Y%m%dT%H%M%SZ"


def now_utc() -> str:
    """Return the current UTC time as ``YYYY-MM-DDTHH:MM:SSZ``."""
    return datetime.now(timezone.utc).strftime(FORMAT)


def compact_utc() -> str:
    """Return the current UTC time compactly (for file-name suffixes)."""
    return datetime.now(timezone.utc).strftime(COMPACT_FORMAT)


def to_epoch(iso: str):
    """Parse an ISO-8601 timestamp to epoch seconds, or ``None`` if invalid."""
    if not iso:
        return None
    try:
        text = iso.strip()
        if text.endswith("Z"):
            text = text[:-1] + "+00:00"
        dt = datetime.fromisoformat(text)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.timestamp()
    except (ValueError, TypeError):
        return None


def shift_iso(iso: str, delta_seconds: int):
    """Return ``iso`` shifted by ``delta_seconds``, or ``None`` if unparseable."""
    epoch = to_epoch(iso)
    if epoch is None:
        return None
    return datetime.fromtimestamp(epoch + delta_seconds, timezone.utc).strftime(FORMAT)


def timestamp_in_window(ts: str, window_start: str, window_epoch: float) -> bool:
    """Whether ``ts`` falls at/after the poll window.

    Mirrors the watcher's contract:

    * no window (empty ``window_start``) -> nothing is in the window;
    * a zero/negative epoch (e.g. the 1970 catch-up fallback) -> everything is;
    * an unparseable timestamp -> treated as in-window (fail open, never lose
      an event on a clock-format surprise).
    """
    if not window_start:
        return False
    if window_epoch <= 0:
        return True
    epoch = to_epoch(ts)
    if epoch is None:
        return True
    return epoch >= window_epoch
