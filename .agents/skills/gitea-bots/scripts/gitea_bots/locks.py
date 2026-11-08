"""Advisory file locks via :mod:`fcntl` (standard library, POSIX).

Python's file descriptors are non-inheritable by default (PEP 446) and the
poll lock is always released *before* notification children are spawned, so a
long-lived ``opencode`` session can never inherit and starve the poll lock.
"""

from __future__ import annotations

import fcntl
from pathlib import Path


class LockBusy(Exception):
    """Raised when a context-manager lock is already held."""


class FileLock:
    def __init__(self, path):
        self.path = Path(path)
        self._fh = None

    def acquire(self, nonblocking: bool = True) -> bool:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._fh = open(self.path, "a+")
        flags = fcntl.LOCK_EX | (fcntl.LOCK_NB if nonblocking else 0)
        try:
            fcntl.flock(self._fh.fileno(), flags)
        except OSError:
            self._fh.close()
            self._fh = None
            return False
        return True

    def release(self) -> None:
        if self._fh is None:
            return
        try:
            fcntl.flock(self._fh.fileno(), fcntl.LOCK_UN)
        finally:
            self._fh.close()
            self._fh = None

    def __enter__(self):
        if not self.acquire():
            raise LockBusy(str(self.path))
        return self

    def __exit__(self, *exc):
        self.release()
