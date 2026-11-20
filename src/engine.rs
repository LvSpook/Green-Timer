//! Framework-independent timer engine.
//!
//! Owns every piece of state that affects what the user sees. The UI only
//! renders the [`Snapshot`] it produces on each tick, which keeps the tricky
//! Pomodoro and countdown logic unit-testable without a window.
//!
//! Timing uses a monotonic clock, so wall-clock adjustments (NTP sync, DST)
//! can never make the timer jump or run backwards.

use crate::config::{Config, Mode};
use crate::formatting::format_duration;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Work,
    Break,
    LongBreak,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Work => "WORK",
            Phase::Break => "BREAK",
            Phase::LongBreak => "LONG BREAK",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    Normal,
    Threshold,
    Overdue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Play the configured sound cue.
    Cue,
    /// The Pomodoro phase advanced.
    PhaseChanged(Phase),
    /// A lap / auto-switch banner should be shown.
    AwaitingBanner,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub text: String,
    pub progress: f32,
    pub color_role: ColorRole,
    pub lap_text: String,
    pub lap_visible: bool,
    pub text_length_changed: bool,
    pub events: Vec<Event>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            text: "00:00.00".to_string(),
            progress: 0.0,
            color_role: ColorRole::Normal,
            lap_text: String::new(),
            lap_visible: false,
            text_length_changed: false,
            events: Vec::new(),
        }
    }
}

const LAP_SECONDS: f64 = 4.0;

/// Abstraction over time so tests can advance it instantly.
pub trait Clock {
    fn now(&self) -> Instant;
}

pub struct RealClock;
impl Clock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

pub struct TimerEngine<C: Clock = RealClock> {
    clock: C,
    running: bool,
    started_at: Instant,
    accumulated: Duration,
    phase: Phase,
    cycle_count: u32,
    lap_text: String,
    lap_until: Option<Instant>,
    last_text: String,
    last_char_count: usize,
    last_time_value: Option<f64>,
    cue_fired: bool,
    last_snapshot: Snapshot,
    pub fluid_phase: f32,
}

impl<C: Clock> TimerEngine<C> {
    pub fn new(clock: C) -> Self {
        let now = clock.now();
        TimerEngine {
            clock,
            running: false,
            started_at: now,
            accumulated: Duration::ZERO,
            phase: Phase::Work,
            cycle_count: 0,
            lap_text: String::new(),
            lap_until: None,
            last_text: String::new(),
            last_char_count: 0,
            last_time_value: None,
            last_snapshot: Snapshot::default(),
            cue_fired: false,
            fluid_phase: 0.0,
        }
    }

    pub fn running(&self) -> bool {
        self.running
    }

    #[cfg(test)]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    #[cfg(test)]
    pub fn cycle_count(&self) -> u32 {
        self.cycle_count
    }

    pub fn elapsed(&self) -> f64 {
        if self.running {
            (self.clock.now() - self.started_at).as_secs_f64() + self.accumulated.as_secs_f64()
        } else {
            self.accumulated.as_secs_f64()
        }
    }

    pub fn phase_duration(&self, config: &Config, phase: Phase) -> f64 {
        (match phase {
            Phase::Work => config.pomo_work,
            Phase::Break => config.pomo_break,
            Phase::LongBreak => config.pomo_long_break,
        }) * 60.0
    }

    pub fn start(&mut self, config: &Config) {
        if self.running || config.mode == Mode::Clock {
            return;
        }
        self.started_at = self.clock.now();
        self.running = true;
    }

    pub fn pause(&mut self) {
        if !self.running {
            return;
        }
        self.accumulated = Duration::from_secs_f64(self.elapsed());
        self.running = false;
    }

    pub fn toggle(&mut self, config: &Config) {
        if self.running {
            self.pause();
        } else {
            self.start(config);
        }
    }

    pub fn reset(&mut self) {
        self.running = false;
        self.accumulated = Duration::ZERO;
        self.started_at = self.clock.now();
        self.phase = Phase::Work;
        self.cycle_count = 0;
        self.lap_text.clear();
        self.lap_until = None;
        self.last_time_value = None;
        self.last_char_count = 0;
        self.last_text.clear();
        self.cue_fired = false;
        self.fluid_phase = 0.0;
    }

