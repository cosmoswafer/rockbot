"""Output helpers: aligned tables, column clipping, and JSON mode.

Alignment uses *display* width, because status labels start with an emoji and
titles mix Chinese and Latin text — counting characters would skew every
column of a real board.
"""

from __future__ import annotations

import json
import sys
import unicodedata

DEFAULT_TITLE_WIDTH = 72
ELLIPSIS = "…"


def char_width(char: str) -> int:
    if unicodedata.combining(char):
        return 0
    if unicodedata.east_asian_width(char) in ("W", "F"):
        return 2
    # Emoji that report "ambiguous" (registered symbols) still render 2 wide.
    return 2 if ord(char) >= 0x1F300 else 1


def display_width(text) -> int:
    return sum(char_width(char) for char in str(text))


def clip(text, limit: int = DEFAULT_TITLE_WIDTH) -> str:
    """Cut to ``limit`` display columns, marking the truncation."""
    text = str(text or "")
    if limit <= 0 or display_width(text) <= limit:
        return text
    out = ""
    used = 0
    for char in text:
        width = char_width(char)
        if used + width > limit - 1:
            break
        out += char
        used += width
    return out + ELLIPSIS


def pad(text, width: int, align: str = "left") -> str:
    text = str(text or "")
    gap = max(0, width - display_width(text))
    return text if align == "left" else " " * gap + text


def table(rows, headers=(), indent: str = "") -> str:
    """Render a list of equal-length row sequences as an aligned text table."""
    rows = [[str(cell or "") for cell in row] for row in rows]
    headers = [str(header or "") for header in headers]
    if not rows and not headers:
        return f"{indent}(nothing)"
    columns = max([len(header or "") for header in headers]
                  + [len(row) for row in rows] or [0])
    widths: list[int] = []
    for index in range(columns):
        cells = [headers[index]] if len(headers) > index else []
        cells += [row[index] for row in rows if len(row) > index]
        widths.append(max([display_width(cell) for cell in cells] or [0]))

    lines = []
    if headers:
        lines.append(indent + "  ".join(
            pad(header, widths[index], "left" if index else "right")
            for index, header in enumerate(headers)
        ).rstrip())
    for row in rows:
        lines.append(indent + "  ".join(
            pad(cell, widths[index], "left" if index else "right")
            for index, cell in enumerate(row)
        ).rstrip())
    return "\n".join(lines)


def json_dump(payload) -> str:
    return json.dumps(payload, indent=2, ensure_ascii=False, sort_keys=False)


class Printer:
    """stdout carries the result; stderr carries advice.

    Keeping the two apart means ``--json`` output pipes straight into ``jq``,
    and ``--dry-run`` plans never mix with the payload.
    """

    def __init__(self, as_json: bool = False, quiet: bool = False,
                 stream=None, err=None):
        self.as_json = as_json
        self.quiet = quiet
        self.stream = stream or sys.stdout
        self.err = err or sys.stderr

    def emit(self, text: str) -> None:
        """Human-readable output — silent in JSON mode and under ``--quiet``."""
        if self.as_json or self.quiet or not text:
            return
        print(text, file=self.stream)

    def emit_json(self, payload) -> None:
        if not self.as_json:
            return
        print(json_dump(payload), file=self.stream)

    def emit_either(self, payload, text: str) -> None:
        """Exactly one of the two: ``payload`` as JSON, or ``text`` for people."""
        if self.as_json:
            print(json_dump(payload), file=self.stream)
        elif text and not self.quiet:
            print(text, file=self.stream)

    def note(self, text: str) -> None:
        """A hint or follow-up, always on stderr unless ``--quiet``."""
        if not self.quiet and text:
            print(text, file=self.err)


def error_stream(text: str) -> None:
    print(text, file=sys.stderr)
