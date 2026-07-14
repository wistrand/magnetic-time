//! Magnetic field of the hand magnets. Hands carry rigid, data-driven magnet
//! layouts; each frame the layouts are rotated into world space and expanded
//! into field elements (point dipoles and pole-face charges) that are summed.
//! See agent_docs/design-simulation.md for the model and units.

use crate::hands;
use crate::vec2::Vec2;

/// Evaluation distance clamp (dial-radius units). Fields diverge at their
/// sources; anything closer than this is treated as being at this distance.
/// Disc magnets clamp at their radius instead.
pub const MIN_DIST: f64 = 0.02;

/// Step for the numeric gradient of |B|^2.
const GRAD_EPS: f64 = 1e-3;

/// Magnet extent as configured per hand (dev panel / CLI). Resolved to a
/// [MagnetShape] against the hand's length by `LayoutSpec::build`.
#[derive(Clone, Copy, PartialEq)]
pub enum SpecShape {
    Point,
    Disc { radius: f64 },
    /// Bar magnet. `len_frac` is the bar length as a fraction of the hand
    /// length: 0 = point, 1 = full hand, up to 2 = overhangs past the hub
    /// (the outer end stays pinned to the tip). `half_wid` is absolute.
    Rect { len_frac: f64, half_wid: f64 },
}

/// Physical extent of one built magnet, in dial-radius units.
#[derive(Clone, Copy, PartialEq)]
pub enum MagnetShape {
    /// Ideal point dipole.
    Point,
    /// Round magnet: dipole far field, near field softened over the radius.
    Disc { radius: f64 },
    /// Bar magnet: north/south pole faces of distributed charge. Halves of
    /// the full length (along the moment) and width.
    Rect { half_len: f64, half_wid: f64 },
}

/// A magnet in hand-local coordinates: +x points along the hand from the
/// center toward the tip.
pub struct LocalMagnet {
    pub pos: Vec2,
    /// Direction is the north-pole axis, magnitude is strength.
    pub moment: Vec2,
    pub shape: MagnetShape,
}

/// Magnet layout carried by one hand.
pub struct HandMagnets {
    pub magnets: Vec<LocalMagnet>,
}

/// One world-space field element.
enum Element {
    Dipole { pos: Vec2, moment: Vec2, r_min: f64 },
    Charge { pos: Vec2, q: f64, r_min: f64 },
}

/// World-space magnet outline for the dipoles debug view.
pub struct Marker {
    pub pos: Vec2,
    /// Unit north-pole axis.
    pub dir: Vec2,
    pub shape: MagnetShape,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MagnetKind {
    /// Single magnet at the tip.
    Tip,
    /// N magnets along the hand, all north-out.
    Strip,
    /// N magnets along the hand, alternating polarity (tip is north-out).
    Alt,
}

/// Innermost magnet position for multi-magnet layouts (clear of the hub).
const STRIP_START: f64 = 0.18;

/// Buildable description of one hand's magnet layout; what the CLI and dev
/// panel edit.
#[derive(Clone, Copy, PartialEq)]
pub struct LayoutSpec {
    pub kind: MagnetKind,
    /// Magnet count; ignored for Tip.
    pub n: usize,
    /// Per-magnet moment magnitude for this hand.
    pub strength: f64,
    pub shape: SpecShape,
}

impl LayoutSpec {
    pub const TIP: LayoutSpec = LayoutSpec {
        kind: MagnetKind::Tip,
        n: 1,
        strength: 1.0,
        shape: SpecShape::Point,
    };

    /// Parse "tip", "strip:N", or "alt:N".
    pub fn parse(s: &str) -> Result<Self, String> {
        let (kind, n) = match s.split_once(':') {
            None => (s, 1),
            Some((k, num)) => (
                k,
                num.parse::<usize>()
                    .map_err(|_| format!("bad magnet count in '{s}'"))?,
            ),
        };
        let n = n.clamp(1, 16);
        let kind = match kind {
            "tip" => return Ok(Self::TIP),
            "strip" => MagnetKind::Strip,
            "alt" => MagnetKind::Alt,
            other => return Err(format!("unknown magnet layout '{other}' (tip, strip:N, alt:N)")),
        };
        Ok(Self { kind, n, ..Self::TIP })
    }

