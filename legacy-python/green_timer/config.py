"""Configuration handling: defaults, validation, persistence and migration.

Historically the settings lived next to the executable (``timer_pro_settings.json``).
That fails for Program Files installs, where the install directory is read-only.
Settings now live in an OS-appropriate per-user directory, with a one-time
migration from the legacy location.
"""

from __future__ import annotations

import copy
import json
import logging
import os
import sys
import tempfile
from typing import Any, Dict

from . import CONFIG_FILENAME

log = logging.getLogger(__name__)

VALID_MODES = ("up", "down", "clock", "pomodoro")
VALID_DISPLAY_STYLES = ("Text", "Arc", "Bar", "Fluid", "Sprite")
VALID_SHAPES = ("Rectangular", "Rounded", "Capsule")
VALID_BORDERS = ("None", "Thin", "Neon")
VALID_ROTATIONS = ("0°", "90°", "270°")
VALID_THEMES = ("Default", "Chalkboard")

DEFAULT_CONFIG: Dict[str, Any] = {
    "mode": "up",
    "countdown_start": 60.0,
    "pomo_work": 25.0,
    "pomo_break": 5.0,
    "pomo_long_break": 15.0,
    "pomo_cycles": 4,
    "clock_format": "%H:%M:%S",
    "threshold_time": 60.0,
    "use_threshold": True,
    "font_size": 42,
    "font_family": "Segoe UI",
    "normal_color": "#00FF66",
    "gray_color": "#808080",
    "decimals": 2,
    "always_show_hours": False,
    "show_text": True,
    "hk_toggle": {"name": "f1", "code": 59},
    "hk_reset": {"name": "f2", "code": 60},
    "hk_settings": {"name": "f3", "code": 61},
    "hk_lap": {"name": "space", "code": 57},
    "hk_ghost": {"name": "f4", "code": 62},
    "ghost_mode": False,
    "sound_cue": True,
    "custom_sound_path": "",
    "opacity": 1.0,
    "text_outline": True,
    "text_outline_color": "#000000",
    "text_outline_thickness": 2,
    "theme": "Default",
    "border_style": "None",
    "border_thickness": 1,
    "window_shape": "Rectangular",
    "display_style": "Text",
    "sprite_path": "",
    "arc_thickness": 12,
    "window_locked": False,
    "pos_x": -1,
    "pos_y": -1,
    "width": 300,
    "height": 120,
    "target_hwnd": 0,
    "padding": 20,
    "corner_radius": 15,
    "window_blur": False,
    "show_background": True,
    "thin_border_color": "#FFFFFF",
    "bar_text_color": "#FFFFFF",
    "rotation": "0°",
    "auto_check_updates": True,
    "reduce_motion": False,
    "start_minimized": False,
}

HOTKEY_KEYS = ("hk_toggle", "hk_reset", "hk_settings", "hk_lap", "hk_ghost")


def _clamp(value: float, low: float, high: float) -> float:
    return max(low, min(high, value))


def _number(raw: Any, default: float, low: float, high: float) -> float:
    try:
        return _clamp(float(raw), low, high)
    except (TypeError, ValueError):
        return default


def _integer(raw: Any, default: int, low: int, high: int) -> int:
    try:
        return int(_clamp(int(raw), low, high))
    except (TypeError, ValueError):
        return default


def _choice(raw: Any, default: str, choices) -> str:
    return raw if raw in choices else default


def _bool(raw: Any, default: bool) -> bool:
    if isinstance(raw, bool):
        return raw
    if isinstance(raw, (int, float)):
        return bool(raw)
    if isinstance(raw, str):
        lowered = raw.strip().lower()
        if lowered in ("true", "1", "yes", "on"):
            return True
        if lowered in ("false", "0", "no", "off", ""):
            return False
    return default


def _color(raw: Any, default: str) -> str:
    """Keep any string Qt understands, but reject values that break styling."""
    if isinstance(raw, str) and raw.strip() and "\n" not in raw and len(raw) <= 32:
        return raw.strip()
    return default


def _hotkey(raw: Any, default: Dict[str, Any]) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return dict(default)
    name = raw.get("name")
    if not isinstance(name, str) or not name.strip():
        name = default["name"]
    return {"name": name.strip(), "code": _integer(raw.get("code"), default["code"], 0, 512)}


