"""Green Timer - a lightweight, always-on-top timer overlay.

This package contains the application core (configuration, timing maths,
timer engine, OS integration and update checking) and the Qt user interface.

The core modules deliberately avoid importing Qt so they can be unit tested
without a display server.
"""

from __future__ import annotations

import os
import sys

APP_NAME = "Green Timer"
APP_VERSION = "v1.2.0"
GITHUB_REPO = "LvSpook/Green-Timer"

# The legacy file name is kept so existing user settings keep working.
CONFIG_FILENAME = "timer_pro_settings.json"
ICON_FILENAME = "green_timer.ico"

__all__ = [
    "APP_NAME",
    "APP_VERSION",
    "CONFIG_FILENAME",
    "GITHUB_REPO",
    "ICON_FILENAME",
    "resource_path",
]


def resource_path(relative_path: str) -> str:
    """Return an absolute path to a bundled resource.

    Works both from a source checkout and from a PyInstaller bundle. When
    frozen, resources live in ``sys._MEIPASS``; otherwise they are looked up
    next to the entry point first and then in this package's parent folder.
    """
    base = getattr(sys, "_MEIPASS", None)
    if base is None:
        entry = os.path.abspath(sys.argv[0]) if sys.argv and sys.argv[0] else __file__
        base = os.path.dirname(entry)
        if not os.path.exists(os.path.join(base, relative_path)):
            base = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    return os.path.join(base, relative_path)
