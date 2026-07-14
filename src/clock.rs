//! The single time source. All display time flows through `ClockSource`;
//! nothing else may read wall time (see CLAUDE.md invariants).

use std::time::Instant;

use chrono::Timelike;

/// Display time in seconds since midnight, advancing at `multiplier` times
/// real speed. Changing the multiplier rebases so display time is continuous.
pub struct ClockSource {
    base_secs: f64,
    anchor: Instant,
    multiplier: f64,
}

impl ClockSource {
    /// Start from the current local wall-clock time.
    pub fn wall(multiplier: f64) -> Self {
        let now = chrono::Local::now();
        let secs = now.num_seconds_from_midnight() as f64 + now.nanosecond() as f64 * 1e-9;
        Self::at(secs, multiplier)
    }

    /// Start from a given time (seconds since midnight).
    pub fn at(secs: f64, multiplier: f64) -> Self {
        Self {
            base_secs: secs,
            anchor: Instant::now(),
            multiplier,
        }
    }

    /// Current display time, seconds since midnight (wraps at 24h).
    pub fn now(&self) -> f64 {
        let t = self.base_secs + self.anchor.elapsed().as_secs_f64() * self.multiplier;
        t.rem_euclid(24.0 * 3600.0)
    }

    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    pub fn set_multiplier(&mut self, multiplier: f64) {
        let now = self.now();
        self.base_secs = now;
        self.anchor = Instant::now();
        self.multiplier = multiplier;
    }
}

/// Parse "HH:MM:SS" or "HH:MM" into seconds since midnight.
pub fn parse_time(s: &str) -> Result<f64, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(format!("bad time '{s}', expected HH:MM[:SS]"));
    }
    let num = |p: &str| -> Result<f64, String> {
        p.parse::<f64>().map_err(|_| format!("bad time component '{p}' in '{s}'"))
    };
    let h = num(parts[0])?;
    let m = num(parts[1])?;
    let sec = if parts.len() == 3 { num(parts[2])? } else { 0.0 };
    if !(0.0..24.0).contains(&h) || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&sec) {
        return Err(format!("time '{s}' out of range"));
    }
    Ok(h * 3600.0 + m * 60.0 + sec)
}

/// Format seconds since midnight as HH:MM:SS.
pub fn format_time(secs: f64) -> String {
    let t = secs.rem_euclid(24.0 * 3600.0) as u64;
    format!("{:02}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}
