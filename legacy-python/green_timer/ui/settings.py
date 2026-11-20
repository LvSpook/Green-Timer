"""Settings dialog with live preview.

Every control writes straight into the live config dict and then asks the main
window to re-apply, so the overlay previews changes immediately. ``Cancel``
restores the snapshot taken when the dialog opened.
"""

from __future__ import annotations

import copy
import logging
import os
from typing import Any, Dict, Optional

from PyQt6.QtCore import Qt, pyqtSignal
from PyQt6.QtGui import QIcon
from PyQt6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QFileDialog,
    QFormLayout,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QScrollArea,
    QSpinBox,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

from .. import APP_NAME, APP_VERSION, GITHUB_REPO, resource_path
from ..config import DEFAULT_CONFIG, HOTKEY_KEYS
from ..fonts import available_families
from ..formatting import format_input, parse_duration
from ..system import IS_WINDOWS, system_info
from ..ui.widgets import ColorButton, SliderRow

log = logging.getLogger(__name__)

MODE_LABELS = {
    "up": "Count Up",
    "down": "Countdown",
    "clock": "Clock",
    "pomodoro": "Pomodoro",
}

HOTKEY_LABELS = {
    "hk_toggle": "Start / Pause",
    "hk_reset": "Reset",
    "hk_settings": "Open Settings",
    "hk_lap": "Trigger Lap",
    "hk_ghost": "Toggle Ghost Mode",
}


