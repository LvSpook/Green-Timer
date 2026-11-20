"""Qt user-interface package."""

from __future__ import annotations

__all__ = ["TimerWindow", "SettingsDialog"]


def __getattr__(name):
    # Imported lazily so the package can be imported without Qt present.
    if name == "TimerWindow":
        from .window import TimerWindow

        return TimerWindow
    if name == "SettingsDialog":
        from .settings import SettingsDialog

        return SettingsDialog
    raise AttributeError(name)
