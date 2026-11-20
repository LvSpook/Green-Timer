"""Thin, defensive wrappers around OS-specific behaviour.

Every function degrades gracefully when the feature is unavailable so the app
runs on non-Windows systems (and in tests) without special-casing everywhere.
"""

from __future__ import annotations

import ctypes
import logging
import os
import platform
import sys

log = logging.getLogger(__name__)

IS_WINDOWS = sys.platform == "win32"


class ACCENTPOLICY(ctypes.Structure):
    _fields_ = [
        ("AccentState", ctypes.c_uint),
        ("AccentFlags", ctypes.c_uint),
        ("GradientColor", ctypes.c_uint),
        ("AnimationId", ctypes.c_uint),
    ]


class WINDOWCOMPOSITIONATTRIBDATA(ctypes.Structure):
    _fields_ = [
        ("Attribute", ctypes.c_int),
        ("Data", ctypes.POINTER(ctypes.c_int)),
        ("SizeOfData", ctypes.c_size_t),
    ]


_ACCENT_DISABLED = 0
_ACCENT_BLURBEHIND = 3
_ACCENT_ACRYLIC = 4
_WCA_ACCENT_POLICY = 19
_DWMWA_SYSTEMBACKDROP_TYPE = 38
_DWMWA_USE_IMMERSIVE_DARK_MODE = 20


def _set_composition_blur(hwnd: int, accent_state: int, gradient: int = 0) -> bool:
    try:
        accent = ACCENTPOLICY()
        accent.AccentState = accent_state
        accent.AccentFlags = 0
        accent.GradientColor = gradient
        accent.AnimationId = 0
        data = WINDOWCOMPOSITIONATTRIBDATA()
        data.Attribute = _WCA_ACCENT_POLICY
        data.Data = ctypes.cast(ctypes.pointer(accent), ctypes.POINTER(ctypes.c_int))
        data.SizeOfData = ctypes.sizeof(accent)
        result = ctypes.windll.user32.SetWindowCompositionAttribute(hwnd, ctypes.byref(data))
        return bool(result)
    except Exception as exc:  # pragma: no cover - platform specific
        log.debug("SetWindowCompositionAttribute failed: %s", exc)
        return False


def apply_blur(hwnd: int, enable: bool) -> None:
    """Enable or disable the native Windows acrylic/blur backdrop."""
    if not IS_WINDOWS:
        return
    try:
        value = ctypes.c_int(3 if enable else 1)  # 3 = acrylic, 1 = none
        result = ctypes.windll.dwmapi.DwmSetWindowAttribute(
            hwnd, _DWMWA_SYSTEMBACKDROP_TYPE, ctypes.byref(value), ctypes.sizeof(value)
        )
        if result == 0:
            return
    except Exception as exc:  # pragma: no cover - platform specific
        log.debug("DwmSetWindowAttribute failed: %s", exc)

    if enable:
        _set_composition_blur(hwnd, _ACCENT_ACRYLIC, 0x01000000)
    else:
        _set_composition_blur(hwnd, _ACCENT_DISABLED)


def set_dark_title_bar(hwnd: int, enabled: bool = True) -> None:
    if not IS_WINDOWS:
        return
    try:
        value = ctypes.c_int(1 if enabled else 0)
        ctypes.windll.dwmapi.DwmSetWindowAttribute(
            hwnd, _DWMWA_USE_IMMERSIVE_DARK_MODE, ctypes.byref(value), ctypes.sizeof(value)
        )
    except Exception:
        pass


def foreground_window() -> int:
    """Handle of the window that currently has keyboard focus (0 if unknown)."""
    if not IS_WINDOWS:
        return 0
    try:
        return int(ctypes.windll.user32.GetForegroundWindow())
    except Exception:
        return 0


def play_sound(path: str = "") -> None:
    """Play the configured WAV file, or the OS alert when none is set."""
    from PyQt6.QtWidgets import QApplication

    if path and os.path.exists(path) and IS_WINDOWS:
        try:
            import winsound

            winsound.PlaySound(path, winsound.SND_FILENAME | winsound.SND_ASYNC)
            return
        except Exception as exc:  # pragma: no cover - platform specific
            log.warning("Could not play %s: %s", path, exc)
    QApplication.beep()


def system_info() -> str:
    return f"{platform.system()} {platform.release()} ({platform.machine()})"
