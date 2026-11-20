"""Font helpers: enumerate usable families and pick a sensible fallback."""

from __future__ import annotations

from typing import List

from PyQt6.QtGui import QFont, QFontDatabase

#: Ordered fallbacks chosen because they exist on virtually every system and
#: look good as a monospaced-ish timer face.
FALLBACKS = ("Segoe UI", "Consolas", "Cascadia Mono", "Arial", "DejaVu Sans")


def available_families() -> List[str]:
    families = QFontDatabase.families()
    return sorted({name for name in families if name}, key=str.lower)


def resolve_family(requested: str) -> str:
    """Return ``requested`` if installed, otherwise the best available fallback."""
    families = set(QFontDatabase.families())
    if requested in families:
        return requested
    for candidate in FALLBACKS:
        if candidate in families:
            return candidate
    return QFont().family()
