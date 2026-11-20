"""The always-on-top timer overlay window."""

from __future__ import annotations

import logging
import os
import time
from typing import Any, Dict, Optional

from PyQt6.QtCore import QPoint, QTimer, Qt
from PyQt6.QtGui import QColor, QIcon
from PyQt6.QtWidgets import (
    QApplication,
    QMenu,
    QStyle,
    QSystemTrayIcon,
    QWidget,
)

from .. import APP_NAME, APP_VERSION, ICON_FILENAME, resource_path
from ..config import save_config
from ..engine import (
    COLOR_NORMAL,
    COLOR_OVERDUE,
    COLOR_THRESHOLD,
    EVENT_CUE,
    MODE_CLOCK,
    Snapshot,
    TimerEngine,
)

from ..fonts import resolve_family
from ..hotkeys import HotkeyManager
from ..system import IS_WINDOWS, apply_blur, foreground_window, play_sound
from ..ui.renderer import Textures, draw_background, draw_foreground, load_textures, mix_color
from ..ui.widgets import ProportionalSizeGrip, fit_point_size, text_box_metrics
from ..updater import check_for_update, download_installer

log = logging.getLogger(__name__)

TICK_MS = 16          # ~60 fps for smooth fluid animation / sub-centisecond digits
IDLE_TICK_MS = 250    # while paused, poll less often to save power
COLOR_ANIMATION_SECONDS = 0.35