def normalize_config(raw: Any) -> Dict[str, Any]:
    """Return a fully populated, validated copy of the configuration.

    Unknown keys are dropped and invalid values fall back to their default so
    a hand-edited or corrupted file can never crash the application.
    """
    cfg = copy.deepcopy(DEFAULT_CONFIG)
    if not isinstance(raw, dict):
        return cfg

    for key, value in raw.items():
        if key not in DEFAULT_CONFIG:
            continue
        default = DEFAULT_CONFIG[key]
        if isinstance(default, dict):
            cfg[key] = _hotkey(value, default)
        elif isinstance(default, bool):
            cfg[key] = _bool(value, default)
        elif key in ("mode", "display_style", "window_shape", "border_style",
                     "rotation", "theme"):
            choices = {
                "mode": VALID_MODES,
                "display_style": VALID_DISPLAY_STYLES,
                "window_shape": VALID_SHAPES,
                "border_style": VALID_BORDERS,
                "rotation": VALID_ROTATIONS,
                "theme": VALID_THEMES,
            }[key]
            cfg[key] = _choice(value, default, choices)
        elif key in ("normal_color", "gray_color", "text_outline_color",
                     "thin_border_color", "bar_text_color"):
            cfg[key] = _color(value, default)
        else:
            cfg[key] = value

    # Numeric ranges.
    cfg["countdown_start"] = _number(cfg["countdown_start"], 60.0, 1.0, 359_999.0)
    cfg["threshold_time"] = _number(cfg["threshold_time"], 60.0, 0.0, 359_999.0)
    cfg["pomo_work"] = _number(cfg["pomo_work"], 25.0, 0.1, 600.0)
    cfg["pomo_break"] = _number(cfg["pomo_break"], 5.0, 0.1, 600.0)
    cfg["pomo_long_break"] = _number(cfg["pomo_long_break"], 15.0, 0.1, 600.0)
    cfg["pomo_cycles"] = _integer(cfg["pomo_cycles"], 4, 1, 100)
    cfg["font_size"] = _integer(cfg["font_size"], 42, 4, 500)
    cfg["decimals"] = _integer(cfg["decimals"], 2, 0, 3)
    cfg["opacity"] = _number(cfg["opacity"], 1.0, 0.1, 1.0)
    cfg["padding"] = _integer(cfg["padding"], 20, 0, 100)
    cfg["corner_radius"] = _integer(cfg["corner_radius"], 15, 0, 100)
    cfg["border_thickness"] = _integer(cfg["border_thickness"], 1, 1, 20)
    cfg["text_outline_thickness"] = _integer(cfg["text_outline_thickness"], 2, 1, 15)
    cfg["arc_thickness"] = _integer(cfg["arc_thickness"], 12, 1, 50)
    cfg["width"] = _integer(cfg["width"], 300, 80, 10_000)
    cfg["height"] = _integer(cfg["height"], 120, 40, 10_000)
    cfg["pos_x"] = _integer(cfg["pos_x"], -1, -100_000, 100_000)
    cfg["pos_y"] = _integer(cfg["pos_y"], -1, -100_000, 100_000)
    cfg["target_hwnd"] = _integer(cfg["target_hwnd"], 0, 0, 2**63 - 1)
    if not isinstance(cfg["clock_format"], str) or not cfg["clock_format"].strip():
        cfg["clock_format"] = DEFAULT_CONFIG["clock_format"]
    if not isinstance(cfg["font_family"], str) or not cfg["font_family"].strip():
        cfg["font_family"] = DEFAULT_CONFIG["font_family"]
    return cfg


def config_dir() -> str:
    """Return the per-user directory where settings are stored."""
    if sys.platform == "win32":
        base = os.environ.get("APPDATA") or os.path.expanduser("~")
        return os.path.join(base, "GreenTimer")
    if sys.platform == "darwin":
        return os.path.join(os.path.expanduser("~"), "Library", "Application Support", "GreenTimer")
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.join(os.path.expanduser("~"), ".config")
    return os.path.join(base, "greentimer")


def config_path() -> str:
    return os.path.join(config_dir(), CONFIG_FILENAME)


def legacy_config_path() -> str:
    """The pre-1.2 location: alongside the executable / working directory."""
    if getattr(sys, "frozen", False):
        return os.path.join(os.path.dirname(sys.executable), CONFIG_FILENAME)
    return os.path.abspath(CONFIG_FILENAME)


def load_config() -> Dict[str, Any]:
    """Load settings, migrating the legacy file on first run if needed."""
    path = config_path()
    if not os.path.exists(path):
        legacy = legacy_config_path()
        if os.path.exists(legacy) and os.path.abspath(legacy) != os.path.abspath(path):
            try:
                with open(legacy, "r", encoding="utf-8") as handle:
                    cfg = normalize_config(json.load(handle))
                save_config(cfg)
                log.info("Migrated settings from %s", legacy)
                return cfg
            except (OSError, ValueError) as exc:
                log.warning("Could not migrate legacy settings: %s", exc)

    try:
        with open(path, "r", encoding="utf-8") as handle:
            return normalize_config(json.load(handle))
    except FileNotFoundError:
        return copy.deepcopy(DEFAULT_CONFIG)
    except (OSError, ValueError) as exc:
        log.warning("Could not read settings (%s); using defaults", exc)
        return copy.deepcopy(DEFAULT_CONFIG)


def save_config(config: Dict[str, Any]) -> bool:
    """Atomically write settings. Returns ``False`` on failure instead of raising."""
    path = config_path()
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        handle, temp_path = tempfile.mkstemp(
            prefix=".settings-", suffix=".tmp", dir=os.path.dirname(path)
        )
        try:
            with os.fdopen(handle, "w", encoding="utf-8") as stream:
                json.dump(normalize_config(config), stream, indent=4, ensure_ascii=False)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temp_path, path)
            return True
        except BaseException:
            try:
                os.unlink(temp_path)
            except OSError:
                pass
            raise
    except OSError as exc:
        log.error("Could not save settings to %s: %s", path, exc)
        return False