    /// Show a lap banner. Returns `true` when accepted.
    pub fn trigger_lap(&mut self, config: &Config) -> bool {
        if !self.running || config.mode == Mode::Clock {
            return false;
        }
        let reference = if self.last_snapshot.text.is_empty() {
            self.format_value(config, 0.0, false)
        } else {
            self.last_snapshot.text.clone()
        };
        self.lap_text = format!("LAP: {reference}");
        self.lap_until = Some(self.clock.now() + Duration::from_secs_f64(LAP_SECONDS));
        true
    }

    fn format_value(&self, config: &Config, value: f64, overdue: bool) -> String {
        if overdue && value < 0.0 {
            format!(
                "+{}",
                format_duration(-value, config.decimals, config.always_show_hours, false)
            )
        } else {
            format_duration(value, config.decimals, config.always_show_hours, false)
        }
    }

    fn color_role(&self, config: &Config, value: f64, overdue: bool) -> ColorRole {
        if overdue {
            return ColorRole::Overdue;
        }
        if config.use_threshold {
            match config.mode {
                Mode::Up if value >= config.threshold_time => return ColorRole::Threshold,
                Mode::Down if value <= config.threshold_time => return ColorRole::Threshold,
                _ => {}
            }
        }
        ColorRole::Normal
    }

    /// Advance to the current time and produce a fresh snapshot.
    pub fn tick(&mut self, config: &Config) -> Snapshot {
        let now = self.clock.now();
        let mut events = Vec::new();

        if config.mode == Mode::Clock {
            let text = format_clock(&config.clock_format);
            return self.build(text, 0.0, ColorRole::Normal, events);
        }

        let (progress, value, overdue);

        match config.mode {
            Mode::Pomodoro => {
                let elapsed = self.elapsed();
                let mut remaining = self.phase_duration(config, self.phase) - elapsed;
                if remaining <= 0.0 {
                    events.push(Event::Cue);
                    if self.phase == Phase::Work {
                        self.cycle_count += 1;
                        let cycles = config.pomo_cycles.max(1);
                        self.phase = if self.cycle_count % cycles == 0 {
                            Phase::LongBreak
                        } else {
                            Phase::Break
                        };
                    } else {
                        self.phase = Phase::Work;
                    }
                    events.push(Event::PhaseChanged(self.phase));
                    self.accumulated = Duration::ZERO;
                    self.started_at = now;
                    remaining = self.phase_duration(config, self.phase);
                    self.lap_text = format!("AUTO: {}", self.phase.label());
                    self.lap_until = Some(now + Duration::from_secs_f64(LAP_SECONDS));
                    events.push(Event::AwaitingBanner);
                }
                let total = self.phase_duration(config, self.phase).max(1e-6);
                progress = (remaining.max(0.0) / total) as f32;
                value = remaining;
                overdue = false;
            }
            Mode::Down => {
                value = config.countdown_start - self.elapsed();
                overdue = value < 0.0;
                if overdue {
                    // Edge-triggered: fire once when the countdown passes zero,
                    // even if no frame was rendered just before it did.
                    if !self.cue_fired {
                        events.push(Event::Cue);
                        self.cue_fired = true;
                    }
                } else {
                    self.cue_fired = false;
                }
                progress = (value.max(0.0) / config.countdown_start.max(1e-6)) as f32;
            }
            Mode::Up => {
                value = self.elapsed();
                progress = ((value % 60.0) / 60.0) as f32;
                overdue = false;
            }
            Mode::Clock => unreachable!(),
        }

        self.last_time_value = Some(value);
        let text = self.format_value(config, value, overdue);
        let role = self.color_role(config, value, overdue);
        self.build(text, progress, role, events)
    }

