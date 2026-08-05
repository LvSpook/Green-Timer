import sys
import time
import json
import os
import math
import keyboard
import ctypes
import platform
import urllib.request
import subprocess
import tempfile
import pkgutil

if platform.system() == "Windows":
    import winsound

from PyQt6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, 
    QDialog, QTabWidget, QFormLayout, QLineEdit, QPushButton, 
    QCheckBox, QComboBox, QColorDialog, QHBoxLayout, QMessageBox,
    QSystemTrayIcon, QMenu, QSlider, QStyle, QFontDialog, QFileDialog
)
from PyQt6.QtCore import Qt, QTimer, pyqtSignal, QObject, QRectF, QThread
from PyQt6.QtGui import QFont, QColor, QPainter, QPen, QAction, QFontMetrics, QPainterPath, QBrush, QPixmap, QImage, QIcon

# --- HELPER FOR PYINSTALLER RESOURCE PATHS ---
def get_resource_path(relative_path):
    """ Get absolute path to resource, works for dev and for PyInstaller """
    try:
        # PyInstaller creates a temp folder and stores path in _MEIPASS
        base_path = sys._MEIPASS
    except Exception:
        base_path = os.path.abspath(".")
    return os.path.join(base_path, relative_path)

# --- APP VERSION & GITHUB REPO CONFIG ---
APP_VERSION = "v1.0.0"
GITHUB_REPO = "LvSpook/Green-Timer"  # CHANGE THIS to your "username/repository"

CONFIG_FILE = "timer_pro_settings.json"

DEFAULT_CONFIG = {
    "mode": "up",              
    "countdown_start": 60.0,   
    "threshold_time": 60.0,    
    "use_threshold": True,
    "font_size": 42,
    "font_family": "Exo",
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
    "auto_check_updates": True
}

# --- AUTO UPDATER THREAD ---
class UpdateCheckerThread(QThread):
    update_available = pyqtSignal(str, str)

    def run(self):
        if GITHUB_REPO == "YourUsername/YourRepoName":
            return
            
        url = f"https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "ProTimerApp"})
            with urllib.request.urlopen(req, timeout=5) as response:
                data = json.loads(response.read().decode())
                latest_tag = data.get("tag_name", "")
                
                if latest_tag and latest_tag != APP_VERSION:
                    assets = data.get("assets", [])
                    download_url = None
                    for asset in assets:
                        if asset.get("name", "").endswith(".exe"):
                            download_url = asset.get("browser_download_url")
                            break
                            
                    if download_url:
                        self.update_available.emit(latest_tag, download_url)
        except Exception:
            pass

# --- WINDOWS BLUR API STRUCTURES ---
if platform.system() == "Windows":
    class ACCENTPOLICY(ctypes.Structure):
        _fields_ = [("AccentState", ctypes.c_uint),
                    ("AccentFlags", ctypes.c_uint),
                    ("GradientColor", ctypes.c_uint),
                    ("AnimationId", ctypes.c_uint)]

    class WINDOWCOMPOSITIONATTRIBDATA(ctypes.Structure):
        _fields_ = [("Attribute", ctypes.c_int),
                    ("Data", ctypes.POINTER(ctypes.c_int)),
                    ("SizeOfData", ctypes.c_size_t)]

class HotkeySignals(QObject):
    toggle = pyqtSignal()
    reset = pyqtSignal()
    settings = pyqtSignal()
    lap = pyqtSignal()
    ghost = pyqtSignal()

# --- CUSTOM PROPORTIONAL SIZE GRIP ---
class ProportionalSizeGrip(QWidget):
    def __init__(self, parent):
        super().__init__(parent)
        self.setFixedSize(16, 16)
        self.setCursor(Qt.CursorShape.SizeFDiagCursor)
        self.dragging = False
        self.start_pos = None
        self.start_size = None

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.dragging = True
            self.start_pos = event.globalPosition().toPoint()
            self.start_size = self.parent().size()
            event.accept()

    def mouseMoveEvent(self, event):
        if self.dragging:
            delta_x = event.globalPosition().toPoint().x() - self.start_pos.x()
            new_w = max(80, self.start_size.width() + delta_x)
            
            rot = self.parent().config.get("rotation", "0°")
            is_vert = rot in ["90°", "270°"]
            
            pad = self.parent().config.get("padding", 20)
            target_text_w = new_w - (pad * 2) if not is_vert else (self.start_size.height() + delta_x) - (pad * 2)
            
            if target_text_w > 10:
                font = QFont(self.parent().config.get("font_family", "Segoe UI"), 12, QFont.Weight.Bold)
                low, high = 1, 500
                best_size = 12
                while low <= high:
                    mid = (low + high) // 2
                    font.setPointSize(mid)
                    fm = QFontMetrics(font)
                    if fm.horizontalAdvance(self.parent().current_text) <= target_text_w:
                        best_size = mid
                        low = mid + 1
                    else:
                        high = mid - 1
                        
                self.parent().current_point_size = best_size
                font.setPointSize(best_size)
                fm = QFontMetrics(font)
                
                b_rect = fm.boundingRect(self.parent().current_text)
                
                calc_w = fm.horizontalAdvance(self.parent().current_text) + (pad * 2)
                calc_h = b_rect.height() + (pad * 2)
                
                if self.parent().lap_label_visible:
                    calc_h += b_rect.height() * 0.4
                    
                if is_vert:
                    self.parent().resize(int(calc_h), int(calc_w))
                else:
                    self.parent().resize(int(calc_w), int(calc_h))
                    
            event.accept()

    def mouseReleaseEvent(self, event):
        self.dragging = False
        self.parent().save_config()
        event.accept()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(QColor(150, 150, 150, 100))
        path = QPainterPath()
        path.moveTo(self.width(), 0)
        path.lineTo(self.width(), self.height())
        path.lineTo(0, self.height())
        painter.drawPath(path)

