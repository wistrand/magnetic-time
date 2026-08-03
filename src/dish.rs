//! Parametric dish shapes: the container the particles live in, as a signed
//! distance function (negative inside, gradient = outward normal). The base
//! shape's boundary sits at the dial edge (world radius 1.0 for the circle);
//! the particle wall is inset by [`WALL_INSET`] everywhere, matching the
//! classic circle's DISH_R = 0.92. Holes carve the allowed region via SDF
//! difference; their walls behave exactly like the outer one. Ticks and hand
//! length stay on the inscribed circle regardless of shape.

use crate::vec2::Vec2;

/// Wall inset from the visual boundary, dial units (circle: 1.0 -> 0.92).
pub const WALL_INSET: f64 = 0.08;

/// Maximum holes; fixed so [`Dish`] stays Copy (SimParams is Copy).
pub const MAX_HOLES: usize = 4;

#[derive(Clone, Copy, PartialEq)]
pub enum DishBase {
    /// Unit circle (the classic dish).
    Circle,
    /// Rounded square, half-side 1.0, corner radius `corner`.
    Square { corner: f64 },
    /// Superellipse |x|^n + |y|^n = 1. n = 2 is the circle; larger n bulges
    /// toward a square. The SDF is a radial pseudo-distance (exact on the
    /// axes), good enough for the wall force and the rasterized edge.
    Super { n: f64 },
    /// N-spiked star: tips at radius 1.0 (one at 12 o'clock), notch vertices
    /// at radius `inner`.
    Star { n: u32, inner: f64 },
    /// Regular N-gon, circumradius 1.0, one vertex at 12 o'clock. (A star
    /// whose notch radius equals the edge-midpoint radius.)
    Poly { n: u32 },
}

