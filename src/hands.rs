//! Hand geometry shared by rendering and the field model. World/sim
//! coordinates are clock-face units: center at origin, dial radius 1.0,
//! y pointing down (pixel orientation). 12 o'clock is -y.

const TAU: f64 = std::f64::consts::TAU;

/// Hand lengths as fractions of the dial radius: hour, minute, second.
pub const LEN: [f64; 3] = [0.52, 0.78, 0.88];

/// Hand angles in radians for a display time (seconds since midnight),
/// smooth (no ticking). Order: hour, minute, second. Angle 0 points +x
/// (3 o'clock); rotation is clockwise on screen.
pub fn angles(time_secs: f64) -> [f64; 3] {
    let s = time_secs % 60.0;
    let m = (time_secs / 60.0) % 60.0;
    let h = (time_secs / 3600.0) % 12.0;
    [h / 12.0, m / 60.0, s / 60.0].map(|frac| frac * TAU - TAU / 4.0)
}