    pub fn label(&self) -> String {
        match self.kind {
            MagnetKind::Tip => "tip".to_string(),
            MagnetKind::Strip => format!("strip:{}", self.n),
            MagnetKind::Alt => format!("alt:{}", self.n),
        }
    }

    /// Build the layout for a hand of the given length.
    pub fn build(&self, len: f64) -> HandMagnets {
        let n = if self.kind == MagnetKind::Tip { 1 } else { self.n.max(1) };
        // Resolve the configured shape against this hand's length. A rect
        // too short to have distinct pole faces degrades to a point.
        let shape = match self.shape {
            SpecShape::Point => MagnetShape::Point,
            SpecShape::Disc { radius } => MagnetShape::Disc { radius },
            SpecShape::Rect { len_frac, half_wid } => {
                let half_len = len_frac.clamp(0.0, 2.0) * len / 2.0;
                if half_len < 0.005 {
                    MagnetShape::Point
                } else {
                    MagnetShape::Rect { half_len, half_wid }
                }
            }
        };
        let mut magnets = Vec::with_capacity(n);
        for i in 0..n {
            // Place from tip inward so the tip always carries a magnet and,
            // for Alt, is always north-out regardless of count.
            let x = if n == 1 {
                len
            } else {
                len - (len - STRIP_START) * i as f64 / (n - 1) as f64
            };
            let sign = match self.kind {
                MagnetKind::Alt if i % 2 == 1 => -1.0,
                _ => 1.0,
            };
            // Keep a bar magnet's outer end at the hand tip: a rect with
            // len_frac 1 on a tip layout spans center-to-tip exactly.
            let x = match shape {
                MagnetShape::Rect { half_len, .. } => x.min(len - half_len),
                _ => x,
            };
            magnets.push(LocalMagnet {
                pos: Vec2::new(x, 0.0),
                moment: Vec2::new(sign * self.strength, 0.0),
                shape,
            });
        }
        HandMagnets { magnets }
    }
}

/// Parse a magnet shape: "point", "disc:R" (radius in dial-radius units), or
/// "rect:FxW" (F = length as a fraction of the hand length, 1 = full hand;
/// W = full width in dial-radius units).
pub fn parse_shape(s: &str) -> Result<SpecShape, String> {
    match s.split_once(':') {
        None if s == "point" => Ok(SpecShape::Point),
        Some(("disc", r)) => {
            let radius: f64 = r.parse().map_err(|_| format!("bad disc radius '{r}'"))?;
            Ok(SpecShape::Disc {
                radius: radius.clamp(0.005, 0.3),
            })
        }
        Some(("rect", dims)) => {
            let (l, w) = dims
                .split_once('x')
                .ok_or(format!("bad rect '{dims}', expected FxW"))?;
            let l: f64 = l.parse().map_err(|_| format!("bad rect length fraction '{l}'"))?;
            let w: f64 = w.parse().map_err(|_| format!("bad rect width '{w}'"))?;
            Ok(SpecShape::Rect {
                len_frac: l.clamp(0.0, 2.0),
                half_wid: (w / 2.0).clamp(0.0025, 0.3),
            })
        }
        _ => Err(format!("unknown shape '{s}' (point, disc:R, rect:FxW)")),
    }
}

/// Specs in hand order: hour, minute, second. These are the owner-tuned
/// "rings" preset defaults; change them only with the owner.
pub fn default_specs() -> [LayoutSpec; 3] {
    let bar = |strength: f64, len_frac: f64| LayoutSpec {
        kind: MagnetKind::Tip,
        n: 1,
        strength,
        shape: SpecShape::Rect {
            len_frac,
            half_wid: 0.015,
        },
    };
    [bar(0.10, 1.4), bar(0.05, 0.9), bar(0.60, 1.0)]
}

pub fn build_layouts(specs: &[LayoutSpec; 3]) -> [HandMagnets; 3] {
    [
        specs[0].build(hands::LEN[0]),
        specs[1].build(hands::LEN[1]),
        specs[2].build(hands::LEN[2]),
    ]
}

/// All hand magnets rotated into world space and expanded into field
/// elements for one display time.
pub struct FieldSources {
    elements: Vec<Element>,
    pub markers: Vec<Marker>,
}

impl FieldSources {
    pub fn at_time(layouts: &[HandMagnets; 3], time_secs: f64) -> Self {
        let angles = hands::angles(time_secs);
        let mut elements = Vec::new();
        let mut markers = Vec::new();
        for (layout, angle) in layouts.iter().zip(angles) {
            let (c, s) = (angle.cos(), angle.sin());
            let rot = |v: Vec2| Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c);
            for mag in &layout.magnets {
                let pos = rot(mag.pos);
                let moment = rot(mag.moment);
                markers.push(Marker {
                    pos,
                    dir: moment.normalized(),
                    shape: mag.shape,
                });
                match mag.shape {
                    MagnetShape::Point => elements.push(Element::Dipole {
                        pos,
                        moment,
                        r_min: MIN_DIST,
                    }),
                    MagnetShape::Disc { radius } => elements.push(Element::Dipole {
                        pos,
                        moment,
                        r_min: radius.max(MIN_DIST),
                    }),
                    MagnetShape::Rect { half_len, half_wid } => {
                        // Two pole faces of distributed charge. Total charge
                        // chosen so the far field matches a point dipole of
                        // the same moment: q_total * 2*half_len = |moment|.
                        let strength = moment.len();
                        if strength < 1e-12 {
                            continue;
                        }
                        let axis = moment / strength;
                        let perp = Vec2::new(-axis.y, axis.x);
                        let samples = ((half_wid / 0.008).ceil() as usize).clamp(1, 5);
                        let q = strength / (2.0 * half_len) / samples as f64;
                        for face in [1.0, -1.0] {
                            let center = pos + axis * (half_len * face);
                            for k in 0..samples {
                                let t = if samples == 1 {
                                    0.0
                                } else {
                                    -1.0 + 2.0 * k as f64 / (samples - 1) as f64
                                };
                                elements.push(Element::Charge {
                                    pos: center + perp * (t * half_wid * 0.8),
                                    q: q * face,
                                    r_min: MIN_DIST,
                                });
                            }
                        }
                    }
                }
            }
        }
        Self { elements, markers }
    }