class SettingsDialog(QDialog):
    key_recorded = pyqtSignal(str, str, int)

    def __init__(self, parent, config: Dict[str, Any]):
        super().__init__(parent)
        self.main_window = parent
        self.original_config = copy.deepcopy(config)
        self._suspend = False
        self._loading = True

        self.setWindowTitle(f"{APP_NAME} Settings — {APP_VERSION}")
        self.setMinimumSize(560, 720)
        icon = resource_path("green_timer.ico")
        if os.path.exists(icon):
            self.setWindowIcon(QIcon(icon))
        self.setModal(False)

        self._build_ui()
        self._connect_signals()
        self._loading = False
        self._refresh_conditional()

    # -- construction helpers -------------------------------------------
    def _scroll_page(self) -> tuple[QWidget, QFormLayout]:
        """Create a tab page whose form is scrollable (small screens)."""
        page = QWidget()
        outer = QVBoxLayout(page)
        outer.setContentsMargins(0, 0, 0, 0)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QScrollArea.Shape.NoFrame)
        inner = QWidget()
        form = QFormLayout(inner)
        form.setFieldGrowthPolicy(QFormLayout.FieldGrowthPolicy.AllNonFixedFieldsGrow)
        form.setLabelAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        scroll.setWidget(inner)
        outer.addWidget(scroll)
        return page, form

    def _build_ui(self) -> None:
        layout = QVBoxLayout(self)
        self.tabs = QTabWidget()
        layout.addWidget(self.tabs, 1)

        self._build_general_tab()
        self._build_appearance_tab()
        self._build_behavior_tab()
        self._build_hotkeys_tab()
        self._build_about_tab()

        buttons = QHBoxLayout()
        self.btn_defaults = QPushButton("Restore Defaults")
        self.btn_defaults.clicked.connect(self._restore_defaults)
        self.btn_save = QPushButton("Save && Apply")
        self.btn_save.setDefault(True)
        self.btn_save.clicked.connect(self.accept)
        self.btn_cancel = QPushButton("Cancel")
        self.btn_cancel.clicked.connect(self.reject)
        self.btn_quit = QPushButton("Quit Timer")
        self.btn_quit.clicked.connect(self.main_window.request_quit)

        buttons.addWidget(self.btn_defaults)
        buttons.addStretch(1)
        buttons.addWidget(self.btn_cancel)
        buttons.addWidget(self.btn_save)
        buttons.addWidget(self.btn_quit)
        layout.addLayout(buttons)

        self._invalid_hint = QLabel("Some values are not valid yet and are being ignored.")
        self._invalid_hint.setStyleSheet("color: #E0A000;")
        self._invalid_hint.setVisible(False)
        self._invalid_hint.setWordWrap(True)
        layout.addWidget(self._invalid_hint)

        self.key_recorded.connect(self._on_key_recorded)

    def _build_general_tab(self) -> None:
        page, form = self._scroll_page()

        self.cb_mode = QComboBox()
        for key, label in MODE_LABELS.items():
            self.cb_mode.addItem(label, key)
        self._select_data(self.cb_mode, self.original_config["mode"])
        form.addRow("Timer Mode", self.cb_mode)

        self.in_countdown = QLineEdit(format_input(self.original_config["countdown_start"]))
        self.in_countdown.setPlaceholderText("e.g. 5:00, 90, or 0:01:30")
        self.in_countdown.setToolTip("Accepted formats: seconds, MM:SS or HH:MM:SS")
        form.addRow("Countdown Start", self.in_countdown)

        self.in_clock_format = QLineEdit(self.original_config.get("clock_format", "%H:%M:%S"))
        self.in_clock_format.setPlaceholderText("%H:%M:%S")
        self.in_clock_format.setToolTip("Any strftime pattern, e.g. %I:%M %p")
        form.addRow("Clock Format", self.in_clock_format)

        pomo = QGroupBox("Pomodoro")
        pomo_form = QFormLayout(pomo)
        self.in_pomo_work = QLineEdit(str(self.original_config.get("pomo_work", 25)))
        self.in_pomo_break = QLineEdit(str(self.original_config.get("pomo_break", 5)))
        self.in_pomo_long = QLineEdit(str(self.original_config.get("pomo_long_break", 15)))
        self.sp_pomo_cycles = QSpinBox()
        self.sp_pomo_cycles.setRange(1, 100)
        self.sp_pomo_cycles.setValue(int(self.original_config.get("pomo_cycles", 4)))
        pomo_form.addRow("Work length (min)", self.in_pomo_work)
        pomo_form.addRow("Short break (min)", self.in_pomo_break)
        pomo_form.addRow("Long break (min)", self.in_pomo_long)
        pomo_form.addRow("Work sessions per long break", self.sp_pomo_cycles)
        form.addRow(pomo)
        self._pomo_group = pomo

        self.chk_threshold = QCheckBox("Gray out the timer past the threshold")
        self.chk_threshold.setChecked(bool(self.original_config.get("use_threshold", True)))
        form.addRow(self.chk_threshold)
        self.in_threshold = QLineEdit(str(self.original_config.get("threshold_time", 60)))
        self.in_threshold.setToolTip("Seconds. In count-up mode it grays out after this long; "
                                     "in countdown mode it grays out below this value.")
        form.addRow("Threshold (seconds)", self.in_threshold)

        self.sp_decimals = QSpinBox()
        self.sp_decimals.setRange(0, 3)
        self.sp_decimals.setValue(int(self.original_config.get("decimals", 2)))
        form.addRow("Decimal precision", self.sp_decimals)

        self.chk_hours = QCheckBox("Always show hours (HH:MM:SS)")
        self.chk_hours.setChecked(bool(self.original_config.get("always_show_hours", False)))
        form.addRow(self.chk_hours)

        self._general_page = page
        self._general_form = form
        self.tabs.addTab(page, "General")

    def _build_appearance_tab(self) -> None:
        page, form = self._scroll_page()

        self.cb_theme = QComboBox()
        self.cb_theme.addItems(["Default", "Chalkboard"])
        self.cb_theme.setCurrentText(self.original_config.get("theme", "Default"))
        form.addRow("Theme", self.cb_theme)

        self.cb_display = QComboBox()
        self.cb_display.addItems(["Text", "Arc", "Bar", "Fluid", "Sprite"])
        self.cb_display.setCurrentText(self.original_config.get("display_style", "Text"))
        form.addRow("Display style", self.cb_display)

        self.chk_show_text = QCheckBox("Show timer text")
        self.chk_show_text.setChecked(bool(self.original_config.get("show_text", True)))
        form.addRow(self.chk_show_text)

        sprite_row = QWidget()
        sprite_layout = QHBoxLayout(sprite_row)
        sprite_layout.setContentsMargins(0, 0, 0, 0)
        self.in_sprite = QLineEdit(self.original_config.get("sprite_path", ""))
        self.in_sprite.setPlaceholderText("No image selected")
        self.btn_sprite = QPushButton("Browse…")
        self.btn_sprite.clicked.connect(self._pick_sprite)
        sprite_layout.addWidget(self.in_sprite, 1)
        sprite_layout.addWidget(self.btn_sprite)
        form.addRow("Sprite image", sprite_row)
        self._sprite_row = sprite_row

        self.cb_shape = QComboBox()
        self.cb_shape.addItems(["Rectangular", "Rounded", "Capsule"])
        self.cb_shape.setCurrentText(self.original_config.get("window_shape", "Rectangular"))
        form.addRow("Background shape", self.cb_shape)

        self.sl_padding = SliderRow(0, 100, int(self.original_config.get("padding", 20)), " px")
        form.addRow("Padding", self.sl_padding)
        self.sl_radius = SliderRow(0, 100, int(self.original_config.get("corner_radius", 15)), " px")
        form.addRow("Corner radius", self.sl_radius)
        self.sl_arc = SliderRow(1, 50, int(self.original_config.get("arc_thickness", 12)), " px")
        form.addRow("Arc thickness", self.sl_arc)

        self.cb_border = QComboBox()
        self.cb_border.addItems(["None", "Thin", "Neon"])
        self.cb_border.setCurrentText(self.original_config.get("border_style", "None"))
        form.addRow("Border style", self.cb_border)
        self.sl_border = SliderRow(1, 20, int(self.original_config.get("border_thickness", 1)), " px")
        form.addRow("Border thickness", self.sl_border)

        self.chk_bg = QCheckBox("Show background panel")
        self.chk_bg.setChecked(bool(self.original_config.get("show_background", True)))
        form.addRow(self.chk_bg)
        self.chk_blur = QCheckBox("Native blur / acrylic (Windows 11)")
        self.chk_blur.setChecked(bool(self.original_config.get("window_blur", False)))
        self.chk_blur.setEnabled(IS_WINDOWS)
        form.addRow(self.chk_blur)

        self.chk_outline = QCheckBox("Text outline")
        self.chk_outline.setChecked(bool(self.original_config.get("text_outline", True)))
        form.addRow(self.chk_outline)
        self.sl_outline = SliderRow(
            1, 15, int(self.original_config.get("text_outline_thickness", 2)), " px"
        )
        form.addRow("Outline thickness", self.sl_outline)

        self.sl_opacity = SliderRow(10, 100, int(float(self.original_config.get("opacity", 1.0)) * 100), " %")
        form.addRow("Window opacity", self.sl_opacity)

        self.btn_normal_color = ColorButton(self.original_config.get("normal_color", "#00FF66"),
                                            title="Normal color")
        form.addRow("Normal color", self.btn_normal_color)
        self.btn_threshold_color = ColorButton(self.original_config.get("gray_color", "#808080"),
                                               title="Threshold / overdue color")
        form.addRow("Threshold color", self.btn_threshold_color)
        self.btn_outline_color = ColorButton(self.original_config.get("text_outline_color", "#000000"),
                                             title="Outline color")
        form.addRow("Outline color", self.btn_outline_color)
        self.btn_border_color = ColorButton(self.original_config.get("thin_border_color", "#FFFFFF"),
                                            title="Border color")
        form.addRow("Border color", self.btn_border_color)
        self.btn_bar_text_color = ColorButton(self.original_config.get("bar_text_color", "#FFFFFF"),
                                              title="Bar text color")
        form.addRow("Bar text color", self.btn_bar_text_color)

        font_row = QWidget()
        font_layout = QHBoxLayout(font_row)
        font_layout.setContentsMargins(0, 0, 0, 0)
        self.cb_font = QComboBox()
        self.cb_font.setEditable(True)
        self.cb_font.addItems(available_families())
        self.cb_font.setCurrentText(self.original_config.get("font_family", "Segoe UI"))
        font_layout.addWidget(self.cb_font, 1)
        form.addRow("Font family", font_row)

        self.sl_font_size = SliderRow(8, 300, int(self.original_config.get("font_size", 42)), " pt")
        form.addRow("Font size", self.sl_font_size)

        self._appearance_page = page
        self._appearance_form = form
        self.tabs.addTab(page, "Appearance")

    def _build_behavior_tab(self) -> None:
        page, form = self._scroll_page()

        self.cb_rotation = QComboBox()
        self.cb_rotation.addItems(["0°", "90°", "270°"])
        self.cb_rotation.setCurrentText(self.original_config.get("rotation", "0°"))
        form.addRow("Rotation", self.cb_rotation)

        self.chk_locked = QCheckBox("Lock position (prevent dragging)")
        self.chk_locked.setChecked(bool(self.original_config.get("window_locked", False)))
        form.addRow(self.chk_locked)

        self.chk_start_minimized = QCheckBox("Start hidden in the tray")
        self.chk_start_minimized.setChecked(bool(self.original_config.get("start_minimized", False)))
        form.addRow(self.chk_start_minimized)

        self.chk_reduce_motion = QCheckBox("Reduce motion (disable colour fades)")
        self.chk_reduce_motion.setChecked(bool(self.original_config.get("reduce_motion", False)))
        form.addRow(self.chk_reduce_motion)

        self.chk_sound = QCheckBox("Play a sound cue on zero / phase change")
        self.chk_sound.setChecked(bool(self.original_config.get("sound_cue", True)))
        form.addRow(self.chk_sound)

        sound_row = QWidget()
        sound_layout = QHBoxLayout(sound_row)
        sound_layout.setContentsMargins(0, 0, 0, 0)
        self.in_sound = QLineEdit(self.original_config.get("custom_sound_path", ""))
        self.in_sound.setPlaceholderText("System alert")
        self.btn_sound = QPushButton("Browse…")
        self.btn_sound.clicked.connect(self._pick_sound)
        self.btn_test_sound = QPushButton("Test")
        self.btn_test_sound.clicked.connect(self._test_sound)
        sound_layout.addWidget(self.in_sound, 1)
        sound_layout.addWidget(self.btn_sound)
        sound_layout.addWidget(self.btn_test_sound)
        form.addRow("Custom sound (.wav)", sound_row)

        snap_row = QWidget()
        snap_layout = QHBoxLayout(snap_row)
        snap_layout.setContentsMargins(0, 0, 0, 0)
        for label, target in (
            ("Top Left", "top-left"),
            ("Top Right", "top-right"),
            ("Top Center", "top-center"),
            ("Bottom Center", "bottom-center"),
        ):
            button = QPushButton(label)
            button.clicked.connect(lambda _checked=False, t=target: self._snap(t))
            snap_layout.addWidget(button)
        form.addRow("Snap position", snap_row)

        focus_row = QWidget()
        focus_layout = QHBoxLayout(focus_row)
        focus_layout.setContentsMargins(0, 0, 0, 0)
        self.btn_focus = QPushButton("Pick target app…")
        self.btn_focus.setToolTip("Pauses the timer whenever another window has focus.")
        self.btn_focus.clicked.connect(self._start_focus_capture)
        self.btn_clear_focus = QPushButton("Clear")
        self.btn_clear_focus.clicked.connect(self._clear_focus_target)
        focus_layout.addWidget(self.btn_focus, 1)
        focus_layout.addWidget(self.btn_clear_focus)
        form.addRow("Auto-pause target", focus_row)
        self._update_focus_label()

        self.chk_auto_update = QCheckBox("Check for updates on launch")
        self.chk_auto_update.setChecked(bool(self.original_config.get("auto_check_updates", True)))
        form.addRow(self.chk_auto_update)
        self.btn_check_updates = QPushButton("Check for updates now")
        self.btn_check_updates.clicked.connect(self.main_window._check_updates_background)
        form.addRow(self.btn_check_updates)

        self._behavior_page = page
        self.tabs.addTab(page, "Behavior")

    def _build_hotkeys_tab(self) -> None:
        page = QWidget()
        layout = QVBoxLayout(page)
        hint = QLabel(
            "Hotkeys are global. Click Record, then press the key you want to use.\n"
            "Bindings are matched by physical key, so they work with any keyboard layout."
        )
        hint.setWordWrap(True)
        layout.addWidget(hint)

        grid = QGridLayout()
        grid.addWidget(QLabel("Action"), 0, 0)
        grid.addWidget(QLabel("Binding"), 0, 1)
        self._hk_displays: Dict[str, QLineEdit] = {}
        self._hk_buttons: Dict[str, QPushButton] = {}
        for row, key in enumerate(HOTKEY_KEYS, start=1):
            grid.addWidget(QLabel(HOTKEY_LABELS[key]), row, 0)
            display = QLineEdit(self._hotkey_label(key))
            display.setReadOnly(True)
            record = QPushButton("Record")
            record.clicked.connect(lambda _checked=False, k=key: self._record_key(k))
            grid.addWidget(display, row, 1)
            grid.addWidget(record, row, 2)
            self._hk_displays[key] = display
            self._hk_buttons[key] = record
        grid.setColumnStretch(1, 1)
        layout.addLayout(grid)
        layout.addStretch(1)
        self._hotkeys_page = page
        self.tabs.addTab(page, "Hotkeys")

    def _build_about_tab(self) -> None:
        page = QWidget()
        layout = QVBoxLayout(page)
        title = QLabel(f"<h2>{APP_NAME} {APP_VERSION}</h2>")
        layout.addWidget(title)
        body = QLabel(
            "<p>A lightweight, always-on-top overlay timer with count-up, countdown, "
            "clock and pomodoro modes.</p>"
            f"<p><b>System:</b> {system_info()}<br>"
            f"<b>Settings file:</b> {self._settings_location()}<br>"
            f"<b>Repository:</b> {GITHUB_REPO or 'not configured'}</p>"
            "<p>Hotkeys: F1 start/pause · F2 reset · F3 settings · F4 ghost mode · Space lap</p>"
        )
        body.setWordWrap(True)
        body.setTextInteractionFlags(Qt.TextInteractionFlag.TextBrowserInteraction)
        layout.addWidget(body)
        layout.addStretch(1)
        self.tabs.addTab(page, "About")

    @staticmethod
    def _settings_location() -> str:
        from ..config import config_path

        return config_path()

    @staticmethod
    def _select_data(combo: QComboBox, value: str) -> None:
        index = combo.findData(value)
        combo.setCurrentIndex(index if index >= 0 else 0)

    def _hotkey_label(self, key: str) -> str:
        entry = self.main_window.config.get(key) or DEFAULT_CONFIG[key]
        return f"{entry['name']}  (scan {entry['code']})"

    # -- signals ---------------------------------------------------------
    def _connect_signals(self) -> None:
        self.cb_mode.currentIndexChanged.connect(self._apply)
        self.in_countdown.textChanged.connect(self._apply)
        self.in_clock_format.textChanged.connect(self._apply)
        self.in_pomo_work.textChanged.connect(self._apply)
        self.in_pomo_break.textChanged.connect(self._apply)
        self.in_pomo_long.textChanged.connect(self._apply)
        self.sp_pomo_cycles.valueChanged.connect(self._apply)
        self.chk_threshold.stateChanged.connect(self._apply)
        self.in_threshold.textChanged.connect(self._apply)
        self.sp_decimals.valueChanged.connect(self._apply)
        self.chk_hours.stateChanged.connect(self._apply)

        self.cb_theme.currentIndexChanged.connect(self._apply)
        self.cb_display.currentIndexChanged.connect(self._apply)
        self.chk_show_text.stateChanged.connect(self._apply)
        self.in_sprite.textChanged.connect(self._apply)
        self.cb_shape.currentIndexChanged.connect(self._apply)
        for slider in (self.sl_padding, self.sl_radius, self.sl_arc, self.sl_border,
                       self.sl_outline, self.sl_opacity):
            slider.valueChanged.connect(self._apply)
        self.cb_border.currentIndexChanged.connect(self._apply)
        self.chk_bg.stateChanged.connect(self._apply)
        self.chk_blur.stateChanged.connect(self._apply)
        self.chk_outline.stateChanged.connect(self._apply)
        for button in (self.btn_normal_color, self.btn_threshold_color, self.btn_outline_color,
                       self.btn_border_color, self.btn_bar_text_color):
            button.colorChanged.connect(self._apply)
        self.cb_font.currentTextChanged.connect(self._apply)
        self.sl_font_size.valueChanged.connect(self._apply)

        self.cb_rotation.currentIndexChanged.connect(self._apply)
        self.chk_locked.stateChanged.connect(self._apply)
        self.chk_start_minimized.stateChanged.connect(self._apply)
        self.chk_reduce_motion.stateChanged.connect(self._apply)
        self.chk_sound.stateChanged.connect(self._apply)
        self.in_sound.textChanged.connect(self._apply)
        self.chk_auto_update.stateChanged.connect(self._apply)

    def _refresh_conditional(self) -> None:
        mode = self.cb_mode.currentData()
        display = self.cb_display.currentText()
        border = self.cb_border.currentText()

        def show(widget, visible: bool) -> None:
            # A QFormLayout row only collapses when both the field and its
            # label are hidden.
            widget.setVisible(visible)
            for form in (self._general_form, self._appearance_form):
                label = form.labelForField(widget)
                if label is not None:
                    label.setVisible(visible)
                    break

        show(self.in_countdown, mode == "down")
        show(self.in_clock_format, mode == "clock")
        show(self.in_threshold, mode in ("up", "down"))
        show(self.chk_threshold, mode in ("up", "down"))
        show(self._pomo_group, mode == "pomodoro")
        show(self.sl_arc, display == "Arc")
        show(self.sl_border, border != "None")
        show(self.btn_border_color, border == "Thin")
        show(self.btn_bar_text_color, display == "Bar")
        show(self._sprite_row, display == "Sprite")

    # -- live apply ------------------------------------------------------
    def _collect(self) -> Optional[Dict[str, Any]]:
        """Read all widgets into a dict, or ``None`` if any input is invalid.

        Building a complete dict first means a partially typed value can never
        leave the live config half-updated.
        """
        try:
            return {
                "mode": self.cb_mode.currentData() or "up",
                "countdown_start": parse_duration(self.in_countdown.text()),
                "clock_format": self.in_clock_format.text().strip() or "%H:%M:%S",
                "pomo_work": float(self.in_pomo_work.text() or 25),
                "pomo_break": float(self.in_pomo_break.text() or 5),
                "pomo_long_break": float(self.in_pomo_long.text() or 15),
                "pomo_cycles": self.sp_pomo_cycles.value(),
                "use_threshold": self.chk_threshold.isChecked(),
                "threshold_time": float(self.in_threshold.text() or 0),
                "decimals": self.sp_decimals.value(),
                "always_show_hours": self.chk_hours.isChecked(),
                "theme": self.cb_theme.currentText(),
                "display_style": self.cb_display.currentText(),
                "show_text": self.chk_show_text.isChecked(),
                "sprite_path": self.in_sprite.text().strip(),
                "window_shape": self.cb_shape.currentText(),
                "padding": self.sl_padding.value(),
                "corner_radius": self.sl_radius.value(),
                "arc_thickness": self.sl_arc.value(),
                "border_style": self.cb_border.currentText(),
                "border_thickness": self.sl_border.value(),
                "show_background": self.chk_bg.isChecked(),
                "window_blur": self.chk_blur.isChecked(),
                "text_outline": self.chk_outline.isChecked(),
                "text_outline_thickness": self.sl_outline.value(),
                "opacity": self.sl_opacity.value() / 100.0,
                "normal_color": self.btn_normal_color.color().name(),
                "gray_color": self.btn_threshold_color.color().name(),
                "text_outline_color": self.btn_outline_color.color().name(),
                "thin_border_color": self.btn_border_color.color().name(),
                "bar_text_color": self.btn_bar_text_color.color().name(),
                "font_family": self.cb_font.currentText().strip() or "Segoe UI",
                "font_size": self.sl_font_size.value(),
                "rotation": self.cb_rotation.currentText(),
                "window_locked": self.chk_locked.isChecked(),
                "start_minimized": self.chk_start_minimized.isChecked(),
                "reduce_motion": self.chk_reduce_motion.isChecked(),
                "sound_cue": self.chk_sound.isChecked(),
                "custom_sound_path": self.in_sound.text().strip(),
                "auto_check_updates": self.chk_auto_update.isChecked(),
            }
        except ValueError:
            return None

    def _apply(self, *_args) -> None:
        if self._loading:
            return
        self._refresh_conditional()

        values = self._collect()
        if values is None:
            # Partially typed input: keep the last valid configuration.
            self._invalid_hint.setVisible(True)
            return
        self._invalid_hint.setVisible(False)
        previous_sprite = self.main_window.config.get("sprite_path")
        self.main_window.config.update(values)

        window = self.main_window
        # Decoding an image is expensive; only redo it when the path changes.
        if values["sprite_path"] != previous_sprite:
            window.reload_assets()
        window.apply_appearance()
        # Apply the requested font size directly; resizing the window is the
        # grip's job, so the slider value is never overwritten here.
        window._font_size = values["font_size"]
        # Refresh the displayed value so format/mode changes are immediately
        # reflected even while the timer is paused.
        window._on_tick()

    # -- pickers ---------------------------------------------------------
    def _pick_sprite(self) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self, "Select Sprite Image", "",
            "Images (*.png *.jpg *.jpeg *.bmp *.gif *.webp);;All files (*)",
        )
        if path:
            self.in_sprite.setText(path)

    def _pick_sound(self) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self, "Select WAV Sound", "", "WAV files (*.wav);;All files (*)"
        )
        if path:
            self.in_sound.setText(path)

    def _test_sound(self) -> None:
        from ..system import play_sound

        play_sound(self.in_sound.text().strip())

    def _snap(self, target: str) -> None:
        from PyQt6.QtWidgets import QApplication

        screen = self.main_window.screen() or QApplication.primaryScreen()
        if not screen:
            return
        area = screen.availableGeometry()
        window = self.main_window
        margin = 24
        if target == "top-left":
            window.move(area.left() + margin, area.top() + margin)
        elif target == "top-right":
            window.move(area.right() - window.width() - margin, area.top() + margin)
        elif target == "top-center":
            window.move(area.left() + (area.width() - window.width()) // 2, area.top() + margin)
        elif target == "bottom-center":
            window.move(area.left() + (area.width() - window.width()) // 2,
                        area.bottom() - window.height() - margin)
        window.save_config()

    def _start_focus_capture(self) -> None:
        if not IS_WINDOWS:
            QMessageBox.information(self, "Not available", "Auto-pause targets require Windows.")
            return
        self.btn_focus.setEnabled(False)
        self.btn_focus.setText("Switch to the target app…")
        from PyQt6.QtCore import QTimer

        QTimer.singleShot(5000, self._capture_focus)

    def _capture_focus(self) -> None:
        from ..system import foreground_window

        hwnd = foreground_window()
        if hwnd and hwnd != int(self.winId()):
            self.main_window.config["target_hwnd"] = hwnd
        self.btn_focus.setEnabled(True)
        self._update_focus_label()

    def _clear_focus_target(self) -> None:
        self.main_window.config["target_hwnd"] = 0
        self._update_focus_label()

    def _update_focus_label(self) -> None:
        hwnd = self.main_window.config.get("target_hwnd", 0)
        self.btn_focus.setText(f"Target captured (HWND {hwnd})" if hwnd else "Pick target app…")

    # -- hotkeys ---------------------------------------------------------
    def _record_key(self, key: str) -> None:
        button = self._hk_buttons[key]
        button.setEnabled(False)
        button.setText("Press a key…")
        self.main_window._hotkeys.record_key(
            key, lambda name, code, k=key: self.key_recorded.emit(k, name, code)
        )

    def _on_key_recorded(self, key: str, name: str, code: int) -> None:
        self.main_window.config[key] = {"name": name, "code": code}
        self._hk_displays[key].setText(f"{name}  (scan {code})")
        button = self._hk_buttons[key]
        button.setEnabled(True)
        button.setText("Record")
        self.main_window._hotkeys.start()

    # -- actions ---------------------------------------------------------
    def _restore_defaults(self) -> None:
        answer = QMessageBox.question(
            self, "Restore Defaults",
            "Reset every setting to its default value?",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
        )
        if answer != QMessageBox.StandardButton.Yes:
            return
        config = self.main_window.config
        config.clear()
        config.update(copy.deepcopy(DEFAULT_CONFIG))
        self.main_window._font_size = int(config["font_size"])
        self.main_window.reload_assets()
        self.main_window.apply_appearance()
        self.main_window.reset()
        self._reload_controls()

    def _reload_controls(self) -> None:
        """Push config values back into every widget (after reset/cancel)."""
        cfg = self.main_window.config
        self._loading = True
        try:
            self._select_data(self.cb_mode, cfg["mode"])
            self.in_countdown.setText(format_input(cfg["countdown_start"]))
            self.in_clock_format.setText(cfg["clock_format"])
            self.in_pomo_work.setText(f"{float(cfg['pomo_work']):g}")
            self.in_pomo_break.setText(f"{float(cfg['pomo_break']):g}")
            self.in_pomo_long.setText(f"{float(cfg['pomo_long_break']):g}")
            self.sp_pomo_cycles.setValue(int(cfg["pomo_cycles"]))
            self.chk_threshold.setChecked(bool(cfg["use_threshold"]))
            self.in_threshold.setText(f"{float(cfg['threshold_time']):g}")
            self.sp_decimals.setValue(int(cfg["decimals"]))
            self.chk_hours.setChecked(bool(cfg["always_show_hours"]))

            self.cb_theme.setCurrentText(cfg["theme"])
            self.cb_display.setCurrentText(cfg["display_style"])
            self.chk_show_text.setChecked(bool(cfg["show_text"]))
            self.in_sprite.setText(cfg["sprite_path"])
            self.cb_shape.setCurrentText(cfg["window_shape"])
            self.sl_padding.setValue(int(cfg["padding"]))
            self.sl_radius.setValue(int(cfg["corner_radius"]))
            self.sl_arc.setValue(int(cfg["arc_thickness"]))
            self.cb_border.setCurrentText(cfg["border_style"])
            self.sl_border.setValue(int(cfg["border_thickness"]))
            self.chk_bg.setChecked(bool(cfg["show_background"]))
            self.chk_blur.setChecked(bool(cfg["window_blur"]))
            self.chk_outline.setChecked(bool(cfg["text_outline"]))
            self.sl_outline.setValue(int(cfg["text_outline_thickness"]))
            self.sl_opacity.setValue(int(float(cfg["opacity"]) * 100))
            self.btn_normal_color.set_color(cfg["normal_color"])
            self.btn_threshold_color.set_color(cfg["gray_color"])
            self.btn_outline_color.set_color(cfg["text_outline_color"])
            self.btn_border_color.set_color(cfg["thin_border_color"])
            self.btn_bar_text_color.set_color(cfg["bar_text_color"])
            self.cb_font.setCurrentText(cfg["font_family"])
            self.sl_font_size.setValue(int(cfg["font_size"]))

            self.cb_rotation.setCurrentText(cfg["rotation"])
            self.chk_locked.setChecked(bool(cfg["window_locked"]))
            self.chk_start_minimized.setChecked(bool(cfg["start_minimized"]))
            self.chk_reduce_motion.setChecked(bool(cfg["reduce_motion"]))
            self.chk_sound.setChecked(bool(cfg["sound_cue"]))
            self.in_sound.setText(cfg["custom_sound_path"])
            self.chk_auto_update.setChecked(bool(cfg["auto_check_updates"]))
            self._update_focus_label()
            for key in HOTKEY_KEYS:
                self._hk_displays[key].setText(self._hotkey_label(key))
        finally:
            self._loading = False
        self._refresh_conditional()

    def reject(self) -> None:
        """Undo live preview by restoring the configuration snapshot."""
        self._restore_snapshot()
        super().reject()

    def _restore_snapshot(self) -> None:
        self.main_window.config.clear()
        self.main_window.config.update(copy.deepcopy(self.original_config))
        self.main_window._font_size = int(self.original_config.get("font_size", 42))
        self.main_window.reload_assets()
        self.main_window.apply_appearance()
        self.main_window._on_tick()
        self._reload_controls()
