"""Global hotkey registration built on the ``keyboard`` library."""

from __future__ import annotations

import logging
from typing import Any, Callable, Dict, Optional

log = logging.getLogger(__name__)

try:  # pragma: no cover - optional dependency at import time
    import keyboard
except Exception as exc:  # pragma: no cover
    keyboard = None
    log.warning("Global hotkeys disabled: %s", exc)


def is_available() -> bool:
    return keyboard is not None


class HotkeyManager:
    """Routes configured hotkeys to callbacks.

    Hotkeys are compared by scan code, which is layout independent on Windows.
    ``rebind`` handles both configuration changes and recording of new keys.
    """

    def __init__(self, config: Dict[str, Any], handlers: Dict[str, Callable[[], None]]):
        self._config = config
        self._handlers = handlers
        self._hook: Optional[Any] = None

    def start(self) -> None:
        if keyboard is None:
            return
        self.stop()
        try:
            self._hook = keyboard.hook(self._on_event)
        except Exception as exc:  # pragma: no cover - platform specific
            log.error("Could not install keyboard hook: %s", exc)
            self._hook = None

    def stop(self) -> None:
        if keyboard is None:
            return
        # ``unhook_all`` also removes recording hooks, which is intentional:
        # the manager is the sole owner of keyboard hooks in the application.
        try:
            keyboard.unhook_all()
        except Exception:
            pass
        self._hook = None

    def _on_event(self, event) -> None:
        try:
            if event.event_type != keyboard.KEY_DOWN:
                return
            code = event.scan_code
            for config_key, handler in self._handlers.items():
                entry = self._config.get(config_key) or {}
                if entry.get("code") == code:
                    handler()
                    return
        except Exception as exc:  # pragma: no cover - hot path
            log.exception("Hotkey handler failed: %s", exc)

    def record_key(self, key_name: str, on_recorded: Callable[[str, int], None]) -> None:
        """Temporarily capture the next key press and report it."""
        if keyboard is None:
            return
        self.stop()

        def _capture(event):
            if event.event_type != keyboard.KEY_DOWN:
                return
            try:
                keyboard.unhook_all()
            except Exception:
                pass
            name = (event.name or "unknown").lower()
            if getattr(event, "is_keypad", False):
                name = f"numpad {name}"
            on_recorded(name, int(event.scan_code))

        try:
            keyboard.hook(_capture)
        except Exception as exc:  # pragma: no cover - platform specific
            log.error("Could not record hotkey: %s", exc)
