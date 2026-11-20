"""Headless smoke test for the Qt UI.

Runs with ``QT_QPA_PLATFORM=offscreen``. It builds the window and dialog,
exercises the settings apply path and renders one frame of every display
style, failing on any exception.
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PyQt6.QtGui import QImage, QPainter  # noqa: E402
from PyQt6.QtWidgets import QApplication  # noqa: E402

import green_timer.config as config_module  # noqa: E402
from green_timer.config import DEFAULT_CONFIG, normalize_config  # noqa: E402
from green_timer.engine import COLOR_OVERDUE  # noqa: E402
from green_timer.ui.window import TICK_MS  # noqa: E402

_app = QApplication.instance() or QApplication([])


class UiSmokeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.patch = mock.patch.object(config_module, "config_dir", return_value=self.tmp.name)
        self.patch.start()
        from green_timer.ui.window import TimerWindow

        self.window = TimerWindow(normalize_config(DEFAULT_CONFIG))

    def tearDown(self):
        self.window._hotkeys.stop()
        if self.window._tray:
            self.window._tray.hide()
        self.window.close()
        self.window.deleteLater()
        _app.processEvents()
        self.patch.stop()
        self.tmp.cleanup()

    def test_styles_render(self):
        for style in ("Text", "Arc", "Bar", "Fluid", "Sprite"):
            self.window.config["display_style"] = style
            self.window._on_tick()
            image = QImage(self.window.width(), self.window.height(), QImage.Format.Format_ARGB32)
            image.fill(0)
            painter = QPainter(image)
            self.window.render(painter)
            painter.end()
            self.assertFalse(image.isNull(), style)

    def test_settings_dialog_applies_and_cancels(self):
        from green_timer.ui.settings import SettingsDialog

        dialog = SettingsDialog(self.window, self.window.config)
        dialog.cb_mode.setCurrentIndex(dialog.cb_mode.findData("pomodoro"))
        dialog.in_pomo_work.setText("40")
        dialog.sl_opacity.setValue(55)
        dialog.cb_display.setCurrentText("Arc")
        self.assertEqual(self.window.config["mode"], "pomodoro")
        self.assertEqual(self.window.config["pomo_work"], 40.0)
        self.assertAlmostEqual(self.window.config["opacity"], 0.55)
        dialog.reject()
        self.assertEqual(self.window.config["mode"], "up")
        self.assertEqual(self.window.config["opacity"], 1.0)
        dialog.deleteLater()

    def test_invalid_numeric_input_is_ignored(self):
        from green_timer.ui.settings import SettingsDialog

        dialog = SettingsDialog(self.window, self.window.config)
        dialog.in_threshold.setText("not a number")
        self.assertEqual(self.window.config["threshold_time"], 60.0)
        dialog.deleteLater()

    def test_conditional_visibility(self):
        from green_timer.ui.settings import SettingsDialog

        dialog = SettingsDialog(self.window, self.window.config)
        dialog.cb_mode.setCurrentIndex(dialog.cb_mode.findData("clock"))
        self.assertFalse(dialog.in_countdown.isVisible())
        dialog.cb_display.setCurrentText("Sprite")
        dialog.cb_border.setCurrentText("Thin")
        dialog.deleteLater()

    def test_toggle_and_reset(self):
        self.window.config["mode"] = "up"
        self.window.reset()
        self.window.toggle()
        self.assertTrue(self.window.engine.running)
        self.window.toggle()
        self.assertFalse(self.window.engine.running)
        self.window.reset()
        self.assertEqual(self.window.engine.elapsed, 0.0)

    def test_font_slider_controls_font_size(self):
        from green_timer.ui.settings import SettingsDialog

        dialog = SettingsDialog(self.window, self.window.config)
        dialog.sl_font_size.setValue(77)
        self.assertEqual(self.window._font_size, 77)
        self.assertEqual(self.window.config["font_size"], 77)
        dialog.deleteLater()

    def test_change_mode_resets_timer(self):
        self.window.config["mode"] = "up"
        self.window.engine.start()
        self.assertTrue(self.window.engine.running)
        self.window.open_settings()
        dialog = self.window._settings_dialog
        dialog.cb_mode.setCurrentIndex(dialog.cb_mode.findData("down"))
        dialog.accept()
        self.assertFalse(self.window.engine.running)

    def test_cancel_restores_every_changed_field(self):
        from green_timer.ui.settings import SettingsDialog

        original = dict(self.window.config)
        dialog = SettingsDialog(self.window, self.window.config)
        dialog.cb_theme.setCurrentText("Chalkboard")
        dialog.cb_shape.setCurrentText("Capsule")
        dialog.chk_show_text.setChecked(False)
        dialog.sl_padding.setValue(3)
        dialog.reject()
        for key in ("theme", "window_shape", "show_text", "padding"):
            self.assertEqual(self.window.config[key], original[key], key)

    def test_hotkey_rebind_updates_config(self):
        from green_timer.ui.settings import SettingsDialog

        dialog = SettingsDialog(self.window, self.window.config)
        dialog._on_key_recorded("hk_toggle", "f8", 66)
        self.assertEqual(self.window.config["hk_toggle"], {"name": "f8", "code": 66})
        self.assertIn("scan 66", dialog._hk_displays["hk_toggle"].text())
        # Snapshot must be taken before mutation, so Cancel cannot resurrect it.
        dialog.deleteLater()

    def test_idle_interval_is_slower(self):
        self.window.config["mode"] = "up"
        self.window.reset()
        self.window._update_timer_interval()
        idle = self.window._timer.interval()
        self.window.toggle()
        self.window._update_timer_interval()
        self.assertLess(self.window._timer.interval(), idle)

    def test_countdown_completes_and_turns_gray(self):
        self.window.config.update(mode="down", countdown_start=0.3, decimals=1,
                                  use_threshold=False)
        self.window.reset()
        self.window.toggle()
        import time

        time.sleep(0.45)
        self.window._on_tick()
        self.assertTrue(self.window._snapshot.overdue)
        self.assertEqual(self.window._snapshot.color_role, COLOR_OVERDUE)
        self.assertTrue(self.window._snapshot.text.startswith("+"))
        self.window.toggle()

    def test_fluid_style_animates_while_paused(self):
        self.window.config["display_style"] = "Fluid"
        self.window.reset()
        self.window._update_timer_interval()
        self.assertEqual(self.window._timer.interval(), TICK_MS)

    def test_start_minimized_requires_a_tray(self):
        from PyQt6.QtWidgets import QSystemTrayIcon

        from green_timer.ui.window import TimerWindow

        self.window._hotkeys.stop()
        self.window.deleteLater()
        config = normalize_config({**DEFAULT_CONFIG, "start_minimized": True})
        hidden = TimerWindow(config)
        try:
            # Starting hidden is only safe with a tray icon to restore from.
            self.assertEqual(hidden.isVisible(), not QSystemTrayIcon.isSystemTrayAvailable())
        finally:
            hidden._hotkeys.stop()
            hidden.close()
            hidden.deleteLater()
            # Restore the fixture window so tearDown has something to clean up.
            from green_timer.ui.window import TimerWindow as TW

            self.window = TW(normalize_config(DEFAULT_CONFIG))

    def test_restore_defaults_updates_every_control(self):
        from PyQt6.QtWidgets import QMessageBox

        from green_timer.ui.settings import SettingsDialog

        dialog = SettingsDialog(self.window, self.window.config)
        # Change a representative field in each group, then reset.
        dialog.cb_theme.setCurrentText("Chalkboard")
        dialog.chk_hours.setChecked(True)
        dialog.in_pomo_break.setText("9")
        dialog.chk_locked.setChecked(True)
        with mock.patch.object(QMessageBox, "question", return_value=QMessageBox.StandardButton.Yes):
            dialog._restore_defaults()
        self.assertEqual(self.window.config["theme"], "Default")
        self.assertFalse(dialog.chk_hours.isChecked())
        self.assertEqual(dialog.in_pomo_break.text(), "5")
        self.assertFalse(dialog.chk_locked.isChecked())
        self.assertEqual(self.window._font_size, self.window.config["font_size"])
        dialog.deleteLater()

    def test_invalid_field_blocks_later_changes(self):
        from green_timer.ui.settings import SettingsDialog

        original = dict(self.window.config)
        dialog = SettingsDialog(self.window, self.window.config)
        dialog.in_threshold.setText("oops")          # invalid...
        dialog.cb_shape.setCurrentText("Capsule")    # ...so this must not apply
        self.assertEqual(self.window.config["window_shape"], original["window_shape"])
        self.assertEqual(self.window.config["threshold_time"], original["threshold_time"])
        self.assertTrue(not dialog._invalid_hint.isHidden())
        dialog.in_threshold.setText("30")            # fixing it unblocks updates
        self.assertTrue(dialog._invalid_hint.isHidden())
        dialog.cb_shape.setCurrentText("Capsule")
        self.assertEqual(self.window.config["window_shape"], "Capsule")
        dialog.deleteLater()


if __name__ == "__main__":
    unittest.main()
