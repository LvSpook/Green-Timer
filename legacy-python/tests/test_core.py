"""Unit tests for the Qt-independent core.

Run with:  python -m pytest -q     (or  python -m unittest discover -s tests)
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from green_timer import config as config_module
from green_timer.config import (
    DEFAULT_CONFIG,
    load_config,
    normalize_config,
    save_config,
)
from green_timer.engine import (
    COLOR_NORMAL,
    COLOR_OVERDUE,
    COLOR_THRESHOLD,
    EVENT_CUE,
    EVENT_PHASE_CHANGED,
    MODE_CLOCK,
    MODE_DOWN,
    MODE_POMODORO,
    MODE_UP,
    PHASE_BREAK,
    PHASE_LONG_BREAK,
    PHASE_WORK,
    TimerEngine,
)
from green_timer.formatting import format_duration, format_input, parse_duration
from green_timer.updater import is_newer, parse_version


class FakeClock:
    def __init__(self, start: float = 1000.0):
        self.now = start

    def __call__(self) -> float:
        return self.now

    def advance(self, seconds: float) -> None:
        self.now += seconds


def make_engine(**overrides) -> tuple[TimerEngine, FakeClock]:
    cfg = normalize_config({**DEFAULT_CONFIG, **overrides})
    clock = FakeClock()
    return TimerEngine(cfg, clock), clock


class FormattingTests(unittest.TestCase):
    def test_basic_durations(self):
        self.assertEqual(format_duration(0, 2), "00:00.00")
        self.assertEqual(format_duration(65.5, 2), "01:05.50")
        self.assertEqual(format_duration(3599, 0), "59:59")
        self.assertEqual(format_duration(3600, 0), "01:00:00")

    def test_rounding_does_not_show_invalid_fraction(self):
        self.assertEqual(format_duration(59.999, 2), "01:00.00")
        self.assertEqual(format_duration(9.999, 2), "00:10.00")

    def test_negative_and_signed(self):
        self.assertEqual(format_duration(-5, 0), "-00:05")
        self.assertEqual(format_duration(5, 0, show_sign=True), "+00:05")

    def test_always_show_hours(self):
        self.assertEqual(format_duration(30, 0, show_hours=True), "00:00:30")

    def test_parse_accepts_common_inputs(self):
        self.assertEqual(parse_duration("90"), 90.0)
        self.assertEqual(parse_duration("1:30"), 90.0)
        self.assertEqual(parse_duration("01:02:03.5"), 3723.5)
        self.assertEqual(parse_duration(" 2.5 "), 2.5)

    def test_parse_rejects_invalid(self):
        for bad in ("", "abc", "-5", "1:99", "1:2:3:4", "1::2"):
            with self.assertRaises(ValueError, msg=bad):
                parse_duration(bad)

    def test_format_input_roundtrip(self):
        for value in (0, 45, 60, 90.5, 3600, 3725):
            self.assertAlmostEqual(parse_duration(format_input(value)), value, places=2)


class ConfigTests(unittest.TestCase):
    def test_defaults_are_valid(self):
        self.assertEqual(normalize_config(DEFAULT_CONFIG), DEFAULT_CONFIG)

    def test_invalid_values_fall_back(self):
        cfg = normalize_config({
            "mode": "nonsense",
            "decimals": 99,
            "opacity": 5,
            "padding": -20,
            "hk_toggle": "bad",
            "unknown_key": 1,
            "normal_color": "a\nb",
        })
        self.assertEqual(cfg["mode"], "up")
        self.assertEqual(cfg["decimals"], 3)
        self.assertEqual(cfg["opacity"], 1.0)
        self.assertEqual(cfg["padding"], 0)
        self.assertEqual(cfg["hk_toggle"], DEFAULT_CONFIG["hk_toggle"])
        self.assertNotIn("unknown_key", cfg)
        self.assertEqual(cfg["normal_color"], "#00FF66")

    def test_bool_coercion(self):
        self.assertFalse(normalize_config({"sound_cue": "false"})["sound_cue"])
        self.assertTrue(normalize_config({"sound_cue": "yes"})["sound_cue"])

    def test_non_dict_input_returns_defaults(self):
        self.assertEqual(normalize_config(["nope"]), DEFAULT_CONFIG)
        self.assertEqual(normalize_config(None), DEFAULT_CONFIG)

    def test_save_and_load_roundtrip(self):
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.object(config_module, "config_dir", return_value=tmp), \
                 mock.patch.object(config_module, "legacy_config_path", return_value=os.path.join(tmp, "none")):
                cfg = normalize_config({"pomo_work": 12.5, "mode": "down"})
                self.assertTrue(save_config(cfg))
                self.assertTrue(os.path.exists(os.path.join(tmp, "timer_pro_settings.json")))
                loaded = load_config()
                self.assertEqual(loaded["pomo_work"], 12.5)
                self.assertEqual(loaded["mode"], "down")

    def test_corrupt_file_uses_defaults(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "timer_pro_settings.json")
            with open(path, "w", encoding="utf-8") as handle:
                handle.write("{ not json")
            with mock.patch.object(config_module, "config_dir", return_value=tmp), \
                 mock.patch.object(config_module, "legacy_config_path", return_value=os.path.join(tmp, "none")), \
                 self.assertLogs("green_timer.config", level="WARNING"):
                self.assertEqual(load_config(), DEFAULT_CONFIG)

    def test_legacy_migration(self):
        with tempfile.TemporaryDirectory() as new_dir, tempfile.TemporaryDirectory() as old_dir:
            legacy = os.path.join(old_dir, "timer_pro_settings.json")
            with open(legacy, "w", encoding="utf-8") as handle:
                handle.write('{"mode": "pomodoro", "pomo_work": 30}')
            with mock.patch.object(config_module, "config_dir", return_value=new_dir), \
                 mock.patch.object(config_module, "legacy_config_path", return_value=legacy):
                loaded = load_config()
                self.assertEqual(loaded["mode"], "pomodoro")
                self.assertEqual(loaded["pomo_work"], 30.0)
                self.assertTrue(os.path.exists(os.path.join(new_dir, "timer_pro_settings.json")))


class EngineTests(unittest.TestCase):
    def test_count_up_tracks_elapsed(self):
        engine, clock = make_engine(mode=MODE_UP)
        engine.start()
        clock.advance(2.5)
        snap = engine.tick()
        self.assertEqual(snap.text, "00:02.50")
        self.assertTrue(snap.running)
        engine.pause()
        clock.advance(100)
        self.assertEqual(engine.tick().text, "00:02.50")

    def test_count_down_overdue_emits_cue_once(self):
        engine, clock = make_engine(mode=MODE_DOWN, countdown_start=10)
        engine.start()
        clock.advance(5)
        self.assertEqual(engine.tick().text, "00:05.00")
        clock.advance(6)
        snap = engine.tick()
        self.assertTrue(snap.overdue)
        self.assertEqual(snap.text, "+00:01.00")
        self.assertIn(EVENT_CUE, snap.events)
        clock.advance(1)
        self.assertNotIn(EVENT_CUE, engine.tick().events)

    def test_threshold_colour_roles(self):
        engine, clock = make_engine(mode=MODE_UP, use_threshold=True, threshold_time=5)
        engine.start()
        clock.advance(2)
        self.assertEqual(engine.tick().color_role, COLOR_NORMAL)
        clock.advance(5)
        self.assertEqual(engine.tick().color_role, COLOR_THRESHOLD)

        down, down_clock = make_engine(mode=MODE_DOWN, countdown_start=10,
                                       use_threshold=True, threshold_time=4)
        down.start()
        down_clock.advance(7)
        self.assertEqual(down.tick().color_role, COLOR_THRESHOLD)
        down_clock.advance(5)
        self.assertEqual(down.tick().color_role, COLOR_OVERDUE)

    def test_pomodoro_cycles_work_break_long_break(self):
        engine, clock = make_engine(mode=MODE_POMODORO, pomo_work=1, pomo_break=1,
                                    pomo_long_break=1, pomo_cycles=2)
        engine.start()
        self.assertEqual(engine.phase, PHASE_WORK)

        clock.advance(61)
        snap = engine.tick()
        self.assertEqual(snap.phase, PHASE_BREAK)
        self.assertIn(EVENT_PHASE_CHANGED, snap.events)
        # The next phase starts from a full duration.
        self.assertEqual(snap.text, "01:00.00")

        clock.advance(61)
        self.assertEqual(engine.tick().phase, PHASE_WORK)
        self.assertEqual(engine.cycle_count, 1)

        clock.advance(61)
        self.assertEqual(engine.tick().phase, PHASE_LONG_BREAK)
        self.assertEqual(engine.cycle_count, 2)

        clock.advance(61)
        self.assertEqual(engine.tick().phase, PHASE_WORK)

    def test_reset_clears_state(self):
        engine, clock = make_engine(mode=MODE_POMODORO, pomo_work=1)
        engine.start()
        clock.advance(61)
        engine.tick()
        engine.reset()
        self.assertFalse(engine.running)
        self.assertEqual(engine.phase, PHASE_WORK)
        self.assertEqual(engine.cycle_count, 0)
        self.assertEqual(engine.elapsed, 0.0)

    def test_lap_only_when_running(self):
        engine, clock = make_engine(mode=MODE_UP)
        self.assertFalse(engine.trigger_lap())
        engine.start()
        clock.advance(1)
        engine.tick()
        self.assertTrue(engine.trigger_lap())
        snap = engine.tick()
        self.assertTrue(snap.lap_visible)
        self.assertIn("LAP:", snap.lap_text)
        clock.advance(5)
        self.assertFalse(engine.tick().lap_visible)

    def test_clock_mode_never_runs(self):
        engine, _ = make_engine(mode=MODE_CLOCK)
        engine.start()
        self.assertFalse(engine.running)
        self.assertRegex(engine.tick().text, r"^\d{2}:\d{2}:\d{2}$")

    def test_progress_is_clamped(self):
        engine, clock = make_engine(mode=MODE_DOWN, countdown_start=10)
        engine.start()
        clock.advance(50)
        self.assertEqual(engine.tick().progress, 0.0)


class UpdaterTests(unittest.TestCase):
    def test_version_parsing_and_comparison(self):
        self.assertEqual(parse_version("v1.2.3"), (1, 2, 3))
        self.assertEqual(parse_version("1.10"), (1, 10))
        self.assertEqual(parse_version("nope"), ())
        self.assertTrue(is_newer("v1.10.0", "v1.9.9"))
        self.assertFalse(is_newer("v1.0.0", "v1.0.0"))
        self.assertFalse(is_newer("v0.9", "v1.0"))


if __name__ == "__main__":
    unittest.main()