class ProGreenTimer(QMainWindow):
    def __init__(self):
        super().__init__()
        self.load_config()
        self.signals = HotkeySignals()
        
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint | 
            Qt.WindowType.WindowStaysOnTopHint
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        
        self.running = False
        self.start_time = 0.0
        self.elapsed_time = 0.0
        self.fluid_phase = 0.0
        self.settings_open = False
        self.settings_dialog = None
        
        self.current_text = "00:00.00"
        self.lap_text = ""
        self.lap_label_visible = False
        self.lap_display_until = 0.0
        self.last_time_val = None
        self.current_point_size = self.config.get("font_size", 42)
        
        self.current_color = QColor(self.config["normal_color"])
        
        self.sprite_color = None
        self.sprite_gray = None
        self.load_sprite()

        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        self.layout = QVBoxLayout(self.central_widget)
        self.layout.setSpacing(0)  
        self.layout.setContentsMargins(0, 0, 0, 0)
        
        self.size_grip = ProportionalSizeGrip(self)
        
        self.clock_timer = QTimer(self)
        self.clock_timer.setInterval(7)
        self.clock_timer.timeout.connect(self.update_clock)
        self.clock_timer.start()
        
        self.signals.toggle.connect(self.toggle_timer)
        self.signals.reset.connect(self.reset_timer)
        self.signals.settings.connect(self.open_settings)
        self.signals.lap.connect(self.trigger_lap)
        self.signals.ghost.connect(self.toggle_ghost_mode)
        
        self.setup_tray_icon()
        self.setup_hotkeys()
        self.apply_appearance()
        self.apply_ghost_mode()
        
        if self.config["pos_x"] != -1:
            self.setGeometry(self.config["pos_x"], self.config["pos_y"], 
                             self.config["width"], self.config["height"])
        else:
            self.resize(self.config["width"], self.config["height"])
            
        self.old_pos = None
        self.reset_timer()

        if self.config.get("auto_check_updates", True):
            self.check_for_updates()

    def check_for_updates(self):
        self.updater_thread = UpdateCheckerThread()
        self.updater_thread.update_available.connect(self.prompt_update)
        self.updater_thread.start()

    def prompt_update(self, new_version, download_url):
        reply = QMessageBox.question(
            self,
            "Update Available",
            f"A new version ({new_version}) is available!\nYour current version is {APP_VERSION}.\n\nWould you like to download and install it now?",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No
        )
        if reply == QMessageBox.StandardButton.Yes:
            self.download_and_install_update(download_url)

    def download_and_install_update(self, url):
        try:
            temp_dir = tempfile.gettempdir()
            installer_path = os.path.join(temp_dir, "ProTimer_Setup.exe")
            
            urllib.request.urlretrieve(url, installer_path)
            subprocess.Popen([installer_path])
            QApplication.quit()
        except Exception as e:
            QMessageBox.critical(self, "Update Error", f"Failed to download update: {e}")

    def load_config(self):
        self.config = DEFAULT_CONFIG.copy()
        if os.path.exists(CONFIG_FILE):
            try:
                with open(CONFIG_FILE, "r") as f:
                    saved = json.load(f)
                    for k, v in saved.items():
                        self.config[k] = v
            except Exception: pass

    def load_sprite(self):
        path = self.config.get("sprite_path", "")
        if path and os.path.exists(path):
            self.sprite_color = QPixmap(path)
            if not self.sprite_color.isNull():
                img = self.sprite_color.toImage()
                gray_img = img.convertToFormat(QImage.Format.Format_Grayscale8)
                self.sprite_gray = QPixmap.fromImage(gray_img)
        else:
            self.sprite_color = None
            self.sprite_gray = None
            
    def save_config(self):
        self.config["pos_x"] = self.x()
        self.config["pos_y"] = self.y()
        self.config["width"] = self.width()
        self.config["height"] = self.height()
        self.config["font_size"] = self.current_point_size
        with open(CONFIG_FILE, "w") as f:
            json.dump(self.config, f, indent=4)

    def setup_tray_icon(self):
        self.tray_icon = QSystemTrayIcon(self)
        
        # USE THE PACKAGED ICON FOR THE TRAY
        icon_path = get_resource_path("green_timer.ico")
        if os.path.exists(icon_path):
            self.tray_icon.setIcon(QIcon(icon_path))
        else:
            self.tray_icon.setIcon(self.style().standardIcon(QStyle.StandardPixmap.SP_ComputerIcon))
        
        menu = QMenu()
        act_settings = QAction("Settings", self)
        act_settings.triggered.connect(self.open_settings)
        act_exit = QAction("Exit", self)
        act_exit.triggered.connect(QApplication.instance().quit)
        
        menu.addAction(act_settings)
        menu.addAction(act_exit)
        self.tray_icon.setContextMenu(menu)
        self.tray_icon.setToolTip("Pro Green Timer")
        self.tray_icon.show()

    def setup_hotkeys(self):
        keyboard.unhook_all()
        def on_key(event):
            if event.event_type == keyboard.KEY_DOWN:
                code = event.scan_code
                if code == self.config["hk_toggle"]["code"]: self.signals.toggle.emit()
                elif code == self.config["hk_reset"]["code"]: self.signals.reset.emit()
                elif code == self.config["hk_settings"]["code"]: self.signals.settings.emit()
                elif code == self.config["hk_lap"]["code"]: self.signals.lap.emit()
                elif code == self.config["hk_ghost"]["code"]: self.signals.ghost.emit()
        try: keyboard.hook(on_key)
        except Exception as e: print(f"Keyboard hook error: {e}")

    def apply_windows_blur(self, enable):
        if platform.system() != "Windows": return
        try:
            hwnd = int(self.winId())
            DWMWA_SYSTEMBACKDROP_TYPE = 38
            value = 3 if enable else 1
            res = ctypes.windll.dwmapi.DwmSetWindowAttribute(
                hwnd, DWMWA_SYSTEMBACKDROP_TYPE, ctypes.byref(ctypes.c_int(value)), ctypes.sizeof(ctypes.c_int)
            )
            
            if res != 0 and enable:
                accent = ACCENTPOLICY()
                accent.AccentState = 4 
                accent.GradientColor = 0x01000000 
                data = WINDOWCOMPOSITIONATTRIBDATA()
                data.Attribute = 19 
                data.Data = ctypes.cast(ctypes.pointer(accent), ctypes.POINTER(ctypes.c_int))
                data.SizeOfData = ctypes.sizeof(accent)
                ctypes.windll.user32.SetWindowCompositionAttribute(hwnd, ctypes.byref(data))
            elif res != 0 and not enable:
                accent = ACCENTPOLICY()
                accent.AccentState = 0 
                data = WINDOWCOMPOSITIONATTRIBDATA()
                data.Attribute = 19
                data.Data = ctypes.cast(ctypes.pointer(accent), ctypes.POINTER(ctypes.c_int))
                data.SizeOfData = ctypes.sizeof(accent)
                ctypes.windll.user32.SetWindowCompositionAttribute(hwnd, ctypes.byref(data))
        except Exception: pass

    def apply_appearance(self):
        self.setWindowOpacity(self.config["opacity"])
        self.central_widget.setStyleSheet("") 

        if not self.config.get("show_background", True) or self.config.get("theme") == "Chalkboard":
            self.apply_windows_blur(False)
        else:
            self.apply_windows_blur(self.config.get("window_blur", False))
            
        self.size_grip.setVisible(not self.config["window_locked"] and not self.config["ghost_mode"])
        self.update()

    def update_window_for_text(self):
        font = QFont(self.config.get("font_family", "Segoe UI"), self.current_point_size, QFont.Weight.Bold)
        fm = QFontMetrics(font)
        pad = self.config.get("padding", 20)
        
        b_rect = fm.boundingRect(self.current_text)
        
        calc_w = fm.horizontalAdvance(self.current_text) + (pad * 2)
        calc_h = b_rect.height() + (pad * 2)
        
        if self.lap_label_visible:
            calc_h += b_rect.height() * 0.4
            
        rot = self.config.get("rotation", "0°")
        if rot in ["90°", "270°"]:
            self.resize(int(calc_h), int(calc_w))
        else:
            self.resize(int(calc_w), int(calc_h))
            
        self.save_config()

    def apply_rotation(self, new_rot):
        old_rot = self.config.get("rotation", "0°")
        if old_rot == new_rot: return
        
        is_old_vert = old_rot in ["90°", "270°"]
        is_new_vert = new_rot in ["90°", "270°"]
        
        if is_old_vert != is_new_vert:
            self.resize(self.height(), self.width())
            
        self.config["rotation"] = new_rot
        self.save_config()
        self.update()

    def toggle_ghost_mode(self):
        self.config["ghost_mode"] = not self.config["ghost_mode"]
        self.apply_ghost_mode()

    def apply_ghost_mode(self):
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, self.config["ghost_mode"])
        self.size_grip.setVisible(not self.config["window_locked"] and not self.config["ghost_mode"])
        self.show()

    def mousePressEvent(self, event):
        if self.config["window_locked"] or self.config["ghost_mode"]: return
        if event.button() == Qt.MouseButton.LeftButton:
            self.old_pos = event.globalPosition().toPoint()
        elif event.button() == Qt.MouseButton.RightButton:
            self.open_settings()
            
    def mouseMoveEvent(self, event):
        if self.config["window_locked"] or self.config["ghost_mode"] or self.old_pos is None: return
        delta = event.globalPosition().toPoint() - self.old_pos
        self.move(self.x() + delta.x(), self.y() + delta.y())
        self.old_pos = event.globalPosition().toPoint()
            
    def mouseReleaseEvent(self, event):
        self.old_pos = None
        self.save_config()

    def mouseDoubleClickEvent(self, event):
        if not self.config["ghost_mode"]:
            self.toggle_timer()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self.size_grip.move(self.width() - self.size_grip.width(), self.height() - self.size_grip.height())
        self.apply_appearance() 

    def draw_text_with_outline(self, painter, rect, text, font, forced_color=None):
        is_chalk = self.config.get("theme", "Default") == "Chalkboard"
        path = QPainterPath()
        fm = QFontMetrics(font)
        
        adv = fm.horizontalAdvance(text)
        b_rect = fm.boundingRect(text)
        
        x = rect.center().x() - adv / 2
        y = rect.center().y() - b_rect.height() / 2 - b_rect.y()
        
        path.addText(x, y, font, text)

        if is_chalk:
            chalk_color = forced_color if forced_color else QColor(240, 255, 240, 60)
            painter.setPen(Qt.PenStyle.NoPen)
            for dx, dy in [(-1, -1), (1, 1), (-1, 1), (1, -1), (0, 0), (0, 2), (2, 0)]:
                translated = path.translated(dx, dy)
                painter.fillPath(translated, QBrush(chalk_color))
            return 
            
        if self.config["text_outline"]:
            outline_pen = QPen(QColor(self.config["text_outline_color"]), self.config["text_outline_thickness"])
            outline_pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
            painter.strokePath(path, outline_pen)
            
        painter.fillPath(path, QBrush(forced_color if forced_color else self.current_color))

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing)

        theme = self.config.get("theme", "Default")
        is_chalk = theme == "Chalkboard"
        
        if self.config.get("show_background", True):
            bg_alpha = 100 if self.config.get("window_blur", False) else 140
            bg_color = QColor(0, 0, 0, bg_alpha)

            if is_chalk:
                bg_color = QColor(46, 68, 50, 255)

            shape = self.config["window_shape"]
            b_thick = self.config.get("border_thickness", 1)
            border_style = self.config["border_style"]
            
            bg_rect = QRectF(0, 0, self.width(), self.height())
            
            if border_style != "None":
                bg_rect.adjust(b_thick/2, b_thick/2, -b_thick/2, -b_thick/2)
                
            painter.setBrush(QBrush(bg_color))
            
            if border_style == "Thin":
                border_col = self.config.get("thin_border_color", "#FFFFFF")
                painter.setPen(QPen(QColor(border_col), b_thick))
            elif border_style == "Neon":
                painter.setPen(QPen(self.current_color, b_thick))
            elif is_chalk:
                painter.setPen(QPen(QColor(100, 70, 40, 255), 4)) 
            else:
                painter.setPen(Qt.PenStyle.NoPen)
                
            radius = 0
            if shape == "Rounded":
                radius = self.config.get("corner_radius", 15)
            elif shape == "Capsule":
                radius = min(self.width(), self.height()) / 2
                
            painter.drawRoundedRect(bg_rect, radius, radius)
            
        pad = self.config.get("padding", 20)
        rect = QRectF(pad, pad, self.width() - (pad*2), self.height() - (pad*2))
        
        center = QRectF(0, 0, self.width(), self.height()).center()
        painter.translate(center)
        rot = self.config.get("rotation", "0°")
        if rot == "90°": painter.rotate(90)
        elif rot == "270°": painter.rotate(270)
        
        if rot in ["90°", "270°"]:
            local_w, local_h = rect.height(), rect.width()
        else:
            local_w, local_h = rect.width(), rect.height()
            
        draw_rect = QRectF(-local_w/2, -local_h/2, local_w, local_h)
        
        font = QFont(self.config.get("font_family", "Segoe UI"), self.current_point_size, QFont.Weight.Bold)
        fm = QFontMetrics(font)
        
        text_rect = QRectF(draw_rect)
        if self.lap_label_visible:
            text_rect.translate(0, -(local_h * 0.1))

        time_val = self.elapsed_time
        if self.config["mode"] == "down":
            time_val = self.config["countdown_start"] - self.elapsed_time
            progress = max(0, time_val) / max(1, self.config["countdown_start"])
        else:
            progress = (time_val % 60) / 60.0

        if self.config["display_style"] == "Text":
            if self.config.get("show_text", True):
                self.draw_text_with_outline(painter, text_rect, self.current_text, font)

        elif self.config["display_style"] == "Arc":
            dim = min(local_w, local_h)
            arc_rect = QRectF(-dim/2, -dim/2, dim, dim)
            a_thick = self.config.get("arc_thickness", 12)
            
            painter.setPen(QPen(QColor(40, 40, 40, 150), a_thick, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
            painter.drawArc(arc_rect, 0, 360 * 16)
            
            painter.setPen(QPen(self.current_color, a_thick, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
                
            span_angle = int(-progress * 360 * 16)
            painter.drawArc(arc_rect, 90 * 16, span_angle)
            
            if self.config.get("show_text", True):
                self.draw_text_with_outline(painter, text_rect, self.current_text, font)

        elif self.config["display_style"] == "Bar":
            fill_rect = QRectF(draw_rect.x(), draw_rect.y(), draw_rect.width() * progress, draw_rect.height())
            
            if self.config.get("show_text", True):
                self.draw_text_with_outline(painter, text_rect, self.current_text, font)
            
            container_path = QPainterPath()
            radius = self.config.get("corner_radius", 15) if self.config["window_shape"] != "Rectangular" else 0
            if self.config["window_shape"] == "Capsule": 
                radius = draw_rect.height() / 2
            container_path.addRoundedRect(draw_rect, radius, radius)
            
            clip_path = QPainterPath()
            clip_path.addRect(fill_rect)
            
            final_clip = clip_path.intersected(container_path)
            painter.setClipPath(final_clip)
            
            painter.fillPath(final_clip, self.current_color)
            
            if self.config.get("show_text", True):
                bar_text_col = QColor(self.config.get("bar_text_color", "#FFFFFF"))
                self.draw_text_with_outline(painter, text_rect, self.current_text, font, forced_color=bar_text_col)
            
            painter.setClipping(False)

        elif self.config["display_style"] == "Fluid":
            fluid_h = draw_rect.height() * progress
            base_y = draw_rect.bottom() - fluid_h
            
            fluid_path = QPainterPath()
            fluid_path.moveTo(draw_rect.left(), draw_rect.bottom())
            
            amp = 6
            freq = 0.05
            for x in range(int(draw_rect.left()), int(draw_rect.right()) + 2, 2):
                y = base_y + math.sin(x * freq + self.fluid_phase) * amp
                y = max(draw_rect.top(), min(y, draw_rect.bottom()))
                fluid_path.lineTo(x, y)
                
            fluid_path.lineTo(draw_rect.right(), draw_rect.bottom())
            fluid_path.closeSubpath()
            
            container_path = QPainterPath()
            radius = self.config.get("corner_radius", 15) if self.config["window_shape"] != "Rectangular" else 0
            if self.config["window_shape"] == "Capsule": 
                radius = draw_rect.height() / 2
            container_path.addRoundedRect(draw_rect, radius, radius)
            
            final_clip = fluid_path.intersected(container_path)
            painter.fillPath(final_clip, self.current_color)
            
            if self.config.get("show_text", True):
                self.draw_text_with_outline(painter, text_rect, self.current_text, font)

        elif self.config["display_style"] == "Sprite":
            if self.sprite_color is not None and not self.sprite_color.isNull():
                scaled_color = self.sprite_color.scaled(draw_rect.size().toSize(), Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation)
                scaled_gray = self.sprite_gray.scaled(draw_rect.size().toSize(), Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation)
                
                pix_rect = scaled_color.rect()
                target_rect = QRectF(draw_rect.center().x() - pix_rect.width() / 2, 
                                     draw_rect.center().y() - pix_rect.height() / 2, 
                                     pix_rect.width(), pix_rect.height())

                painter.drawPixmap(target_rect.toRect(), scaled_gray)

                fill_height = target_rect.height() * progress
                clip_rect = QRectF(target_rect.x(), target_rect.bottom() - fill_height, target_rect.width(), fill_height)

                painter.setClipRect(clip_rect)
                painter.drawPixmap(target_rect.toRect(), scaled_color)
                painter.setClipping(False)
            
            if self.config.get("show_text", True):
                self.draw_text_with_outline(painter, text_rect, self.current_text, font)

        if self.lap_label_visible:
            lap_font = QFont(self.config.get("font_family", "Segoe UI"), max(8, int(self.current_point_size * 0.4)), QFont.Weight.Bold)
            lap_rect = QRectF(draw_rect)
            lap_rect.translate(0, fm.boundingRect(self.lap_text).height() / 2 + (local_h * 0.05))
            lap_col = QColor(255, 255, 100, 150) if is_chalk else QColor("#FFFF00")
            self.draw_text_with_outline(painter, lap_rect, self.lap_text, lap_font, forced_color=lap_col)

    def toggle_timer(self):
        if not self.running:
            self.start_time = time.time() - self.elapsed_time
            self.running = True
        else:
            self.elapsed_time = time.time() - self.start_time
            self.running = False
            
    def reset_timer(self):
        self.running = False
        self.elapsed_time = 0.0
        self.lap_text = ""
        self.lap_label_visible = False
        self.current_color = QColor(self.config["normal_color"])
        self.last_time_val = None
        self.update_display(0.0 if self.config["mode"] == "up" else self.config["countdown_start"], is_overdue=False)

    def trigger_lap(self):
        if self.running:
            self.lap_label_visible = True
            self.lap_text = f"LAP: {self.current_text}"
            self.lap_display_until = time.time() + 4.0
            self.update_window_for_text()
            self.update()

    def update_clock(self):
        if self.config["display_style"] == "Fluid":
            self.fluid_phase += 0.05
            self.update()

        if self.running:
            if self.config.get("target_hwnd", 0) != 0:
                try:
                    active_hwnd = ctypes.windll.user32.GetForegroundWindow()
                    if active_hwnd != self.config["target_hwnd"]:
                        self.toggle_timer() 
                except Exception:
                    pass

            self.elapsed_time = time.time() - self.start_time
            
            is_overdue = False
            if self.config["mode"] == "up":
                time_val = self.elapsed_time
            else:
                time_val = self.config["countdown_start"] - self.elapsed_time
                is_overdue = time_val < 0
                
            if self.last_time_val is not None and self.last_time_val >= 0 and time_val < 0:
                if self.config["sound_cue"]:
                    sound_path = self.config.get("custom_sound_path", "")
                    if sound_path and os.path.exists(sound_path) and platform.system() == "Windows":
                        winsound.PlaySound(sound_path, winsound.SND_FILENAME | winsound.SND_ASYNC)
                    else:
                        QApplication.beep()

            self.update_display(time_val, is_overdue)
            self.last_time_val = time_val
            
        if time.time() > self.lap_display_until and self.lap_label_visible:
            self.lap_label_visible = False
            self.lap_text = ""
            self.update_window_for_text()
            self.update()

    def update_display(self, time_val, is_overdue):
        dest_color = QColor(self.config["gray_color"] if is_overdue else self.config["normal_color"])
        
        if self.config["use_threshold"] and not is_overdue:
            if (self.config["mode"] == "up" and time_val >= self.config["threshold_time"]) or \
               (self.config["mode"] == "down" and time_val <= self.config["threshold_time"]):
                dest_color = QColor(self.config["gray_color"])
                
        self.current_color = dest_color

        prec = self.config["decimals"]
        abs_time = abs(time_val)
        rounded_time = round(abs_time, prec) if prec > 0 else round(abs_time)
        
        mins, secs = divmod(rounded_time, 60)
        hours, mins = divmod(mins, 60)
        
        prefix = "+" if is_overdue else ""
        sec_str = f"{secs:0{3+prec}.{prec}f}" if prec > 0 else f"{int(secs):02}"
            
        if hours > 0 or self.config["always_show_hours"]:
            time_str = f"{prefix}{int(hours):02}:{int(mins):02}:{sec_str}"
        else:
            time_str = f"{prefix}{int(mins):02}:{sec_str}"
            
        length_changed = len(time_str) != len(self.current_text)
        self.current_text = time_str
        
        if length_changed:
            self.update_window_for_text()
            
        self.update()

    def open_settings(self):
        if self.settings_open:
            if self.settings_dialog:
                self.settings_dialog.raise_()
                self.settings_dialog.activateWindow()
            return
            
        self.settings_open = True
        self.settings_dialog = SettingsDialog(self, self.config)
        self.settings_dialog.finished.connect(self.on_settings_closed)
        self.settings_dialog.show()

    def on_settings_closed(self, result):
        self.settings_open = False
        if result == QDialog.DialogCode.Accepted:
            self.save_config()
            self.setup_hotkeys()
            if self.config.get('countdown_start') != self.settings_dialog.original_config.get('countdown_start') or self.config.get('mode') != self.settings_dialog.original_config.get('mode'):
                self.reset_timer()
        else:
            self.config = self.settings_dialog.original_config.copy()
            old_rot = self.settings_dialog.original_config.get("rotation", "0°")
            if self.config.get("rotation") != old_rot:
                self.apply_rotation(old_rot)
                
            self.load_sprite()
            self.apply_appearance()
            self.apply_ghost_mode()
            self.update_window_for_text()
            self.update()
            
        self.settings_dialog = None


class SettingsDialog(QDialog):
    key_recorded = pyqtSignal(str, str, int)

    def __init__(self, parent, config):
        super().__init__(parent)
        self.main_window = parent
        self.setWindowTitle(f"Pro Timer Settings ({APP_VERSION})")
        self.resize(480, 780)
        self.setModal(False) 
        self.original_config = config.copy()
        self.recording_hook = None
        
        self.key_recorded.connect(self.on_key_recorded)
        
        layout = QVBoxLayout(self)
        tabs = QTabWidget()
        layout.addWidget(tabs)
        
        # --- TAB 1: General & Time ---
        tab_gen = QWidget()
        form_gen = QFormLayout(tab_gen)
        
        self.cb_mode = QComboBox()
        self.cb_mode.addItems(["up", "down"])
        self.cb_mode.setCurrentText(self.original_config["mode"])
        form_gen.addRow("Timer Mode:", self.cb_mode)
        
        start_str = self.format_time_str(self.original_config["countdown_start"])
        self.in_cd = QLineEdit(start_str)
        self.in_cd.setPlaceholderText("MM:SS or Seconds")
        form_gen.addRow("Countdown Start:", self.in_cd)
        
        self.chk_thresh = QCheckBox("Enable Gray Threshold")
        self.chk_thresh.setChecked(self.original_config["use_threshold"])
        form_gen.addRow("", self.chk_thresh)
        
        self.in_thresh = QLineEdit(str(self.original_config["threshold_time"]))
        form_gen.addRow("Threshold Time (s):", self.in_thresh)
        
        self.cb_dec = QComboBox()
        self.cb_dec.addItems(["0", "1", "2", "3"])
        self.cb_dec.setCurrentText(str(self.original_config["decimals"]))
        form_gen.addRow("Decimal Precision:", self.cb_dec)
        
        self.chk_hours = QCheckBox("Always Show Hours (00:MM:SS)")
        self.chk_hours.setChecked(self.original_config["always_show_hours"])
        form_gen.addRow("", self.chk_hours)
        tabs.addTab(tab_gen, "General")
        
        # --- TAB 2: Appearance & Shapes ---
        tab_app = QWidget()
        self.form_app = QFormLayout(tab_app)
        
        self.cb_theme = QComboBox()
        self.cb_theme.addItems(["Default", "Chalkboard"])
        self.cb_theme.setCurrentText(self.original_config.get("theme", "Default"))
        self.form_app.addRow("Master Theme:", self.cb_theme)

        self.chk_bg = QCheckBox("Enable Background")
        self.chk_bg.setChecked(self.original_config.get("show_background", True))
        self.form_app.addRow("", self.chk_bg)
        
        self.chk_show_text = QCheckBox("Show Timer Text")
        self.chk_show_text.setChecked(self.original_config.get("show_text", True))
        self.form_app.addRow("", self.chk_show_text)

        self.cb_display = QComboBox()
        self.cb_display.addItems(["Text", "Arc", "Bar", "Fluid", "Sprite"])
        self.cb_display.setCurrentText(self.original_config["display_style"])
        self.form_app.addRow("Display Style:", self.cb_display)

        self.sprite_layout = QHBoxLayout()
        self.lbl_sprite_path = QLineEdit(self.original_config.get("sprite_path", ""))
        self.lbl_sprite_path.setPlaceholderText("Select Image...")
        self.btn_sprite = QPushButton("Browse...")
        self.btn_sprite.clicked.connect(self.pick_sprite)
        self.sprite_layout.addWidget(self.lbl_sprite_path)
        self.sprite_layout.addWidget(self.btn_sprite)
        self.lbl_sprite_widget = QWidget()
        self.lbl_sprite_widget.setLayout(self.sprite_layout)
        self.sprite_layout.setContentsMargins(0,0,0,0)
        self.form_app.addRow("Sprite Image:", self.lbl_sprite_widget)
        
        self.cb_shape = QComboBox()
        self.cb_shape.addItems(["Rectangular", "Rounded", "Capsule"])
        self.cb_shape.setCurrentText(self.original_config["window_shape"])
        self.form_app.addRow("Background Shape:", self.cb_shape)
        
        self.sl_padding = QSlider(Qt.Orientation.Horizontal)
        self.sl_padding.setRange(0, 100)
        self.sl_padding.setValue(self.original_config.get("padding", 20))
        self.form_app.addRow("Window Padding:", self.sl_padding)
        
        self.sl_radius = QSlider(Qt.Orientation.Horizontal)
        self.sl_radius.setRange(0, 100)
        self.sl_radius.setValue(self.original_config.get("corner_radius", 15))
        self.form_app.addRow("Corner Radius:", self.sl_radius)
        
        self.sl_arc_thick = QSlider(Qt.Orientation.Horizontal)
        self.sl_arc_thick.setRange(1, 50)
        self.sl_arc_thick.setValue(self.original_config.get("arc_thickness", 12))
        self.form_app.addRow("Arc Thickness:", self.sl_arc_thick)

        self.cb_border = QComboBox()
        self.cb_border.addItems(["None", "Thin", "Neon"])
        self.cb_border.setCurrentText(self.original_config["border_style"])
        self.form_app.addRow("Border Style:", self.cb_border)
        
        self.sl_b_thick = QSlider(Qt.Orientation.Horizontal)
        self.sl_b_thick.setRange(1, 20)
        self.sl_b_thick.setValue(self.original_config.get("border_thickness", 1))
        self.form_app.addRow("Border Thickness:", self.sl_b_thick)
        
        self.chk_blur = QCheckBox("Enable Native OS Blur (Windows)")
        self.chk_blur.setChecked(self.original_config.get("window_blur", False))
        self.form_app.addRow("", self.chk_blur)
        
        self.chk_outline = QCheckBox("Enable Text Outline")
        self.chk_outline.setChecked(self.original_config["text_outline"])
        self.form_app.addRow("", self.chk_outline)
        
        self.sl_outline_thick = QSlider(Qt.Orientation.Horizontal)
        self.sl_outline_thick.setRange(1, 15)
        self.sl_outline_thick.setValue(self.original_config.get("text_outline_thickness", 2))
        self.form_app.addRow("Outline Thickness:", self.sl_outline_thick)
        
        self.sl_opacity = QSlider(Qt.Orientation.Horizontal)
        self.sl_opacity.setRange(10, 100)
        self.sl_opacity.setValue(int(self.original_config["opacity"] * 100))
        self.form_app.addRow("Global Opacity:", self.sl_opacity)
        
        self.btn_font = QPushButton(self.original_config.get("font_family", "Select Font"))
        self.btn_font.clicked.connect(self.pick_font)
        self.form_app.addRow("Timer Font:", self.btn_font)
        
        self.btn_norm = QPushButton("Select Normal Color")
        self.btn_norm.setStyleSheet(f"background-color: {self.original_config['normal_color']}; font-weight: bold; color: black;")
        self.btn_norm.clicked.connect(lambda: self.pick_color("normal_color", self.btn_norm))
        self.form_app.addRow("Normal Color:", self.btn_norm)
        
        self.btn_gray = QPushButton("Select Threshold Color")
        self.btn_gray.setStyleSheet(f"background-color: {self.original_config['gray_color']}; font-weight: bold; color: black;")
        self.btn_gray.clicked.connect(lambda: self.pick_color("gray_color", self.btn_gray))
        self.form_app.addRow("Threshold Color:", self.btn_gray)

        self.btn_outline_col = QPushButton("Select Outline Color")
        self.btn_outline_col.setStyleSheet(f"background-color: {self.original_config.get('text_outline_color', '#000000')}; font-weight: bold; color: white;")
        self.btn_outline_col.clicked.connect(lambda: self.pick_color("text_outline_color", self.btn_outline_col))
        self.form_app.addRow("Outline Color:", self.btn_outline_col)

        self.btn_thin_border = QPushButton("Select Border Color")
        self.btn_thin_border.setStyleSheet(f"background-color: {self.original_config.get('thin_border_color', '#FFFFFF')}; font-weight: bold; color: black;")
        self.btn_thin_border.clicked.connect(lambda: self.pick_color("thin_border_color", self.btn_thin_border))
        self.form_app.addRow("Window Border Color:", self.btn_thin_border)
        
        self.btn_bar_text = QPushButton("Select Filled Text Color")
        self.btn_bar_text.setStyleSheet(f"background-color: {self.original_config.get('bar_text_color', '#FFFFFF')}; font-weight: bold; color: black;")
        self.btn_bar_text.clicked.connect(lambda: self.pick_color("bar_text_color", self.btn_bar_text))
        self.form_app.addRow("Bar Filled Text:", self.btn_bar_text)
        
        tabs.addTab(tab_app, "Appearance")

        # --- TAB 3: Behavior & Position ---
        tab_beh = QWidget()
        form_beh = QFormLayout(tab_beh)
        
        self.cb_rot = QComboBox()
        self.cb_rot.addItems(["0°", "90°", "270°"])
        self.cb_rot.setCurrentText(self.original_config.get("rotation", "0°"))
        form_beh.addRow("Window Rotation:", self.cb_rot)
        
        self.chk_lock = QCheckBox("Lock Window Position (Prevent Dragging)")
        self.chk_lock.setChecked(self.original_config["window_locked"])
        form_beh.addRow("", self.chk_lock)
        
        self.chk_sound = QCheckBox("Enable Sound Cue on Zero")
        self.chk_sound.setChecked(self.original_config["sound_cue"])
        form_beh.addRow("", self.chk_sound)
        
        self.chk_auto_update = QCheckBox("Automatically Check for Updates on Launch")
        self.chk_auto_update.setChecked(self.original_config.get("auto_check_updates", True))
        form_beh.addRow("", self.chk_auto_update)

        sound_layout = QHBoxLayout()
        self.lbl_sound_path = QLineEdit(self.original_config.get("custom_sound_path", ""))
        self.lbl_sound_path.setPlaceholderText("Default Beep")
        self.btn_sound_path = QPushButton("Browse...")
        self.btn_sound_path.clicked.connect(self.pick_sound)
        sound_layout.addWidget(self.lbl_sound_path)
        sound_layout.addWidget(self.btn_sound_path)
        form_beh.addRow("Custom Sound (.wav):", sound_layout)
        
        pos_layout = QHBoxLayout()
        for pos in ["Top-Right", "Top-Center", "Bottom-Center"]:
            btn = QPushButton(pos.split('-')[0][:1] + pos.split('-')[1][:1])
            btn.setToolTip(pos)
            btn.clicked.connect(lambda ch, p=pos: self.snap_position(p))
            pos_layout.addWidget(btn)
        form_beh.addRow("Snap Position:", pos_layout)
        
        self.btn_focus = QPushButton("Click to set Target App (5s delay)")
        self.btn_focus.clicked.connect(self.start_focus_capture)
        form_beh.addRow("Auto-Pause Target:", self.btn_focus)
        
        self.btn_manual_update = QPushButton("Check for Updates Now")
        self.btn_manual_update.clicked.connect(self.main_window.check_for_updates)
        form_beh.addRow("Software Update:", self.btn_manual_update)

        tabs.addTab(tab_beh, "Behavior")
        
        # --- TAB 4: Hotkeys ---
        tab_keys = QWidget()
        form_keys = QFormLayout(tab_keys)
        
        self.hk_widgets = {}
        key_list = [("hk_toggle", "Start/Pause:"), ("hk_reset", "Reset:"), 
                    ("hk_settings", "Settings:"), ("hk_lap", "Trigger Lap:"),
                    ("hk_ghost", "Toggle Ghost Mode:")]
                    
        for key, label in key_list:
            row_layout = QHBoxLayout()
            lbl_display = QLineEdit(f"{self.original_config[key]['name']} (Scan: {self.original_config[key]['code']})")
            lbl_display.setReadOnly(True)
            btn_rec = QPushButton("Record")
            btn_rec.clicked.connect(lambda ch, k=key: self.record_key(k))
            
            row_layout.addWidget(lbl_display)
            row_layout.addWidget(btn_rec)
            form_keys.addRow(label, row_layout)
            
            self.hk_widgets[key] = {
                "display": lbl_display, 
                "button": btn_rec, 
                "data": self.original_config[key].copy()
            }
            
        tabs.addTab(tab_keys, "Hotkeys")
        
        # --- Actions Bottom Row ---
        btn_layout = QHBoxLayout()
        btn_save = QPushButton("Save && Apply")
        btn_save.clicked.connect(self.accept)
        btn_cancel = QPushButton("Cancel")
        btn_cancel.clicked.connect(self.reject)
        
        btn_reset = QPushButton("Reset to Defaults")
        btn_reset.setStyleSheet("color: #FF5555;")
        btn_reset.clicked.connect(self.reset_to_defaults)

        btn_quit = QPushButton("Quit Timer")
        btn_quit.setStyleSheet("color: white; background-color: #AA0000; font-weight: bold;")
        btn_quit.clicked.connect(QApplication.instance().quit)
        
        btn_layout.addWidget(btn_save)
        btn_layout.addWidget(btn_cancel)
        btn_layout.addWidget(btn_reset)
        btn_layout.addWidget(btn_quit)
        layout.addLayout(btn_layout)

        self.cb_mode.currentTextChanged.connect(self.live_update)
        self.in_cd.textChanged.connect(self.live_update)
        self.chk_thresh.stateChanged.connect(self.live_update)
        self.in_thresh.textChanged.connect(self.live_update)
        self.cb_dec.currentTextChanged.connect(self.live_update)
        self.chk_hours.stateChanged.connect(self.live_update)
        self.cb_theme.currentTextChanged.connect(self.live_update)
        self.chk_bg.stateChanged.connect(self.live_update)
        self.chk_show_text.stateChanged.connect(self.live_update)
        self.cb_display.currentTextChanged.connect(self.live_update)
        self.cb_shape.currentTextChanged.connect(self.live_update)
        self.sl_padding.valueChanged.connect(self.live_update)
        self.sl_radius.valueChanged.connect(self.live_update)
        self.sl_arc_thick.valueChanged.connect(self.live_update)
        self.cb_border.currentTextChanged.connect(self.live_update)
        self.sl_b_thick.valueChanged.connect(self.live_update)
        self.chk_blur.stateChanged.connect(self.live_update)
        self.chk_outline.stateChanged.connect(self.live_update)
        self.sl_outline_thick.valueChanged.connect(self.live_update)
        self.sl_opacity.valueChanged.connect(self.live_update)
        self.chk_lock.stateChanged.connect(self.live_update)
        self.chk_sound.stateChanged.connect(self.live_update)
        self.chk_auto_update.stateChanged.connect(self.live_update)
        self.cb_rot.currentTextChanged.connect(self.live_update)
        self.lbl_sprite_path.textChanged.connect(self.live_update)

        self.update_conditional_settings()

    def update_conditional_settings(self):
        display_style = self.cb_display.currentText()
        border_style = self.cb_border.currentText()

        def toggle_row(widget, is_visible):
            widget.setVisible(is_visible)
            lbl = self.form_app.labelForField(widget)
            if lbl: lbl.setVisible(is_visible)

        toggle_row(self.sl_arc_thick, display_style == "Arc")
        toggle_row(self.btn_bar_text, display_style == "Bar")
        toggle_row(self.lbl_sprite_widget, display_style == "Sprite")
        
        toggle_row(self.sl_b_thick, border_style != "None")
        toggle_row(self.btn_thin_border, border_style == "Thin")

    def live_update(self, *args):
        try:
            self.update_conditional_settings()
            
            old_dec = self.main_window.config["decimals"]
            new_dec = int(self.cb_dec.currentText())
            old_hr = self.main_window.config["always_show_hours"]
            new_hr = self.chk_hours.isChecked()
            format_changed = (old_dec != new_dec) or (old_hr != new_hr)
            
            old_rot = self.main_window.config.get("rotation", "0°")
            new_rot = self.cb_rot.currentText()
            
            old_pad = self.main_window.config.get("padding", 20)
            new_pad = self.sl_padding.value()
            pad_changed = old_pad != new_pad

            old_sprite = self.main_window.config.get("sprite_path", "")
            new_sprite = self.lbl_sprite_path.text()
            
            self.main_window.config["mode"] = self.cb_mode.currentText()
            self.main_window.config["countdown_start"] = self.parse_time_input(self.in_cd.text())
            self.main_window.config["use_threshold"] = self.chk_thresh.isChecked()
            self.main_window.config["threshold_time"] = float(self.in_thresh.text() or 0)
            self.main_window.config["decimals"] = new_dec
            self.main_window.config["always_show_hours"] = new_hr
            self.main_window.config["theme"] = self.cb_theme.currentText()
            self.main_window.config["show_text"] = self.chk_show_text.isChecked()
            self.main_window.config["display_style"] = self.cb_display.currentText()
            self.main_window.config["sprite_path"] = new_sprite
            self.main_window.config["window_shape"] = self.cb_shape.currentText()
            self.main_window.config["padding"] = new_pad
            self.main_window.config["corner_radius"] = self.sl_radius.value()
            self.main_window.config["arc_thickness"] = self.sl_arc_thick.value()
            self.main_window.config["show_background"] = self.chk_bg.isChecked()
            self.main_window.config["window_blur"] = self.chk_blur.isChecked()
            self.main_window.config["border_style"] = self.cb_border.currentText()
            self.main_window.config["border_thickness"] = self.sl_b_thick.value()
            self.main_window.config["text_outline"] = self.chk_outline.isChecked()
            self.main_window.config["text_outline_thickness"] = self.sl_outline_thick.value()
            self.main_window.config["opacity"] = self.sl_opacity.value() / 100.0
            self.main_window.config["window_locked"] = self.chk_lock.isChecked()
            self.main_window.config["sound_cue"] = self.chk_sound.isChecked()
            self.main_window.config["auto_check_updates"] = self.chk_auto_update.isChecked()
            self.main_window.config["custom_sound_path"] = self.lbl_sound_path.text()
            
            for k, v in self.hk_widgets.items():
                self.main_window.config[k] = v["data"]

            if old_sprite != new_sprite:
                self.main_window.load_sprite()
                
            if old_rot != new_rot:
                self.main_window.apply_rotation(new_rot)

            self.main_window.update_display(self.main_window.last_time_val if self.main_window.last_time_val is not None else 0.0, False)
            
            if format_changed or pad_changed:
                self.main_window.update_window_for_text()
                
            self.main_window.apply_appearance()
            self.main_window.apply_ghost_mode()

        except ValueError:
            pass 

    def format_time_str(self, seconds):
        mins = int(seconds // 60)
        secs = seconds % 60
        if mins > 0:
            return f"{mins:02}:{secs:05.2f}" if secs % 1 != 0 else f"{mins:02}:{int(secs):02}"
        return str(seconds)

    def parse_time_input(self, text):
        try:
            if ":" in text:
                parts = text.split(":")
                return int(parts[0]) * 60 + float(parts[1])
            return float(text)
        except Exception:
            return 60.0

    def snap_position(self, mode):
        screen = QApplication.primaryScreen().availableGeometry()
        w = self.main_window.width()
        h = self.main_window.height()
        if mode == "Top-Right":
            self.main_window.move(screen.width() - w, 0)
        elif mode == "Top-Center":
            self.main_window.move((screen.width() - w) // 2, 0)
        elif mode == "Bottom-Center":
            self.main_window.move((screen.width() - w) // 2, screen.height() - h)
        self.main_window.save_config()

    def start_focus_capture(self):
        self.btn_focus.setText("Switch to target App NOW...")
        self.btn_focus.setEnabled(False)
        QTimer.singleShot(5000, self.capture_hwnd)
        
    def capture_hwnd(self):
        try:
            hwnd = ctypes.windll.user32.GetForegroundWindow()
            self.main_window.config["target_hwnd"] = hwnd
            self.btn_focus.setText(f"App Captured (HWND: {hwnd})")
        except Exception:
            self.btn_focus.setText("Windows OS Required")
        self.btn_focus.setEnabled(True)

    def on_key_recorded(self, key_name, name, code):
        self.hk_widgets[key_name]["data"] = {"name": name, "code": code}
        display_widget = self.hk_widgets[key_name]["display"]
        btn_widget = self.hk_widgets[key_name]["button"]
        display_widget.setText(f"{name} (Scan: {code})")
        btn_widget.setText("Record")
        btn_widget.setEnabled(True)
        self.live_update()

    def record_key(self, key_name):
        if self.recording_hook:
            try: keyboard.unhook(self.recording_hook)
            except Exception: pass
            
        btn_widget = self.hk_widgets[key_name]["button"]
        btn_widget.setText("Press any key...")
        btn_widget.setEnabled(False)
        
        def on_event(event):
            if event.event_type == keyboard.KEY_DOWN:
                try: keyboard.unhook(self.recording_hook)
                except Exception: pass
                self.recording_hook = None
                code = event.scan_code
                name = event.name.lower()
                if event.is_keypad: name = f"numpad {name}"
                
                self.key_recorded.emit(key_name, name, code)
                
        self.recording_hook = keyboard.hook(on_event)

    def pick_color(self, config_key, btn_widget):
        color = QColorDialog.getColor(QColor(self.main_window.config[config_key]), self, "Select Color")
        if color.isValid():
            self.main_window.config[config_key] = color.name()
            btn_text_col = "white" if config_key == "text_outline_color" and color.lightness() < 128 else "black"
            btn_widget.setStyleSheet(f"background-color: {color.name()}; font-weight: bold; color: {btn_text_col};")
            self.live_update()

    def pick_font(self):
        current_font = QFont(self.main_window.config.get("font_family", "Segoe UI"))
        font, ok = QFontDialog.getFont(current_font, self, "Select Timer Font")
        if ok:
            self.main_window.config["font_family"] = font.family()
            self.btn_font.setText(font.family())
            self.live_update()

    def pick_sound(self):
        path, _ = QFileDialog.getOpenFileName(self, "Select WAV Sound", "", "WAV Files (*.wav)")
        if path:
            self.lbl_sound_path.setText(path)
            self.live_update()

    def pick_sprite(self):
        path, _ = QFileDialog.getOpenFileName(self, "Select Sprite Image", "", "Images (*.png *.jpg *.jpeg *.bmp)")
        if path:
            self.lbl_sprite_path.setText(path)
            self.live_update()

    def reset_to_defaults(self):
        reply = QMessageBox.question(
            self, "Reset Defaults", 
            "Are you sure you want to completely reset all settings to their defaults?", 
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No
        )
        if reply == QMessageBox.StandardButton.Yes:
            self.main_window.config = DEFAULT_CONFIG.copy()
            self.accept() 

    def closeEvent(self, event):
        if self.recording_hook:
            try: keyboard.unhook(self.recording_hook)
            except Exception: pass
        super().closeEvent(event)

if __name__ == "__main__":
    app = QApplication(sys.argv)
    
    # Enable High DPI scaling
    if hasattr(Qt.ApplicationAttribute, 'AA_UseHighDpiPixmaps'):
        app.setAttribute(Qt.ApplicationAttribute.AA_UseHighDpiPixmaps)
        
    # --- LOAD THE ICON FOR WINDOW AND TASKBAR ---
    icon_path = get_resource_path("green_timer.ico")
    if os.path.exists(icon_path):
        app.setWindowIcon(QIcon(icon_path))
        
    timer = ProGreenTimer()
    timer.show()
    sys.exit(app.exec())