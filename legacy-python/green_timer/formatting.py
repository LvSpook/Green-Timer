"""Time parsing and display formatting.

The display format is ``[+][HH:]MM:SS[.ff]`` where the fractional part has
``decimals`` digits. Formatting works on integer ticks (1/1000 s) so rounding
never accumulates floating point artefacts such as ``00:59.999``.
"""

from __future__ import annotations

import math
from typing import Tuple

MAX_DECIMALS = 3
_SCALE = 10 ** MAX_DECIMALS


def _ticks(seconds: float) -> int:
    """Convert seconds to integer milliseconds, rounding half away from zero."""
    return int(round(abs(seconds) * _SCALE))


def split_ticks(seconds: float, decimals: int = 2) -> Tuple[int, int, int, int]:
    """Split ``seconds`` into ``(hours, minutes, seconds, fractional)``.

    Rounding is applied at the requested precision, so the returned fractional
    part is an integer in ``[0, 10**decimals)``.
    """
    decimals = max(0, min(MAX_DECIMALS, int(decimals)))
    unit = _SCALE // (10 ** decimals)
    # Round (not truncate) to the display unit so 59.999 shows as 01:00.00.
    total_units = (_ticks(seconds) + unit // 2) // unit
    frac = total_units % (10 ** decimals) if decimals else 0
    total_seconds = total_units // (10 ** decimals)
    minutes, secs = divmod(total_seconds, 60)
    hours, minutes = divmod(minutes, 60)
    return hours, minutes, secs, frac


def format_duration(
    seconds: float,
    decimals: int = 2,
    show_hours: bool = False,
    show_sign: bool = False,
) -> str:
    """Format a duration for display.

    >>> format_duration(65.5, 2)
    '01:05.50'
    >>> format_duration(-5, 0, show_sign=True)
    '-00:05'
    """
    negative = seconds < 0
    hours, minutes, secs, frac = split_ticks(seconds, decimals)

    body = f"{minutes:02d}:{secs:02d}"
    if decimals > 0:
        body += f".{frac:0{decimals}d}"
    if hours > 0 or show_hours:
        body = f"{hours:02d}:" + body

    if negative:
        return "-" + body
    if show_sign:
        return "+" + body
    return body


def parse_duration(text: str) -> float:
    """Parse user input such as ``"90"``, ``"1:30"`` or ``"01:02:03.5"``.

    Accepts ``[[HH:]MM:]SS[.fraction]`` and plain seconds. Raises ``ValueError``
    for unparsable or negative input.
    """
    raw = (text or "").strip()
    if not raw:
        raise ValueError("empty duration")

    if ":" not in raw:
        value = float(raw)
        if math.isnan(value) or math.isinf(value):
            raise ValueError("invalid duration")
        if value < 0:
            raise ValueError("negative duration")
        return value

    parts = raw.split(":")
    if len(parts) > 3 or any(part.strip() == "" for part in parts):
        raise ValueError(f"invalid duration: {text!r}")

    total = 0.0
    for index, part in enumerate(parts):
        value = float(part)
        if value < 0:
            raise ValueError("negative duration")
        remaining = len(parts) - index - 1
        if remaining == 0:
            if value >= 60:
                raise ValueError("seconds must be below 60")
            total += value
        elif remaining == 1:
            if value >= 60:
                raise ValueError("minutes must be below 60")
            total += value * 60
        else:
            total += value * 3600
    return total


def format_input(seconds: float) -> str:
    """Human-friendly editable representation used to prefill fields."""
    seconds = float(seconds)
    if seconds < 60:
        return f"{seconds:g}"
    minutes, secs = divmod(seconds, 60)
    if minutes < 60:
        return f"{int(minutes):02d}:{secs:05.2f}" if secs % 1 else f"{int(minutes):02d}:{int(secs):02d}"
    hours, minutes = divmod(int(minutes), 60)
    return f"{hours:02d}:{minutes:02d}:{int(secs):02d}"
