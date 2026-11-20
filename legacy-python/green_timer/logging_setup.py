"""Application-wide logging configuration."""

from __future__ import annotations

import logging
import os
import sys
from logging.handlers import RotatingFileHandler

from .config import config_dir

_configured = False


def setup_logging(level: int = logging.INFO, console: bool = False) -> str:
    """Configure root logging to a rotating file and return the log path."""
    global _configured
    log_dir = os.path.join(config_dir(), "logs")
    try:
        os.makedirs(log_dir, exist_ok=True)
        path = os.path.join(log_dir, "green_timer.log")
    except OSError:
        return ""

    if _configured:
        return path

    root = logging.getLogger()
    root.setLevel(level)
    formatter = logging.Formatter(
        "%(asctime)s %(levelname)-8s %(name)s: %(message)s", "%Y-%m-%d %H:%M:%S"
    )

    try:
        file_handler = RotatingFileHandler(
            path, maxBytes=1_000_000, backupCount=3, encoding="utf-8"
        )
        file_handler.setFormatter(formatter)
        root.addHandler(file_handler)
    except OSError:
        return ""

    if console and sys.stderr is not None:
        stream = logging.StreamHandler()
        stream.setFormatter(formatter)
        root.addHandler(stream)

    _configured = True
    return path