/// Exact signed distance to an N-spiked star (tips at radius 1.0, notches at
/// `inner`, one tip on the 12 o'clock axis; y is down). Angular fold into a
/// half-sector reduces the boundary to one spike edge segment.
fn star_sdf(p: Vec2, n: u32, inner: f64) -> f64 {
    use std::f64::consts::PI;
    let an = PI / n as f64;
    let phi = p.x.atan2(-p.y);
    let b = phi.rem_euclid(2.0 * an) - an;
    let l = p.len();
    let q = Vec2::new(l * b.cos(), l * b.sin().abs());
    let a_pt = Vec2::new(1.0, 0.0);
    let b_pt = Vec2::new(inner * an.cos(), inner * an.sin());
    let ab = b_pt - a_pt;
    let t = ((q - a_pt).dot(ab) / ab.len_sq()).clamp(0.0, 1.0);
    let d = (q - (a_pt + ab * t)).len();
    let cross = ab.x * (q.y - a_pt.y) - ab.y * (q.x - a_pt.x);
    if cross >= 0.0 {
        -d
    } else {
        d
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Dish {
    pub base: DishBase,
    /// Circular holes (center x, center y, radius) carved out of the base.
    pub holes: [Option<(f64, f64, f64)>; MAX_HOLES],
}

impl Default for Dish {
    fn default() -> Self {
        Self {
            base: DishBase::Circle,
            holes: [None; MAX_HOLES],
        }
    }
}

impl Dish {
    /// The default circle with no holes: sim and renderer keep their exact
    /// original circle fast paths, so default output stays byte-identical.
    pub fn is_plain_circle(&self) -> bool {
        self.base == DishBase::Circle && self.holes.iter().all(|h| h.is_none())
    }

    /// Signed distance to the visual boundary: negative inside the dish,
    /// positive outside or inside a hole.
    pub fn sdf(&self, p: Vec2) -> f64 {
        let mut s = match self.base {
            DishBase::Circle => p.len() - 1.0,
            DishBase::Square { corner } => {
                let b = 1.0 - corner;
                let qx = p.x.abs() - b;
                let qy = p.y.abs() - b;
                let ax = qx.max(0.0);
                let ay = qy.max(0.0);
                (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - corner
            }
            DishBase::Super { n } => {
                let v = p.x.abs().powf(n) + p.y.abs().powf(n);
                v.powf(1.0 / n) - 1.0
            }
            DishBase::Star { n, inner } => star_sdf(p, n, inner),
            DishBase::Poly { n } => {
                let an = std::f64::consts::PI / n as f64;
                star_sdf(p, n, an.cos())
            }
        };
        for &(hx, hy, hr) in self.holes.iter().flatten() {
            s = s.max(hr - Vec2::new(p.x - hx, p.y - hy).len());
        }
        s
    }

    /// Signed distance outside the particle wall (positive = must be pushed
    /// back) and the outward normal, via central differences on the SDF.
    /// Callers keep a plain-circle fast path in f32; this is the shaped path.
    pub fn wall(&self, p: Vec2) -> (f64, Vec2) {
        const E: f64 = 1e-4;
        let s = self.sdf(p) + WALL_INSET;
        let gx = self.sdf(Vec2::new(p.x + E, p.y)) - self.sdf(Vec2::new(p.x - E, p.y));
        let gy = self.sdf(Vec2::new(p.x, p.y + E)) - self.sdf(Vec2::new(p.x, p.y - E));
        let g = Vec2::new(gx, gy);
        let gl = g.len();
        let n = if gl > 1e-12 { g / gl } else { Vec2::new(1.0, 0.0) };
        (s, n)
    }

    /// Parse the CLI grammar: `circle | square[:CORNER] | super[:N] |
    /// ring:INNER`, with up to [`MAX_HOLES`] `+hole:X,Y,R` suffixes.
    /// `ring:I` is shorthand for `circle+hole:0,0,I`.
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut dish = Dish::default();
        let mut n_holes = 0;
        let mut add_hole = |dish: &mut Dish, x: f64, y: f64, r: f64| -> Result<(), String> {
            if n_holes >= MAX_HOLES {
                return Err(format!("--dish: at most {MAX_HOLES} holes"));
            }
            if !(x.is_finite() && y.is_finite() && r.is_finite())
                || (x * x + y * y).sqrt() >= 1.0
                || !(0.02..=0.9).contains(&r)
            {
                return Err(format!(
                    "--dish: hole must have |center| < 1 and radius in 0.02..=0.9, got {x},{y},{r}"
                ));
            }
            dish.holes[n_holes] = Some((x, y, r));
            n_holes += 1;
            Ok(())
        };
        let mut parts = s.split('+');
        let base = parts.next().unwrap_or("");
        match base.split_once(':') {
            None if base == "circle" => {}
            None if base == "square" => dish.base = DishBase::Square { corner: 0.15 },
            None if base == "super" => dish.base = DishBase::Super { n: 3.0 },
            None if base == "star" => dish.base = DishBase::Star { n: 5, inner: 0.5 },
            None if base == "poly" => dish.base = DishBase::Poly { n: 6 },
            Some(("square", c)) => {
                let corner: f64 = c.parse().map_err(|_| format!("--dish: bad corner '{c}'"))?;
                if !corner.is_finite() || !(0.0..=0.9).contains(&corner) {
                    return Err(format!("--dish: corner must be in 0..=0.9, got {corner}"));
                }
                dish.base = DishBase::Square { corner };
            }
            Some(("super", n)) => {
                let n: f64 = n.parse().map_err(|_| format!("--dish: bad exponent '{n}'"))?;
                if !n.is_finite() || !(0.5..=8.0).contains(&n) {
                    return Err(format!("--dish: super exponent must be in 0.5..=8, got {n}"));
                }
                dish.base = DishBase::Super { n };
            }
            Some(("star", rest)) => {
                let (n_s, inner_s) = match rest.split_once(':') {
                    Some((n, i)) => (n, Some(i)),
                    None => (rest, None),
                };
                let n: u32 = n_s.parse().map_err(|_| format!("--dish: bad spike count '{n_s}'"))?;
                if !(3..=24).contains(&n) {
                    return Err(format!("--dish: star spikes must be 3..=24, got {n}"));
                }
                let inner: f64 = match inner_s {
                    Some(i) => i.parse().map_err(|_| format!("--dish: bad inner radius '{i}'"))?,
                    None => 0.5,
                };
                if !inner.is_finite() || !(0.1..=0.9).contains(&inner) {
                    return Err(format!("--dish: star inner radius must be in 0.1..=0.9, got {inner}"));
                }
                dish.base = DishBase::Star { n, inner };
            }
            Some(("poly", n_s)) => {
                let n: u32 = n_s.parse().map_err(|_| format!("--dish: bad side count '{n_s}'"))?;
                if !(3..=24).contains(&n) {
                    return Err(format!("--dish: poly sides must be 3..=24, got {n}"));
                }
                dish.base = DishBase::Poly { n };
            }
            Some(("ring", i)) => {
                let inner: f64 = i.parse().map_err(|_| format!("--dish: bad inner radius '{i}'"))?;
                add_hole(&mut dish, 0.0, 0.0, inner)?;
            }
            _ => {
                return Err(format!(
                    "--dish: unknown shape '{base}' (circle, square[:CORNER], super[:N], \
                     star[:N[:INNER]], poly[:N], ring:INNER)"
                ))
            }
        }
        for part in parts {
            let Some(spec) = part.strip_prefix("hole:") else {
                return Err(format!("--dish: expected +hole:X,Y,R, got '+{part}'"));
            };
            let v: Vec<&str> = spec.split(',').collect();
            if v.len() != 3 {
                return Err(format!("--dish: hole needs X,Y,R, got '{spec}'"));
            }
            let f = |s: &str| s.parse::<f64>().map_err(|_| format!("--dish: bad hole number '{s}'"));
            add_hole(&mut dish, f(v[0])?, f(v[1])?, f(v[2])?)?;
        }
        Ok(dish)
    }

    /// Round-trippable form of the configuration (preset "dish" key).
    pub fn label(&self) -> String {
        let mut s = match self.base {
            DishBase::Circle => "circle".to_string(),
            DishBase::Square { corner } => format!("square:{corner}"),
            DishBase::Super { n } => format!("super:{n}"),
            DishBase::Star { n, inner } => format!("star:{n}:{inner}"),
            DishBase::Poly { n } => format!("poly:{n}"),
        };
        for &(x, y, r) in self.holes.iter().flatten() {
            s.push_str(&format!("+hole:{x},{y},{r}"));
        }
        s
    }
}