    fn build(
        &mut self,
        text: String,
        progress: f32,
        role: ColorRole,
        events: Vec<Event>,
    ) -> Snapshot {
        let now = self.clock.now();
        let lap_visible = match self.lap_until {
            Some(until) => now < until,
            None => false,
        };
        if !lap_visible && !self.lap_text.is_empty() {
            self.lap_text.clear();
        }

        let text_length_changed = text.chars().count() != self.last_char_count;
        self.last_char_count = text.chars().count();
        self.last_text = text.clone();

        let snapshot = Snapshot {
            text,
            progress: progress.clamp(0.0, 1.0),
            color_role: role,
            lap_text: self.lap_text.clone(),
            lap_visible,
            text_length_changed,
            events,
        };
        self.last_snapshot = snapshot.clone();
        snapshot
    }

    /// Latest rendered text (used for lap labels and window sizing).
    pub fn current_text(&self, config: &Config) -> String {
        if !self.last_text.is_empty() {
            return self.last_text.clone();
        }
        let value = match config.mode {
            Mode::Down => config.countdown_start,
            Mode::Pomodoro => self.phase_duration(config, self.phase),
            _ => 0.0,
        };
        self.format_value(config, value, false)
    }
}

/// Format the wall clock using a C-style `strftime` subset.
pub fn format_clock(pattern: &str) -> String {
    let now = chrono_lite::LocalTime::now();
    now.format(pattern)
}

/// Minimal `strftime` implementation so the app needs no chrono dependency.
mod chrono_lite {
    use std::time::{SystemTime, UNIX_EPOCH};

    pub struct LocalTime {
        hour: u32,
        minute: u32,
        second: u32,
        hour12: u32,
        am: bool,
        year: u32,
        month: u32,
        day: u32,
        weekday: u32,
    }

    impl LocalTime {
        pub fn now() -> Self {
            let unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Self::from_unix(unix)
        }

        pub fn from_unix(unix: i64) -> Self {
            // Local offset is obtained from the OS so DST is handled correctly.
            let offset = local_offset_seconds(unix);
            let local = unix + offset;

            let days = local.div_euclid(86_400);
            let secs_of_day = local.rem_euclid(86_400);
            let (year, month, day) = civil_from_days(days);
            let hour = (secs_of_day / 3600) as u32;
            let minute = ((secs_of_day % 3600) / 60) as u32;
            let second = (secs_of_day % 60) as u32;
            let hour12 = match hour % 12 {
                0 => 12,
                h => h,
            };
            LocalTime {
                hour,
                minute,
                second,
                hour12,
                am: hour < 12,
                year,
                month,
                day,
                weekday: ((days + 4).rem_euclid(7)) as u32, // 1970-01-01 was a Thursday
            }
        }

        pub fn format(&self, pattern: &str) -> String {
            let mut out = String::with_capacity(pattern.len() + 8);
            let mut chars = pattern.chars();
            while let Some(ch) = chars.next() {
                if ch != '%' {
                    out.push(ch);
                    continue;
                }
                match chars.next() {
                    Some('H') => out.push_str(&format!("{:02}", self.hour)),
                    Some('I') => out.push_str(&format!("{:02}", self.hour12)),
                    Some('M') => out.push_str(&format!("{:02}", self.minute)),
                    Some('S') => out.push_str(&format!("{:02}", self.second)),
                    Some('p') => out.push_str(if self.am { "AM" } else { "PM" }),
                    Some('Y') => out.push_str(&format!("{:04}", self.year)),
                    Some('m') => out.push_str(&format!("{:02}", self.month)),
                    Some('d') => out.push_str(&format!("{:02}", self.day)),
                    Some('A') => out.push_str(WEEKDAYS[self.weekday as usize]),
                    Some('a') => out.push_str(&WEEKDAYS[self.weekday as usize][..3]),
                    Some('%') => out.push('%'),
                    Some(other) => {
                        out.push('%');
                        out.push(other);
                    }
                    None => out.push('%'),
                }
            }
            out
        }
    }

    const WEEKDAYS: [&str; 7] = [
        "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
    ];

    /// Days-to-civil conversion (Howard Hinnant's algorithm).
    fn civil_from_days(z: i64) -> (u32, u32, u32) {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        (y as u32, m as u32, d as u32)
    }

