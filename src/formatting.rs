//! Duration parsing and display formatting.
//!
//! Display format is `[+][HH:]MM:SS[.ff]`. Formatting works on integer
//! milliseconds so rounding can never produce something like `00:59.999`
//! (it correctly carries to `01:00.00`).

pub const MAX_DECIMALS: u8 = 3;
const SCALE: u64 = 1_000; // milliseconds

fn round_half_away(seconds: f64) -> u64 {
    (seconds.abs() * SCALE as f64).round() as u64
}

/// Split into `(hours, minutes, seconds, fraction)` at the given precision.
pub fn split_ticks(seconds: f64, decimals: u8) -> (u32, u32, u32, u32) {
    let decimals = decimals.min(MAX_DECIMALS);
    let divisor = 10u64.pow(decimals as u32);
    let unit = SCALE / divisor; // ms per display unit
    // Round to the display unit so 59.999 becomes 01:00.00.
    let total_units = (round_half_away(seconds) + unit / 2) / unit;

    let frac = if decimals == 0 { 0 } else { (total_units % divisor) as u32 };
    let total_seconds = total_units / divisor;
    let seconds_part = (total_seconds % 60) as u32;
    let minutes_total = total_seconds / 60;
    let minutes = (minutes_total % 60) as u32;
    let hours = (minutes_total / 60) as u32;
    (hours, minutes, seconds_part, frac)
}

/// Format a duration for display.
pub fn format_duration(seconds: f64, decimals: u8, show_hours: bool, show_sign: bool) -> String {
    let decimals = decimals.min(MAX_DECIMALS);
    let (hours, minutes, secs, frac) = split_ticks(seconds, decimals);

    let mut body = format!("{minutes:02}:{secs:02}");
    if decimals > 0 {
        body.push_str(&format!(".{frac:0width$}", width = decimals as usize));
    }
    if hours > 0 || show_hours {
        body = format!("{hours:02}:{body}");
    }

    if seconds < 0.0 {
        format!("-{body}")
    } else if show_sign {
        format!("+{body}")
    } else {
        body
    }
}

/// Parse user input such as `"90"`, `"1:30"` or `"01:02:03.5"`.
///
/// Accepts `[[HH:]MM:]SS[.fraction]`. Returns an error for unparsable or
/// negative input, or when minutes/seconds are not below 60.
pub fn parse_duration(text: &str) -> Result<f64, String> {
    let raw = text.trim();
    if raw.is_empty() {
        return Err("empty duration".to_string());
    }

    if !raw.contains(':') {
        let value: f64 = raw.parse().map_err(|_| format!("invalid duration: {raw:?}"))?;
        if !value.is_finite() {
            return Err("invalid duration".to_string());
        }
        if value < 0.0 {
            return Err("negative duration".to_string());
        }
        return Ok(value);
    }

    let parts: Vec<&str> = raw.split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.trim().is_empty()) {
        return Err(format!("invalid duration: {raw:?}"));
    }

    let mut total = 0.0;
    for (index, part) in parts.iter().enumerate() {
        let value: f64 = part.parse().map_err(|_| format!("invalid duration: {raw:?}"))?;
        if value < 0.0 || !value.is_finite() {
            return Err("negative duration".to_string());
        }
        let remaining = parts.len() - index - 1;
        match remaining {
            0 => {
                if value >= 60.0 {
                    return Err("seconds must be below 60".to_string());
                }
                total += value;
            }
            1 => {
                if value >= 60.0 {
                    return Err("minutes must be below 60".to_string());
                }
                total += value * 60.0;
            }
            _ => total += value * 3600.0,
        }
    }
    Ok(total)
}

/// Human-friendly editable representation used to prefill fields.
pub fn format_input(seconds: f64) -> String {
    if seconds < 60.0 {
        return trim_number(seconds);
    }
    let minutes_total = (seconds / 60.0) as u64;
    let secs = seconds - (minutes_total as f64) * 60.0;
    if minutes_total < 60 {
        if (secs.fract() - 0.0).abs() < f64::EPSILON {
            format!("{minutes_total:02}:{:02}", secs as u64)
        } else {
            format!("{minutes_total:02}:{secs:05.2}")
        }
    } else {
        let hours = minutes_total / 60;
        let minutes = minutes_total % 60;
        format!("{hours:02}:{minutes:02}:{:02}", secs as u64)
    }
}

fn trim_number(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        let text = format!("{value:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_durations() {
        assert_eq!(format_duration(0.0, 2, false, false), "00:00.00");
        assert_eq!(format_duration(65.5, 2, false, false), "01:05.50");
        assert_eq!(format_duration(3599.0, 0, false, false), "59:59");
        assert_eq!(format_duration(3600.0, 0, false, false), "01:00:00");
    }

    #[test]
    fn rounding_carries_correctly() {
        assert_eq!(format_duration(59.999, 2, false, false), "01:00.00");
        assert_eq!(format_duration(9.999, 2, false, false), "00:10.00");
        assert_eq!(format_duration(9.994, 2, false, false), "00:09.99");
    }

    #[test]
    fn negative_and_signed() {
        assert_eq!(format_duration(-5.0, 0, false, false), "-00:05");
        assert_eq!(format_duration(5.0, 0, false, true), "+00:05");
    }

    #[test]
    fn always_show_hours() {
        assert_eq!(format_duration(30.0, 0, true, false), "00:00:30");
    }

    #[test]
    fn zero_decimals_have_no_dot() {
        let text = format_duration(42.0, 0, false, false);
        assert!(!text.contains('.'), "{text}");
    }

    #[test]
    fn parse_accepts_common_inputs() {
        assert_eq!(parse_duration("90").unwrap(), 90.0);
        assert_eq!(parse_duration("1:30").unwrap(), 90.0);
        assert_eq!(parse_duration("01:02:03.5").unwrap(), 3723.5);
        assert_eq!(parse_duration(" 2.5 ").unwrap(), 2.5);
    }

    #[test]
    fn parse_rejects_invalid() {
        for bad in ["", "abc", "-5", "1:99", "1:2:3:4", "1::2"] {
            assert!(parse_duration(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn format_input_roundtrip() {
        for value in [0.0, 45.0, 60.0, 90.5, 3600.0, 3725.0] {
            let text = format_input(value);
            let back = parse_duration(&text).unwrap();
            assert!((back - value).abs() < 0.01, "{value} -> {text} -> {back}");
        }
    }
}
