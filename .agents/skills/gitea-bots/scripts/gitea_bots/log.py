"""Small structured logger, injectable so components stay decoupled from stdio.

Output format matches the bash watcher: ``<UTC> <msg>`` on stdout,
``<UTC> WARN <msg>`` on stderr. Event lines are printed verbatim.
"""

from __future__ import annotations

import sys

from .clock import now_utc
from .errors import BotFatal


class Logger:
    def __init__(self, verbose: bool = False, out=None, err=None):
        self.verbose_enabled = verbose
        self.out = out if out is not None else sys.stdout
        self.err = err if err is not None else sys.stderr

    @staticmethod
    def _print(stream, text: str) -> None:
        print(text, file=stream, flush=True)

    def log(self, msg: str) -> None:
        self._print(self.out, f"{now_utc()} {msg}")

    def warn(self, msg: str) -> None:
        self._print(self.err, f"{now_utc()} WARN {msg}")

    def verbose(self, msg: str) -> None:
        if self.verbose_enabled:
            self.log(msg)

    def event(self, line: str) -> None:
        """Print a raw event/inbox line verbatim."""
        self._print(self.out, line)

    def die(self, msg: str):
        """Log a fatal error and raise :class:`BotFatal` (message printed once)."""
        self.warn(msg)
        raise BotFatal(msg)