    /// Total field at a point. Dipole: k*(3(m.r_hat)r_hat - m)/|r|^3 with
    /// k=1. Charge: q*r_hat/|r|^2.
    pub fn b(&self, p: Vec2) -> Vec2 {
        let mut b = Vec2::ZERO;
        for e in &self.elements {
            match *e {
                Element::Dipole { pos, moment, r_min } => {
                    let dp = p - pos;
                    let dist = dp.len().max(r_min);
                    let rh = dp / dist;
                    let mdotr = moment.dot(rh);
                    b += (rh * (3.0 * mdotr) - moment) / (dist * dist * dist);
                }
                Element::Charge { pos, q, r_min } => {
                    let dp = p - pos;
                    let dist = dp.len().max(r_min);
                    b += dp * (q / (dist * dist * dist));
                }
            }
        }
        b
    }

    /// grad(|B|^2), central differences. The force on a superparamagnetic
    /// particle is proportional to this.
    pub fn grad_b2(&self, p: Vec2) -> Vec2 {
        let b2 = |q: Vec2| self.b(q).len_sq();
        Vec2::new(
            (b2(p + Vec2::new(GRAD_EPS, 0.0)) - b2(p - Vec2::new(GRAD_EPS, 0.0))) / (2.0 * GRAD_EPS),
            (b2(p + Vec2::new(0.0, GRAD_EPS)) - b2(p - Vec2::new(0.0, GRAD_EPS))) / (2.0 * GRAD_EPS),
        )
    }
}
