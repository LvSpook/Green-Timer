"""Reusable Qt widgets used by the timer window and settings dialog."""

from __future__ import annotations

from typing import Optional

from PyQt6.QtCore import Qt, pyqtSignal
from PyQt6.QtGui import QColor, QFont, QFontMetrics, QPainter, QPainterPath
from PyQt6.QtWidgets import (
    QColorDialog,
    QPushButton,
    QSlider,
    QWidget,
)

def readable_text_color(color: QColor) -> str:
    """Black or white, whichever contrasts better with ``color``."""
    luminance = (0.299 * color.red() + 0.587 * color.green() + 0.114 * color.blue()) / 255
    return "#000000" if luminance > 0.55 else "#FFFFFF"


class ColorButton(QPushButton):
    """A button that shows its colour and opens a colour picker."""

    colorChanged = pyqtSignal(str)

    def __init__(self, color: str, parent: Optional[QWidget] = None, title: str = "Select Color"):
        super().__init__(parent)
        self._title = title
        self._color = QColor(color)
        self.clicked.connect(self._pick)
        self._refresh()

    def color(self) -> QColor:
        return QColor(self._color)

    def set_color(self, color: str) -> None:
        candidate = QColor(color)
        if candidate.isValid():
            self._color = candidate
            self._refresh()

    def _refresh(self) -> None:
        name = self._color.name()
        super().setText(name.upper())
        super().setStyleSheet(
            f"background-color: {name}; color: {readable_text_color(self._color)}; "
            "font-weight: bold; border: 1px solid rgba(0,0,0,90); padding: 4px;"
        )

    def _pick(self) -> None:
        color = QColorDialog.getColor(self._color, self, self._title)
        if color.isValid():
            self._color = color
            self._refresh()
            self.colorChanged.emit(color.name())


class SliderRow(QWidget):
    """Slider with a live numeric label, so users can see the exact value."""

    valueChanged = pyqtSignal(int)

    def __init__(self, minimum: int, maximum: int, value: int, suffix: str = "",
                 parent: Optional[QWidget] = None):
        super().__init__(parent)
        from PyQt6.QtWidgets import QHBoxLayout, QLabel

        self._suffix = suffix
        self.slider = QSlider(Qt.Orientation.Horizontal)
        self.slider.setRange(minimum, maximum)
        self.slider.setValue(value)
        self._label = QLabel()
        self._label.setMinimumWidth(40)
        self._label.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.slider, 1)
        layout.addWidget(self._label, 0)
        self._sync(value)
        self.slider.valueChanged.connect(self._on_change)

    def _sync(self, value: int) -> None:
        self._label.setText(f"{value}{self._suffix}")

    def _on_change(self, value: int) -> None:
        self._sync(value)
        self.valueChanged.emit(value)

    def value(self) -> int:
        return self.slider.value()

    def setValue(self, value: int) -> None:  # noqa: N802 - Qt naming
        self.slider.setValue(value)


class ProportionalSizeGrip(QWidget):
    """Bottom-right grip that resizes the window while keeping the text fitting.

    Resizing proportionally means the user drags one corner and the font size
    follows the available text width, which is what an overlay timer needs.
    """

    def __init__(self, parent: QWidget):
        super().__init__(parent)
        self.setFixedSize(16, 16)
        self.setCursor(Qt.CursorShape.SizeFDiagCursor)
        self.setToolTip("Drag to resize")
        self._dragging = False
        self._start_x = 0.0
        self._start_size = parent.size()

    def mousePressEvent(self, event):  # noqa: N802
        if event.button() == Qt.MouseButton.LeftButton:
            self._dragging = True
            self._start_x = event.globalPosition().toPoint().x()
            self._start_size = self.parent().size()
            event.accept()

    def mouseMoveEvent(self, event):  # noqa: N802
        if not self._dragging:
            return
        delta = event.globalPosition().toPoint().x() - self._start_x
        parent = self.parent()
        vertical = parent.config.get("rotation") in ("90°", "270°")
        new_w = max(80, self._start_size.width() + delta)
        available = (self._start_size.height() + delta) if vertical else new_w
        parent.fit_to_width(available)
        event.accept()

    def mouseReleaseEvent(self, event):  # noqa: N802
        if self._dragging:
            self._dragging = False
            self.parent().save_config()
        event.accept()

    def paintEvent(self, event):  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(QColor(150, 150, 150, 110))
        path = QPainterPath()
        path.moveTo(self.width(), 0)
        path.lineTo(self.width(), self.height())
        path.lineTo(0, self.height())
        painter.drawPath(path)


def fit_point_size(
    text: str,
    family: str,
    available_width: float,
    min_size: int = 4,
    max_size: int = 500,
) -> int:
    """Binary-search the largest bold point size whose text fits ``available_width``."""
    if not text or available_width <= 1:
        return min_size
    probe = QFont(family, 12, QFont.Weight.Bold)
    low, high = min_size, max_size
    best = min_size
    metrics = QFontMetrics(probe)
    while low <= high:
        mid = (low + high) // 2
        probe.setPointSize(mid)
        metrics = QFontMetrics(probe)
        if metrics.horizontalAdvance(text) <= available_width:
            best = mid
            low = mid + 1
        else:
            high = mid - 1
    return best


def text_box_metrics(text: str, family: str, point_size: int, padding: int,
                     lap_visible: bool) -> tuple[int, int]:
    """Compute the window size needed to display ``text`` with ``padding``."""
    font = QFont(family, point_size, QFont.Weight.Bold)
    metrics = QFontMetrics(font)
    width = metrics.horizontalAdvance(text) + padding * 2
    height = metrics.height() + padding * 2
    if lap_visible:
        height += int(metrics.height() * 0.55)
    return max(1, width), max(1, height)