    #[cfg(windows)]
    fn local_offset_seconds(unix: i64) -> i64 {
        use windows::Win32::Foundation::SYSTEMTIME;
        use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

        // Convert the instant to a SYSTEMTIME (UTC), then ask Windows for the
        // local time at that moment so the DST rule in force is respected.
        unsafe {
            let utc = unix_to_systemtime(unix);
            let mut local = SYSTEMTIME::default();
            if SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
                return 0;
            }
            systemtime_to_unix(&local) - systemtime_to_unix(&utc)
        }
    }

    #[cfg(windows)]
    fn unix_to_systemtime(unix: i64) -> windows::Win32::Foundation::SYSTEMTIME {
        use windows::Win32::Foundation::SYSTEMTIME;
        let days = unix.div_euclid(86_400);
        let secs_of_day = unix.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        SYSTEMTIME {
            wYear: year as u16,
            wMonth: month as u16,
            wDay: day as u16,
            wHour: (secs_of_day / 3600) as u16,
            wMinute: ((secs_of_day % 3600) / 60) as u16,
            wSecond: (secs_of_day % 60) as u16,
            wMilliseconds: 0,
            ..Default::default()
        }
    }

    #[cfg(windows)]
    fn systemtime_to_unix(st: &windows::Win32::Foundation::SYSTEMTIME) -> i64 {
        let days = days_from_civil(st.wYear as i64, st.wMonth as i64, st.wDay as i64);
        days * 86_400
            + (st.wHour as i64) * 3600
            + (st.wMinute as i64) * 60
            + st.wSecond as i64
    }

    #[cfg(windows)]
    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = if m > 2 { m - 3 } else { m + 9 };
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    #[cfg(not(windows))]
    fn local_offset_seconds(_unix: i64) -> i64 {
        0 // Tests and non-Windows builds use UTC.
    }

    #[cfg(test)]
    pub fn test_time(hour: u32, minute: u32, second: u32) -> LocalTime {
        LocalTime {
            hour,
            minute,
            second,
            hour12: if hour % 12 == 0 { 12 } else { hour % 12 },
            am: hour < 12,
            year: 2026,
            month: 11,
            day: 20,
            weekday: 4,
        }
    }
}

