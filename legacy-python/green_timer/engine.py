"""Framework-independent timer engine.

The engine owns all state that affects what the user sees: whether the clock is
running, how much time has elapsed, the pomodoro phase, the lap banner and the
colour state. The Qt layer only renders the snapshot it receives, which keeps
the tricky logic unit-testable without a display.

Timing uses a monotonic clock so wall-clock adjustments (NTP sync, DST, manual
changes) never make the timer jump or run backwards.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional

from .formatting import format_duration

MODE_UP = "up"
MODE_DOWN = "down"
MODE_CLOCK = "clock"
MODE_POMODORO = "pomodoro"

PHASE_WORK = "work"
PHASE_BREAK = "break"
PHASE_LONG_BREAK = "long_break"

COLOR_NORMAL = "normal"
COLOR_THRESHOLD = "threshold"
COLOR_OVERDUE = "overdue"

EVENT_CUE = "cue"
EVENT_PHASE_CHANGED = "phase_changed"
EVENT_LAP = "lap"
EVENT_OVERDUE = "overdue"

LAP_SECONDS = 4.0


@dataclass
class Snapshot:
    """Everything the view needs for one repaint."""

    text: str = "00:00.00"
    progress: float = 0.0
    color_role: str = COLOR_NORMAL
    time_value: float = 0.0
    running: bool = False
    overdue: bool = False
    phase: str = PHASE_WORK
    lap_text: str = ""
    lap_visible: bool = False
    #: True when the length of ``text`` grew, so the window may need resizing.
    text_length_changed: bool = False
    text_changed: bool = False
    events: List[str] = field(default_factory=list)


class TimerEngine:
    """State machine for count-up, countdown, clock and pomodoro modes."""

    def __init__(self, config: Dict[str, Any], clock: Callable[[], float] = time.monotonic):
        self._config = config
        self._clock = clock
        self._running = False
        self._started_at = 0.0
        self._accumulated = 0.0
        self._phase = PHASE_WORK
        self._cycle_count = 0
        self._lap_text = ""
        self._lap_until = 0.0
        self._last_char_count = 0
        self._last_text = ""
        self._last_time_value: Optional[float] = None
        self._last_snapshot = Snapshot()
        self.fluid_phase = 0.0

    # -- configuration ---------------------------------------------------
    def update_config(self, config: Dict[str, Any]) -> None:
        self._config = config

    @property
    def running(self) -> bool:
        return self._running

    @property
    def phase(self) -> str:
        return self._phase

    @property
    def cycle_count(self) -> int:
        return self._cycle_count

    @property
    def elapsed(self) -> float:
        if self._running:
            return (self._clock() - self._started_at) + self._accumulated
        return self._accumulated

    def phase_duration(self, phase: Optional[str] = None) -> float:
        phase = phase or self._phase
        cfg = self._config
        if phase == PHASE_WORK:
            return float(cfg.get("pomo_work", 25.0)) * 60.0
        if phase == PHASE_BREAK:
            return float(cfg.get("pomo_break", 5.0)) * 60.0
        return float(cfg.get("pomo_long_break", 15.0)) * 60.0

    # -- control ---------------------------------------------------------
    def start(self) -> None:
        if self._running or self._config.get("mode") == MODE_CLOCK:
            return
        self._started_at = self._clock()
        self._running = True

    def pause(self) -> None:
        if not self._running:
            return
        self._accumulated = self.elapsed
        self._running = False

    def toggle(self) -> None:
        if self._running:
            self.pause()
        else:
            self.start()

    def reset(self) -> None:
        self._running = False
        self._accumulated = 0.0
        self._started_at = 0.0
        self._phase = PHASE_WORK
        self._cycle_count = 0
        self._lap_text = ""
        self._lap_until = 0.0
        self._last_time_value = None
        self._last_char_count = 0
        self._last_text = ""

    def set_elapsed(self, seconds: float) -> None:
        """Move the elapsed counter (used when toggling during a session)."""
        self._accumulated = max(0.0, float(seconds))
        self._started_at = self._clock()

    def trigger_lap(self) -> bool:
        """Show a lap banner. Returns ``True`` if the lap was accepted."""
        if not self._running or self._config.get("mode") == MODE_CLOCK:
            return False
        snapshot = self._last_snapshot
        reference = snapshot.text if snapshot.text else self._format_value(0.0, False)
        self._lap_text = f"LAP: {reference}"
        self._lap_until = self._clock() + LAP_SECONDS
        return True

    # -- rendering helpers ----------------------------------------------
    def _format_value(self, time_val: float, overdue: bool) -> str:
        # Overdue countdowns are shown as positive time with a leading "+",
        # never as a negative number, matching the original behaviour.
        decimals = int(self._config.get("decimals", 2))
        show_hours = bool(self._config.get("always_show_hours", False))
        if overdue and time_val < 0:
            return "+" + format_duration(-time_val, decimals, show_hours)
        return format_duration(time_val, decimals, show_hours)

    def _color_role(self, time_val: float, overdue: bool) -> str:
        cfg = self._config
        if overdue:
            return COLOR_OVERDUE
        mode = cfg.get("mode")
        if cfg.get("use_threshold", True) and mode in (MODE_UP, MODE_DOWN):
            threshold = float(cfg.get("threshold_time", 60.0))
            if mode == MODE_UP and time_val >= threshold:
                return COLOR_THRESHOLD
            if mode == MODE_DOWN and time_val <= threshold:
                return COLOR_THRESHOLD
        return COLOR_NORMAL

    # -- main update -----------------------------------------------------
    def tick(self) -> Snapshot:
        """Advance state to the current time and return a fresh snapshot."""
        cfg = self._config
        now = self._clock()
        events: List[str] = []
        mode = cfg.get("mode", MODE_UP)

        if mode == MODE_CLOCK:
            text = time.strftime(cfg.get("clock_format", "%H:%M:%S"))
            secs = time.localtime().tm_sec
            return self._build(text, secs / 60.0, COLOR_NORMAL, 0.0, False, events)

        overdue = False
        if mode == MODE_POMODORO:
            time_val = self.phase_duration() - self.elapsed
            if time_val <= 0:
                events.append(EVENT_CUE)
                if self._phase == PHASE_WORK:
                    self._cycle_count += 1
                    cycles = max(1, int(cfg.get("pomo_cycles", 4)))
                    self._phase = PHASE_LONG_BREAK if self._cycle_count % cycles == 0 else PHASE_BREAK
                else:
                    self._phase = PHASE_WORK
                events.append(EVENT_PHASE_CHANGED)
                self._accumulated = 0.0
                self._started_at = now
                time_val = self.phase_duration()
                self._lap_text = f"AUTO: {self._phase.replace('_', ' ').upper()}"
                self._lap_until = now + LAP_SECONDS
                events.append(EVENT_LAP)
            progress = max(0.0, time_val) / max(1e-6, self.phase_duration())
        elif mode == MODE_DOWN:
            time_val = float(cfg.get("countdown_start", 60.0)) - self.elapsed
            overdue = time_val < 0
            if overdue and (self._last_time_value is not None and self._last_time_value >= 0):
                events.append(EVENT_OVERDUE)
                events.append(EVENT_CUE)
            progress = max(0.0, time_val) / max(1e-6, float(cfg.get("countdown_start", 60.0)))
        else:
            time_val = self.elapsed
            progress = (time_val % 60.0) / 60.0

        self._last_time_value = time_val
        text = self._format_value(time_val, overdue)
        role = self._color_role(time_val, overdue)
        return self._build(text, progress, role, time_val, overdue, events)

    def _build(
        self,
        text: str,
        progress: float,
        role: str,
        time_val: float,
        overdue: bool,
        events: List[str],
    ) -> Snapshot:
        now = self._clock()
        lap_visible = bool(self._lap_text) and now < self._lap_until
        if not lap_visible:
            self._lap_text = ""

        text_changed = text != self._last_text
        length_changed = len(text) != self._last_char_count
        self._last_text = text
        self._last_char_count = len(text)

        snapshot = Snapshot(
            text=text,
            progress=max(0.0, min(1.0, progress)),
            color_role=role,
            time_value=time_val,
            running=self._running,
            overdue=overdue,
            phase=self._phase,
            lap_text=self._lap_text,
            lap_visible=lap_visible,
            text_length_changed=length_changed,
            text_changed=text_changed,
            events=events,
        )
        self._last_snapshot = snapshot
        return snapshot

    def current_text(self) -> str:
        """The most recently rendered text (used for lap labels/resizing)."""
        if self._last_text:
            return self._last_text
        mode = self._config.get("mode", MODE_UP)
        if mode == MODE_CLOCK:
            return time.strftime(self._config.get("clock_format", "%H:%M:%S"))
        if mode == MODE_DOWN:
            value = float(self._config.get("countdown_start", 60.0))
        elif mode == MODE_POMODORO:
            value = self.phase_duration()
        else:
            value = 0.0
        text = self._format_value(value, False)
        self._last_text = text
        self._last_char_count = len(text)
        return text