class TimerWindow(QWidget):
    def __init__(self, config: Dict[str, Any]):
        super().__init__()
        self.config = config
        self.engine = TimerEngine(config)

        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowStaysOnTopHint
            | Qt.WindowType.Tool
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.customContextMenuRequested.connect(self._show_context_menu)

        self._drag_offset: Optional[QPoint] = None
        self._settings_dialog = None
        self._textures = Textures()
        self._font_size = int(config.get("font_size", 42))
        self._snapshot = Snapshot()
        self._display_color = self._target_color()
        self._color_anim_start = 0.0
        self._color_anim_from = QColor(self._target_color())
        self._last_phase: Optional[str] = None

        self._grip = ProportionalSizeGrip(self)

        self._timer = QTimer(self)
        self._timer.timeout.connect(self._on_tick)
        self._timer.start(TICK_MS)

        self._hotkeys = HotkeyManager(config, {
            "hk_toggle": self.toggle,
            "hk_reset": self.reset,
            "hk_settings": self.open_settings,
            "hk_lap": self.trigger_lap,
            "hk_ghost": self.toggle_ghost_mode,
        })

        self._tray: Optional[QSystemTrayIcon] = None
        self._build_tray()
        self._hotkeys.start()
        self.reload_assets()
        self._apply_all()

        self._restore_geometry()
        self._update_timer_interval()
        self.engine.reset()
        if self.config.get("auto_check_updates", True):
            QTimer.singleShot(1500, self._check_updates_background)

    # -- setup -----------------------------------------------------------
    def _restore_geometry(self) -> None:
        # Starting hidden is only safe when a tray icon exists to restore from.
        start_hidden = bool(self.config.get("start_minimized", False)) and self._tray is not None
        if not start_hidden:
            self.show()
        x, y = self.config["pos_x"], self.config["pos_y"]
        if x >= 0 and y >= 0:
            self.setGeometry(x, y, self.config["width"], self.config["height"])
        else:
            self.resize(self.config["width"], self.config["height"])
            self._move_to_default_position()

    def _move_to_default_position(self) -> None:
        screen = QApplication.primaryScreen()
        if not screen:
            return
        area = screen.availableGeometry()
        margin = 24
        self.move(area.right() - self.width() - margin, area.top() + margin)

    def _build_tray(self) -> None:
        if not QSystemTrayIcon.isSystemTrayAvailable():
            return
        self._tray = QSystemTrayIcon(self)
        icon_path = resource_path(ICON_FILENAME)
        if icon_path and os.path.exists(icon_path):
            self._tray.setIcon(QIcon(icon_path))
        else:
            self._tray.setIcon(self.style().standardIcon(QStyle.StandardPixmap.SP_ComputerIcon))
        self._tray.setToolTip(f"{APP_NAME} {APP_VERSION}")

        menu = QMenu()
        start_pause = menu.addAction("Start / Pause")
        start_pause.triggered.connect(self.toggle)
        reset = menu.addAction("Reset")
        reset.triggered.connect(self.reset)
        menu.addSeparator()
        settings = menu.addAction("Settings...")
        settings.triggered.connect(self.open_settings)
        show = menu.addAction("Show / Hide Window")
        show.triggered.connect(self._toggle_visibility)
        menu.addSeparator()
        quit_action = menu.addAction("Exit")
        quit_action.triggered.connect(self.request_quit)

        self._tray.setContextMenu(menu)
        self._tray.activated.connect(self._on_tray_activated)
        self._tray.show()

    def _on_tray_activated(self, reason) -> None:
        if reason == QSystemTrayIcon.ActivationReason.Trigger:
            self._toggle_visibility()

    def _toggle_visibility(self) -> None:
        if self.isVisible():
            self.hide()
        else:
            self.show()
            self.raise_()
            self.activateWindow()

    def reload_assets(self) -> None:
        self._textures = load_textures(self.config.get("sprite_path", ""))

    # -- appearance ------------------------------------------------------
    def _target_color(self) -> QColor:
        role = self._snapshot.color_role if self._snapshot else COLOR_NORMAL
        if role in (COLOR_OVERDUE, COLOR_THRESHOLD):
            return QColor(self.config.get("gray_color", "#808080"))
        return QColor(self.config.get("normal_color", "#00FF66"))

    def _blur_desired(self) -> bool:
        return (
            IS_WINDOWS
            and bool(self.config.get("window_blur", False))
            and bool(self.config.get("show_background", True))
            and self.config.get("theme") != "Chalkboard"
        )

    def _apply_all(self) -> None:
        self.config["font_family"] = resolve_family(self.config.get("font_family", "Segoe UI"))
        self.setWindowOpacity(float(self.config.get("opacity", 1.0)))
        # Win32 composition calls are comparatively expensive, so only invoke
        # them when the desired blur state actually changed. The key includes
        # the window handle because a re-created window needs re-applying.
        key = (self._blur_desired(), int(self.winId()))
        if key != getattr(self, "_blur_key", None):
            self._blur_key = key
            apply_blur(key[1], key[0])
        self._grip.setVisible(not self.config.get("window_locked", False) and not self.config.get("ghost_mode", False))
        self.setAttribute(
            Qt.WidgetAttribute.WA_TransparentForMouseEvents, bool(self.config.get("ghost_mode", False))
        )
        self.update()

    def apply_appearance(self) -> None:
        self._apply_all()
        self._update_timer_interval()
        if self.config.get("rotation") in ("90°", "270°"):
            self._resize_for_text()
        # Repaint even when paused so live settings changes are visible.
        self.update()

    # -- public actions --------------------------------------------------
    def toggle(self) -> None:
        if self.config.get("mode") == MODE_CLOCK:
            return
        self.engine.toggle()
        self._update_timer_interval()
        self._on_tick()

    def reset(self) -> None:
        self.engine.reset()
        self._last_phase = None
        self._update_timer_interval()
        self._on_tick()

    def trigger_lap(self) -> None:
        if self.engine.trigger_lap():
            self._resize_for_text()

    def toggle_ghost_mode(self) -> None:
        self.config["ghost_mode"] = not self.config.get("ghost_mode", False)
        self._apply_all()
        self.save_config()

    def save_config(self) -> None:
        self.config["pos_x"] = self.x()
        self.config["pos_y"] = self.y()
        self.config["width"] = self.width()
        self.config["height"] = self.height()
        self.config["font_size"] = self._font_size
        if not save_config(self.config):
            log.warning("Settings could not be persisted")

    # -- settings --------------------------------------------------------
    def open_settings(self) -> None:
        if self._settings_dialog is not None:
            self._settings_dialog.raise_()
            self._settings_dialog.activateWindow()
            return
        from ..ui.settings import SettingsDialog

        self._settings_dialog = SettingsDialog(self, self.config)
        self._settings_dialog.finished.connect(self._on_settings_closed)
        self._settings_dialog.show()

    def _on_settings_closed(self, result: int) -> None:
        from PyQt6.QtWidgets import QDialog

        dialog = self._settings_dialog
        self._settings_dialog = None
        if dialog is None:
            return

        if result == QDialog.DialogCode.Accepted.value:
            self.save_config()
            # The manager holds a reference to the live config dict, so a
            # restart is enough to pick up rebound hotkeys.
            self._hotkeys.start()
            if (
                dialog.original_config.get("mode") != self.config.get("mode")
                or dialog.original_config.get("countdown_start") != self.config.get("countdown_start")
                or dialog.original_config.get("pomo_work") != self.config.get("pomo_work")
            ):
                self.reset()
            else:
                self._on_tick()
        else:
            # The dialog already restored the original configuration.
            self._on_tick()

    def request_quit(self) -> None:
        self.save_config()
        self._hotkeys.stop()
        worker = getattr(self, "_update_worker", None)
        if worker is not None and worker.isRunning():
            worker.wait(2000)
        if self._tray:
            self._tray.hide()
        QApplication.instance().quit()

    # -- updates ---------------------------------------------------------
    def _check_updates_background(self) -> None:
        from PyQt6.QtCore import QThread

        existing = getattr(self, "_update_worker", None)
        if existing is not None and existing.isRunning():
            return

        class Worker(QThread):
            result = None

            def run(self) -> None:  # noqa: D401 - QThread entry point
                self.result = check_for_update(APP_VERSION)

        worker = Worker(self)
        worker.finished.connect(lambda: self._handle_update_result(worker.result))
        worker.start()
        self._update_worker = worker

    def _handle_update_result(self, result) -> None:
        if not result:
            return
        from PyQt6.QtWidgets import QMessageBox

        latest, url = result
        answer = QMessageBox.question(
            self,
            "Update Available",
            f"{APP_NAME} {latest} is available (you have {APP_VERSION}).\n\n"
            "Download and run the installer now?",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
        )
        if answer == QMessageBox.StandardButton.Yes:
            self._install_update(url)

    def _install_update(self, url: str) -> None:
        import subprocess

        from PyQt6.QtWidgets import QMessageBox

        try:
            QApplication.setOverrideCursor(Qt.CursorShape.WaitCursor)
            path = download_installer(url)
        except Exception as exc:
            log.error("Update download failed: %s", exc)
            QMessageBox.critical(self, "Update Error", f"Could not download the update:\n{exc}")
            return
        finally:
            QApplication.restoreOverrideCursor()

        try:
            subprocess.Popen([path])
        except OSError as exc:
            QMessageBox.critical(self, "Update Error", f"Could not start the installer:\n{exc}")
            return
        self.request_quit()

    # -- tick loop -------------------------------------------------------
    def _update_timer_interval(self) -> None:
        active = self.engine.running or self.config.get("mode") == MODE_CLOCK
        if self.config.get("display_style") == "Fluid":
            active = True
        self._timer.setInterval(TICK_MS if active else IDLE_TICK_MS)

    def _on_tick(self) -> None:
        snapshot = self.engine.tick()

        # Auto-pause when a target window is configured and focus leaves it.
        if self.engine.running and self.config.get("target_hwnd"):
            current = foreground_window()
            if current and current != self.config["target_hwnd"]:
                self.engine.pause()
                self._update_timer_interval()

        for event in snapshot.events:
            if event == EVENT_CUE and self.config.get("sound_cue", True):
                play_sound(self.config.get("custom_sound_path", ""))

        self._last_phase = snapshot.phase
        previous_role = self._snapshot.color_role
        self._snapshot = snapshot

        if snapshot.color_role != previous_role:
            self._start_color_animation()

        if snapshot.text_changed or snapshot.lap_visible:
            self.update()
        elif self.config.get("display_style") == "Fluid":
            self.engine.fluid_phase += 0.25
            self.update()

        if snapshot.text_length_changed:
            self._resize_for_text()

    def _start_color_animation(self) -> None:
        self._color_anim_from = QColor(self._display_color)
        self._color_anim_start = time.monotonic()

    def _current_display_color(self) -> QColor:
        target = self._target_color()
        if self.config.get("reduce_motion", False):
            return target
        elapsed = time.monotonic() - self._color_anim_start
        if elapsed >= COLOR_ANIMATION_SECONDS:
            return target
        return mix_color(self._color_anim_from, target, elapsed / COLOR_ANIMATION_SECONDS)

    # -- geometry --------------------------------------------------------
    def _resize_for_text(self) -> None:
        text = self._snapshot.text or self.engine.current_text()
        family = self.config.get("font_family", "Segoe UI")
        width, height = text_box_metrics(
            text, family, self._font_size, int(self.config.get("padding", 20)),
            self._snapshot.lap_visible,
        )
        if self.config.get("rotation") in ("90°", "270°"):
            width, height = height, width
        if width != self.width() or height != self.height():
            self.resize(max(80, width), max(40, height))

    def fit_to_width(self, available_width: float) -> None:
        """Choose the largest font that fits, then resize the window to match."""
        text = self._snapshot.text or self.engine.current_text()
        padding = int(self.config.get("padding", 20))
        self._font_size = fit_point_size(
            text,
            self.config.get("font_family", "Segoe UI"),
            max(10.0, available_width - padding * 2),
        )
        width, height = text_box_metrics(
            text, self.config.get("font_family", "Segoe UI"), self._font_size, padding,
            self._snapshot.lap_visible,
        )
        if self.config.get("rotation") in ("90°", "270°"):
            self.resize(max(40, height), max(80, width))
        else:
            self.resize(max(80, width), max(40, height))
        self.update()

    def resizeEvent(self, event):  # noqa: N802
        super().resizeEvent(event)
        self._grip.move(self.width() - self._grip.width(), self.height() - self._grip.height())

    # -- painting --------------------------------------------------------
    def paintEvent(self, event):  # noqa: N802
        from PyQt6.QtGui import QPainter

        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)

        self._display_color = self._current_display_color()
        draw_background(painter, self.config, self.size(), self._display_color)
        draw_foreground(
            painter, self.config, self._snapshot, self._display_color,
            self._textures, self._font_size, self.engine.fluid_phase,
        )

    # -- mouse interaction -----------------------------------------------
    def mousePressEvent(self, event):  # noqa: N802
        if self.config.get("window_locked", False) or self.config.get("ghost_mode", False):
            return
        if event.button() == Qt.MouseButton.LeftButton:
            self._drag_offset = event.globalPosition().toPoint() - self.frameGeometry().topLeft()
        elif event.button() == Qt.MouseButton.RightButton:
            self.open_settings()

    def mouseMoveEvent(self, event):  # noqa: N802
        if self.config.get("window_locked", False) or self.config.get("ghost_mode", False):
            return
        if self._drag_offset is not None and event.buttons() & Qt.MouseButton.LeftButton:
            self.move(event.globalPosition().toPoint() - self._drag_offset)
            event.accept()

    def mouseReleaseEvent(self, event):  # noqa: N802
        if self._drag_offset is not None:
            self._drag_offset = None
            self.save_config()

    def mouseDoubleClickEvent(self, event):  # noqa: N802
        if not self.config.get("ghost_mode", False) and self.config.get("mode") != MODE_CLOCK:
            self.toggle()

    def _show_context_menu(self, position) -> None:
        menu = QMenu(self)
        menu.addAction("Start / Pause", self.toggle)
        menu.addAction("Reset", self.reset)
        menu.addAction("Lap", self.trigger_lap)
        menu.addSeparator()
        ghost = menu.addAction("Ghost Mode")
        ghost.setCheckable(True)
        ghost.setChecked(bool(self.config.get("ghost_mode", False)))
        ghost.triggered.connect(self.toggle_ghost_mode)
        menu.addAction("Settings...", self.open_settings)
        menu.addSeparator()
        menu.addAction("Exit", self.request_quit)
        menu.exec(self.mapToGlobal(position))

    def keyPressEvent(self, event):  # noqa: N802
        if event.key() == Qt.Key.Key_Escape:
            self.request_quit()
        elif event.key() == Qt.Key.Key_Space:
            self.toggle()
        else:
            super().keyPressEvent(event)