#[cfg(test)]
pub use chrono_lite::test_time;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct FakeClock(Rc<Cell<Duration>>);
    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            // A fixed base plus the fake offset keeps comparisons stable.
            static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            let base = *BASE.get_or_init(Instant::now);
            base + self.0.get()
        }
    }

    struct Harness {
        engine: TimerEngine<FakeClock>,
        time: Rc<Cell<Duration>>,
        config: Config,
    }

    impl Harness {
        fn new(config: Config) -> Self {
            let time = Rc::new(Cell::new(Duration::ZERO));
            let engine = TimerEngine::new(FakeClock(time.clone()));
            Harness { engine, time, config }
        }

        fn advance(&self, seconds: f64) {
            let next = self.time.get() + Duration::from_secs_f64(seconds);
            self.time.set(next);
        }

        fn tick(&mut self) -> Snapshot {
            self.engine.tick(&self.config)
        }
    }

    #[test]
    fn count_up_tracks_elapsed() {
        let mut h = Harness::new(Config { mode: Mode::Up, ..Default::default() });
        h.engine.start(&h.config);
        h.advance(2.5);
        let snap = h.tick();
        assert_eq!(snap.text, "00:02.50");
        assert!(h.engine.running());
        h.engine.pause();
        h.advance(100.0);
        assert_eq!(h.tick().text, "00:02.50");
    }

    #[test]
    fn countdown_goes_overdue_with_sign() {
        let mut h = Harness::new(Config {
            mode: Mode::Down,
            countdown_start: 10.0,
            ..Default::default()
        });
        h.engine.start(&h.config);
        h.advance(5.0);
        assert_eq!(h.tick().text, "00:05.00");
        h.advance(6.0);
        let snap = h.tick();
        assert_eq!(snap.text, "+00:01.00");
        assert_eq!(snap.color_role, ColorRole::Overdue);
        assert!(snap.events.contains(&Event::Cue));
    }

    #[test]
    fn cue_fires_only_once_when_crossing_zero() {
        let mut h = Harness::new(Config {
            mode: Mode::Down,
            countdown_start: 2.0,
            ..Default::default()
        });
        h.engine.start(&h.config);
        h.advance(3.0);
        assert!(h.tick().events.contains(&Event::Cue));
        h.advance(1.0);
        assert!(!h.tick().events.contains(&Event::Cue));
    }

    #[test]
    fn threshold_roles() {
        let mut up = Harness::new(Config {
            threshold_time: 5.0,
            use_threshold: true,
            ..Default::default()
        });
        up.engine.start(&up.config);
        up.advance(2.0);
        assert_eq!(up.tick().color_role, ColorRole::Normal);
        up.advance(5.0);
        assert_eq!(up.tick().color_role, ColorRole::Threshold);

        let mut down = Harness::new(Config {
            mode: Mode::Down,
            countdown_start: 10.0,
            threshold_time: 4.0,
            use_threshold: true,
            ..Default::default()
        });
        down.engine.start(&down.config);
        down.advance(7.0);
        assert_eq!(down.tick().color_role, ColorRole::Threshold);
    }

    #[test]
    fn pomodoro_cycles_through_phases() {
        let mut h = Harness::new(Config {
            mode: Mode::Pomodoro,
            pomo_work: 1.0,
            pomo_break: 1.0,
            pomo_long_break: 1.0,
            pomo_cycles: 2,
            ..Default::default()
        });
        h.engine.start(&h.config);
        assert_eq!(h.engine.phase(), Phase::Work);

        h.advance(61.0);
        let snap = h.tick();
        assert_eq!(h.engine.phase(), Phase::Break);
        assert!(snap.events.contains(&Event::PhaseChanged(Phase::Break)));
        assert_eq!(snap.text, "01:00.00");

        h.advance(61.0);
        h.tick();
        assert_eq!(h.engine.phase(), Phase::Work);
        assert_eq!(h.engine.cycle_count(), 1);

        h.advance(61.0);
        h.tick();
        assert_eq!(h.engine.phase(), Phase::LongBreak);
        assert_eq!(h.engine.cycle_count(), 2);

        h.advance(61.0);
        h.tick();
        assert_eq!(h.engine.phase(), Phase::Work);
    }

    #[test]
    fn reset_clears_state() {
        let mut h = Harness::new(Config {
            mode: Mode::Pomodoro,
            pomo_work: 1.0,
            ..Default::default()
        });
        h.engine.start(&h.config);
        h.advance(61.0);
        h.tick();
        h.engine.reset();
        assert!(!h.engine.running());
        assert_eq!(h.engine.phase(), Phase::Work);
        assert_eq!(h.engine.cycle_count(), 0);
        assert_eq!(h.engine.elapsed(), 0.0);
    }

    #[test]
    fn lap_only_when_running() {
        let mut h = Harness::new(Config::default());
        assert!(!h.engine.trigger_lap(&h.config));
        h.engine.start(&h.config);
        h.advance(1.0);
        h.tick();
        assert!(h.engine.trigger_lap(&h.config));
        let snap = h.tick();
        assert!(snap.lap_visible);
        assert!(snap.lap_text.starts_with("LAP:"));
        h.advance(5.0);
        assert!(!h.tick().lap_visible);
    }

    #[test]
    fn clock_mode_never_runs() {
        let mut h = Harness::new(Config { mode: Mode::Clock, ..Default::default() });
        h.engine.start(&h.config);
        assert!(!h.engine.running());
    }

    #[test]
    fn progress_is_clamped() {
        let mut h = Harness::new(Config {
            mode: Mode::Down,
            countdown_start: 10.0,
            ..Default::default()
        });
        h.engine.start(&h.config);
        h.advance(50.0);
        assert_eq!(h.tick().progress, 0.0);
    }

    #[test]
    fn clock_pattern_formatting() {
        let time = test_time(14, 5, 9);
        assert_eq!(time.format("%H:%M:%S"), "14:05:09");
        assert_eq!(time.format("%I:%M %p"), "02:05 PM");
        assert_eq!(time.format("%Y-%m-%d"), "2026-11-20");
        assert_eq!(time.format("100%%"), "100%");
    }

    #[test]
    fn clock_midnight_is_twelve_am() {
        let time = test_time(0, 30, 0);
        assert_eq!(time.format("%I %p"), "12 AM");
    }
}
