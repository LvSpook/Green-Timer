"""Painting of the timer overlay.

All drawing lives here so the window class stays focused on behaviour. The
renderer is stateless apart from cached sprite pixmaps produced by the caller.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Any, Dict, Optional

from PyQt6.QtCore import QPointF, QRectF, Qt
from PyQt6.QtGui import (
    QBrush,
    QColor,
    QFont,
    QFontMetrics,
    QImage,
    QPainter,
    QPainterPath,
    QPen,
    QPixmap,
)

from ..engine import Snapshot

CHALK_BOARD = QColor(46, 68, 50, 255)
CHALK_TEXT = QColor(240, 255, 240, 70)


@dataclass
class Textures:
    color: Optional[QPixmap] = None
    gray: Optional[QPixmap] = None


def load_textures(path: str) -> Textures:
    textures = Textures()
    if not path:
        return textures
    pixmap = QPixmap(path)
    if pixmap.isNull():
        return textures
    textures.color = pixmap
    image = pixmap.toImage().convertToFormat(QImage.Format.Format_Grayscale8)
    textures.gray = QPixmap.fromImage(image)
    return textures


def _corner_radius(config: Dict[str, Any], rect: QRectF) -> float:
    shape = config.get("window_shape", "Rectangular")
    if shape == "Capsule":
        return min(rect.width(), rect.height()) / 2
    if shape == "Rounded":
        return float(config.get("corner_radius", 15))
    return 0.0


def draw_background(painter: QPainter, config: Dict[str, Any], size, color: QColor) -> None:
    if not config.get("show_background", True):
        return

    is_chalk = config.get("theme") == "Chalkboard"
    alpha = 100 if config.get("window_blur", False) else 140
    background = CHALK_BOARD if is_chalk else QColor(0, 0, 0, alpha)

    thickness = float(config.get("border_thickness", 1))
    border_style = config.get("border_style", "None")
    rect = QRectF(0, 0, size.width(), size.height())
    if border_style != "None":
        rect.adjust(thickness / 2, thickness / 2, -thickness / 2, -thickness / 2)

    if border_style == "Thin":
        painter.setPen(QPen(QColor(config.get("thin_border_color", "#FFFFFF")), thickness))
    elif border_style == "Neon":
        painter.setPen(QPen(color, thickness))
    elif is_chalk:
        painter.setPen(QPen(QColor(100, 70, 40, 255), 4))
    else:
        painter.setPen(Qt.PenStyle.NoPen)

    painter.setBrush(QBrush(background))
    radius = _corner_radius(config, rect)
    painter.drawRoundedRect(rect, radius, radius)


def _centered_text_path(font: QFont, text: str, center: QPointF) -> QPainterPath:
    """Build a text path centred on ``center``.

    Note: ``QFontMetrics.boundingRect(str)`` can return an origin offset far from
    zero on some Qt builds, so the baseline is derived from ascent/descent
    instead, which is always reliable.
    """
    metrics = QFontMetrics(font)
    advance = metrics.horizontalAdvance(text)
    ascent = metrics.ascent()
    descent = metrics.descent()
    x = center.x() - advance / 2
    baseline = center.y() + (ascent - descent) / 2
    path = QPainterPath()
    path.addText(x, baseline, font, text)
    return path


def draw_text(
    painter: QPainter,
    config: Dict[str, Any],
    font: QFont,
    text: str,
    rect: QRectF,
    fill: QColor,
) -> None:
    if not text:
        return
    path = _centered_text_path(font, text, rect.center())

    if config.get("theme") == "Chalkboard":
        painter.setPen(Qt.PenStyle.NoPen)
        for dx, dy, shade in (
            (-1, -1, 40), (1, 1, 40), (-1, 1, 40), (1, -1, 40), (0, 0, 90), (0, 2, 60), (2, 0, 60)
        ):
            painter.fillPath(path.translated(dx, dy), QBrush(QColor(fill.red(), fill.green(), fill.blue(), shade)))
        painter.fillPath(path, QBrush(fill))
        return

    if config.get("text_outline", True):
        pen = QPen(
            QColor(config.get("text_outline_color", "#000000")),
            float(config.get("text_outline_thickness", 2)),
        )
        pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
        painter.strokePath(path, pen)
    painter.fillPath(path, QBrush(fill))


def _draw_arc(painter: QPainter, config: Dict[str, Any], rect: QRectF,
              progress: float, color: QColor) -> None:
    dim = min(rect.width(), rect.height())
    arc_rect = QRectF(rect.center().x() - dim / 2, rect.center().y() - dim / 2, dim, dim)
    thickness = float(config.get("arc_thickness", 12))
    track = QColor(60, 60, 60, 150 if config.get("show_background", True) else 60)
    painter.setPen(QPen(track, thickness, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
    painter.drawArc(arc_rect, 0, 360 * 16)
    painter.setPen(QPen(color, thickness, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
    painter.drawArc(arc_rect, 90 * 16, int(-progress * 360 * 16))


def _clipped_shape(config: Dict[str, Any], rect: QRectF, fill: QPainterPath) -> QPainterPath:
    container = QPainterPath()
    radius = _corner_radius(config, rect)
    container.addRoundedRect(rect, radius, radius)
    return fill.intersected(container)


def _draw_bar(painter: QPainter, config: Dict[str, Any], rect: QRectF,
              progress: float, color: QColor) -> None:
    fill_rect = QRectF(rect.x(), rect.y(), rect.width() * progress, rect.height())
    fill_path = QPainterPath()
    fill_path.addRect(fill_rect)
    painter.fillPath(_clipped_shape(config, rect, fill_path), color)


def _draw_fluid(painter: QPainter, config: Dict[str, Any], rect: QRectF,
                progress: float, color: QColor, phase: float) -> None:
    surface = rect.bottom() - rect.height() * progress
    path = QPainterPath()
    path.moveTo(rect.left(), rect.bottom())
    amplitude = min(6.0, rect.height() / 8)
    step = max(1.0, rect.width() / 240)
    x = rect.left()
    while x <= rect.right():
        y = surface + math.sin(x * 0.05 + phase) * amplitude
        path.lineTo(x, max(rect.top(), min(y, rect.bottom())))
        x += step
    path.lineTo(rect.right(), rect.bottom())
    path.closeSubpath()
    painter.fillPath(_clipped_shape(config, rect, path), color)


def _draw_sprite(painter: QPainter, rect: QRectF, progress: float,
                 textures: Textures) -> None:
    if textures.color is None or textures.color.isNull():
        return
    target_size = rect.size().toSize()
    color_pix = textures.color.scaled(
        target_size, Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation
    )
    gray = textures.gray
    gray_pix = gray.scaled(
        target_size, Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation
    ) if gray is not None else color_pix
    target = QRectF(
        rect.center().x() - color_pix.width() / 2,
        rect.center().y() - color_pix.height() / 2,
        color_pix.width(),
        color_pix.height(),
    )
    painter.drawPixmap(target.toRect(), gray_pix)
    fill_height = target.height() * progress
    if fill_height > 0:
        clip = QRectF(target.x(), target.bottom() - fill_height, target.width(), fill_height)
        painter.save()
        painter.setClipRect(clip)
        painter.drawPixmap(target.toRect(), color_pix)
        painter.restore()


def draw_foreground(
    painter: QPainter,
    config: Dict[str, Any],
    snapshot: Snapshot,
    color: QColor,
    textures: Textures,
    font_size: int,
    fluid_phase: float = 0.0,
) -> None:
    """Draw the rotating content rect (arc/bar/fluid/sprite plus text)."""
    padding = float(config.get("padding", 20))
    width, height = painter.device().width(), painter.device().height()
    rect = QRectF(padding, padding, max(1, width - padding * 2), max(1, height - padding * 2))

    center = QRectF(0, 0, width, height).center()
    painter.save()
    painter.translate(center)
    rotation = config.get("rotation", "0°")
    if rotation == "90°":
        painter.rotate(90)
    elif rotation == "270°":
        painter.rotate(270)

    if rotation in ("90°", "270°"):
        local_w, local_h = rect.height(), rect.width()
    else:
        local_w, local_h = rect.width(), rect.height()
    draw_rect = QRectF(-local_w / 2, -local_h / 2, local_w, local_h)

    font = QFont(config.get("font_family", "Segoe UI"), font_size, QFont.Weight.Bold)
    show_text = config.get("show_text", True)

    text_rect = QRectF(draw_rect)
    if snapshot.lap_visible:
        text_rect.translate(0, -local_h * 0.1)

    style = config.get("display_style", "Text")
    if style == "Arc":
        _draw_arc(painter, config, draw_rect, snapshot.progress, color)
        if show_text:
            draw_text(painter, config, font, snapshot.text, text_rect, color)
    elif style == "Bar":
        _draw_bar(painter, config, draw_rect, snapshot.progress, color)
        if show_text:
            bar_color = QColor(config.get("bar_text_color", "#FFFFFF"))
            draw_text(painter, config, font, snapshot.text, text_rect, bar_color)
    elif style == "Fluid":
        _draw_fluid(painter, config, draw_rect, snapshot.progress, color, fluid_phase)
        if show_text:
            draw_text(painter, config, font, snapshot.text, text_rect, color)
    elif style == "Sprite":
        _draw_sprite(painter, draw_rect, snapshot.progress, textures)
        if show_text:
            draw_text(painter, config, font, snapshot.text, text_rect, color)
    elif show_text:
        draw_text(painter, config, font, snapshot.text, text_rect, color)

    if snapshot.lap_visible and snapshot.lap_text:
        lap_size = max(8, int(font_size * 0.4))
        lap_font = QFont(config.get("font_family", "Segoe UI"), lap_size, QFont.Weight.Bold)
        metrics = QFontMetrics(font)
        lap_rect = QRectF(draw_rect)
        lap_rect.translate(0, metrics.height() / 2 + local_h * 0.05)
        is_chalk = config.get("theme") == "Chalkboard"
        lap_color = QColor(255, 255, 100, 170) if is_chalk else QColor("#FFFF00")
        draw_text(painter, config, lap_font, snapshot.lap_text, lap_rect, lap_color)

    painter.restore()


def mix_color(start: QColor, end: QColor, t: float) -> QColor:
    t = max(0.0, min(1.0, t))
    return QColor(
        int(start.red() + (end.red() - start.red()) * t),
        int(start.green() + (end.green() - start.green()) * t),
        int(start.blue() + (end.blue() - start.blue()) * t),
        int(start.alpha() + (end.alpha() - start.alpha()) * t),
    )
