"""Green Timer entry point.

Kept intentionally small: wire up logging, build the Qt application, restore
settings and hand control to the UI.
"""

from __future__ import annotations

import logging
import os
import sys


def _enable_high_dpi() -> None:
    from PyQt6.QtCore import Qt
    from PyQt6.QtGui import QGuiApplication

    QGuiApplication.setHighDpiScaleFactorRoundingPolicy(
        Qt.HighDpiScaleFactorRoundingPolicy.PassThrough
    )


def main() -> int:
    from PyQt6.QtGui import QIcon
    from PyQt6.QtWidgets import QApplication

    from green_timer import APP_NAME, ICON_FILENAME, resource_path
    from green_timer.config import load_config
    from green_timer.logging_setup import setup_logging
    from green_timer.ui.window import TimerWindow

    log_path = setup_logging(console="--debug" in sys.argv)
    log = logging.getLogger("green_timer")
    log.info("Starting %s", APP_NAME)
    if log_path:
        log.info("Log file: %s", log_path)

    _enable_high_dpi()
    app = QApplication(sys.argv)
    app.setApplicationName(APP_NAME)
    app.setOrganizationName("GreenTimer")
    app.setQuitOnLastWindowClosed(False)

    icon_path = resource_path(ICON_FILENAME)
    if os.path.exists(icon_path):
        app.setWindowIcon(QIcon(icon_path))

    config = load_config()

    def _excepthook(exc_type, exc_value, exc_tb):
        log.critical("Unhandled exception", exc_info=(exc_type, exc_value, exc_tb))

    sys.excepthook = _excepthook

    window = TimerWindow(config)
    # ``TimerWindow`` shows itself unless "start hidden" is enabled, so the
    # reference is kept alive for the lifetime of the event loop.
    app._main_window = window
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
