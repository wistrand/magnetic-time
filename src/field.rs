//! Magnetic field of the hand magnets. Hands carry rigid, data-driven magnet
//! layouts; each frame the layouts are rotated into world space and expanded
//! into field elements (point dipoles and pole-face charges) that are summed.
//! See agent_docs/design-simulation.md for the model and units.

use crate::hands;
use crate::vec2::Vec2;

/// DEFAULT near-field clamp radius (dial-radius units). Fields diverge at
/// their sources; anything closer than the clamp is treated as being at the
/// clamp distance. The live value is SimParams::field_clamp (this const is
/// its default); the pointer magnet also uses this as its clamp floor.
/// Disc magnets clamp at their own radius when larger.
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

/// Parse a magnet layout list: "tip,alt:6,tip" per hand (hour,minute,second)
/// or one spec applied to all hands. Shared by the CLI --magnets flag and the
/// web component's magnets attribute.
pub fn parse_magnets(s: &str) -> Result<[LayoutSpec; 3], String> {
    let parts: Vec<&str> = s.split(',').collect();
    match parts.len() {
        1 => {
            let spec = LayoutSpec::parse(parts[0])?;
            Ok([spec; 3])
        }
        3 => Ok([
            LayoutSpec::parse(parts[0])?,
            LayoutSpec::parse(parts[1])?,
            LayoutSpec::parse(parts[2])?,
        ]),
        _ => Err("magnets takes one spec or three (hour,minute,second)".to_string()),
    }
}

/// Parse a shape list: one shape for all hands or "point,disc:0.05,rect:1x0.03".
pub fn parse_shapes(s: &str) -> Result<[SpecShape; 3], String> {
    let parts: Vec<&str> = s.split(',').collect();
    match parts.len() {
        1 => Ok([parse_shape(parts[0])?; 3]),
        3 => Ok([
            parse_shape(parts[0])?,
            parse_shape(parts[1])?,
            parse_shape(parts[2])?,
        ]),
        _ => Err("shapes takes one shape or three (hour,minute,second)".to_string()),
    }
}

/// Parse a strength list: "1.5" for all hands or "2,1,0.5" per hand.
pub fn parse_strengths(s: &str) -> Result<[f64; 3], String> {
    let vals: Result<Vec<f64>, _> = s.split(',').map(str::parse::<f64>).collect();
    let vals = vals.map_err(|e| format!("strengths: {e}"))?;
    match vals.len() {
        1 => Ok([vals[0]; 3]),
        3 => Ok([vals[0], vals[1], vals[2]]),
        _ => Err("strengths takes one value or three (hour,minute,second)".to_string()),
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

/// Seven-segment digital face: bar magnets laid out as HH:MM (or HH:MM:SS)
/// digits, the segments of each digit switched on by the current time. An
/// alternative to the rotating [hands]; the particles spell the readout.
/// Coordinates are dial units (center origin, y down), sized to fit the dial.
#[derive(Clone, Copy)]
pub struct SegClock {
    /// Per-segment bar moment magnitude.
    pub strength: f64,
    /// HH:MM:SS when true, HH:MM otherwise (smaller digits to fit).
    pub with_seconds: bool,
    /// Segment bar half-width, dial units.
    pub half_wid: f64,
}

impl Default for SegClock {
    fn default() -> Self {
        Self {
            strength: 0.14,
            with_seconds: false,
            half_wid: 0.02,
        }
    }
}

/// Active segments per digit 0-9, bit a=1,b=2,c=4,d=8,e=16,f=32,g=64.
const SEG_DIGITS: [u8; 10] = [
    0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F,
];

/// Segment offsets in (half-width, half-height) units from the digit center,
/// and whether the segment is horizontal. Order a,b,c,d,e,f,g (bits 0..6).
const SEG_GEOM: [(f64, f64, bool); 7] = [
    (0.0, -1.0, true),   // a top
    (1.0, -0.5, false),  // b upper right
    (1.0, 0.5, false),   // c lower right
    (0.0, 1.0, true),    // d bottom
    (-1.0, 0.5, false),  // e lower left
    (-1.0, -0.5, false), // f upper left
    (0.0, 0.0, true),    // g middle
];

impl SegClock {
    /// Digit and colon metrics (digit height, width, inter-glyph gap, colon
    /// cell width) for the current mode. Six digits need smaller glyphs.
    fn metrics(&self) -> (f64, f64, f64, f64) {
        if self.with_seconds {
            (0.42, 0.18, 0.06, 0.07)
        } else {
            (0.60, 0.27, 0.09, 0.10)
        }
    }

    /// World-space magnets (pos, moment, shape) for a display time. A digit
    /// contributes up to seven bar magnets; a colon contributes two dots.
    fn magnets_at(&self, time_secs: f64) -> Vec<(Vec2, Vec2, MagnetShape)> {
        const SHRINK: f64 = 0.8;
        // Collinear alternating sub-bars per segment (see the loop below).
        const SEG_SUB: usize = 3;
        let (dh, dw, gap, colon_w) = self.metrics();
        let (half_w, half_h) = (dw / 2.0, dh / 2.0);

        let total = time_secs.rem_euclid(24.0 * 3600.0);
        let h = (total / 3600.0) as u32 % 24;
        let mi = (total / 60.0) as u32 % 60;
        let s = total as u32 % 60;

        // Glyphs left to right; None = colon.
        let mut glyphs: Vec<Option<u8>> = vec![
            Some((h / 10) as u8),
            Some((h % 10) as u8),
            None,
            Some((mi / 10) as u8),
            Some((mi % 10) as u8),
        ];
        if self.with_seconds {
            glyphs.push(None);
            glyphs.push(Some((s / 10) as u8));
            glyphs.push(Some((s % 10) as u8));
        }

        let width = |g: &Option<u8>| if g.is_some() { dw } else { colon_w };
        let total_w: f64 =
            glyphs.iter().map(width).sum::<f64>() + gap * (glyphs.len() - 1) as f64;

        let mut out = Vec::new();
        let mut x = -total_w / 2.0;
        for g in &glyphs {
            let w = width(g);
            let cx = x + w / 2.0;
            match g {
                Some(d) => {
                    let bits = SEG_DIGITS[(*d as usize).min(9)];
                    for (i, &(sdx, sdy, horiz)) in SEG_GEOM.iter().enumerate() {
                        if bits & (1 << i) == 0 {
                            continue;
                        }
                        let center = Vec2::new(cx + sdx * half_w, sdy * half_h);
                        // A bar magnet has only two pole faces (its ends), so
                        // one bar per segment attracts particles to the
                        // segment's ends, not its length. Split each segment
                        // into SEG_SUB collinear bars of alternating polarity:
                        // the extra pole faces distribute along the segment so
                        // it fills instead of reading as two end nodes.
                        let (axis, full_len) = if horiz {
                            (Vec2::new(1.0, 0.0), half_w * SHRINK)
                        } else {
                            (Vec2::new(0.0, 1.0), (half_h / 2.0) * SHRINK)
                        };
                        let sub_half = full_len / SEG_SUB as f64;
                        for k in 0..SEG_SUB {
                            let t = -1.0 + (2 * k + 1) as f64 / SEG_SUB as f64;
                            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
                            out.push((
                                center + axis * (t * full_len),
                                axis * (sign * self.strength),
                                MagnetShape::Rect {
                                    half_len: sub_half,
                                    half_wid: self.half_wid,
                                },
                            ));
                        }
                    }
                }
                None => {
                    // Colon: two dots, magnetically lighter than the digits.
                    for sign in [-1.0, 1.0] {
                        out.push((
                            Vec2::new(cx, sign * half_h * 0.45),
                            Vec2::new(0.0, self.strength * 0.7),
                            MagnetShape::Disc {
                                radius: self.half_wid * 1.2,
                            },
                        ));
                    }
                }
            }
            x += w + gap;
        }

        // HH:MM has no seconds digits, so a disc magnet orbits the dial once
        // a minute as a seconds indicator, like the analog second hand but as
        // a moving disc (its wake reads the seconds off the particles).
        if !self.with_seconds {
            const TAU: f64 = std::f64::consts::TAU;
            const ORBIT_R: f64 = 0.85;
            let ang = time_secs.rem_euclid(60.0) / 60.0 * TAU - TAU / 4.0;
            let pos = Vec2::new(ang.cos() * ORBIT_R, ang.sin() * ORBIT_R);
            out.push((
                pos,
                pos.normalized() * (self.strength * 1.5),
                MagnetShape::Disc { radius: 0.05 },
            ));
        }
        out
    }
}

/// Which face drives the field: rotating hands or a digital readout.
pub enum Face {
    Hands([HandMagnets; 3]),
    Seg(SegClock),
}

/// Copy descriptor the CLI, dev panel, and web attributes edit; built into a
/// [Face] (which owns non-Copy [HandMagnets]) on change.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FaceKind {
    Hands,
    Seg,
}

/// Build the live [Face] from the current hand specs and seg config, chosen
/// by `kind`. `specs` is ignored in Seg mode, `seg` in Hands mode; both are
/// kept so switching modes preserves each one's settings.
pub fn build_face(kind: FaceKind, specs: &[LayoutSpec; 3], seg: SegClock) -> Face {
    match kind {
        FaceKind::Hands => Face::Hands(build_layouts(specs)),
        FaceKind::Seg => Face::Seg(seg),
    }
}

/// Expand one world-space magnet into field elements (and a debug marker).
/// Shared by both faces; `min_dist` is the near-field clamp for point/rect
/// elements (discs clamp at their own radius if larger).
fn expand(
    pos: Vec2,
    moment: Vec2,
    shape: MagnetShape,
    min_dist: f64,
    elements: &mut Vec<Element>,
    markers: &mut Vec<Marker>,
) {
    markers.push(Marker {
        pos,
        dir: moment.normalized(),
        shape,
    });
    match shape {
        MagnetShape::Point => elements.push(Element::Dipole {
            pos,
            moment,
            r_min: min_dist,
        }),
        MagnetShape::Disc { radius } => elements.push(Element::Dipole {
            pos,
            moment,
            r_min: radius.max(min_dist),
        }),
        MagnetShape::Rect { half_len, half_wid } => {
            // Two pole faces of distributed charge. Total charge chosen so
            // the far field matches a point dipole of the same moment:
            // q_total * 2*half_len = |moment|.
            let strength = moment.len();
            if strength < 1e-12 {
                return;
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
                        r_min: min_dist,
                    });
                }
            }
        }
    }
}

/// All hand magnets rotated into world space and expanded into field
/// elements for one display time.
pub struct FieldSources {
    elements: Vec<Element>,
    pub markers: Vec<Marker>,
    /// The interactive pointer magnet (pos, q, r_min) when active. Kept
    /// separately so the sim can attenuate it in the display/magnetization
    /// field: the pointer must be strong to exert force (F ~ grad|B|^2) but
    /// would otherwise saturate stroke color and orientation dish-wide.
    pointer: Option<(Vec2, f64, f64)>,
}

impl FieldSources {
    /// `min_dist` is the near-field clamp radius for point/rect elements
    /// (SimParams::field_clamp); discs clamp at their own radius if larger.
    pub fn at_time(face: &Face, time_secs: f64, min_dist: f64) -> Self {
        let mut elements = Vec::new();
        let mut markers = Vec::new();
        match face {
            Face::Hands(layouts) => {
                let angles = hands::angles(time_secs);
                for (layout, angle) in layouts.iter().zip(angles) {
                    let (c, s) = (angle.cos(), angle.sin());
                    let rot = |v: Vec2| Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c);
                    for mag in &layout.magnets {
                        expand(rot(mag.pos), rot(mag.moment), mag.shape, min_dist,
                            &mut elements, &mut markers);
                    }
                }
            }
            Face::Seg(cfg) => {
                for (pos, moment, shape) in cfg.magnets_at(time_secs) {
                    expand(pos, moment, shape, min_dist, &mut elements, &mut markers);
                }
            }
        }
        Self {
            elements,
            markers,
            pointer: None,
        }
    }

    /// Add the interactive pointer magnet: an axial disc magnet held against
    /// the dish, whose in-plane field is radial. Modeled as a single soft
    /// charge (pole face), clamped over the disc radius. Appended by the app
    /// after the hand elements whenever the pointer is down.
    pub fn add_pointer(&mut self, pos: Vec2, strength: f64, radius: f64) {
        let r_min = radius.max(MIN_DIST);
        self.elements.push(Element::Charge {
            pos,
            q: strength,
            r_min,
        });
        self.markers.push(Marker {
            pos,
            dir: Vec2::ZERO,
            shape: MagnetShape::Disc { radius },
        });
        self.pointer = Some((pos, strength, r_min));
    }

    /// The pointer magnet's own field contribution (zero when inactive).
    /// Must match the Element::Charge branch of `b()`.
    pub fn pointer_b(&self, p: Vec2) -> Vec2 {
        match self.pointer {
            None => Vec2::ZERO,
            Some((pos, q, r_min)) => {
                let dp = p - pos;
                let dist = dp.len().max(r_min);
                dp * (q / (dist * dist * dist))
            }
        }
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

    /// Field and grad(|B|^2) in one sweep: accumulate B and the Jacobian
    /// J_ik = dB_i/dr_k over all elements, then grad(|B|^2) = 2 J^T B. The
    /// force on a superparamagnetic particle is proportional to the gradient.
    /// Inside an element's r_min clamp, the derivative of the clamped field
    /// form is used, matching `b()` exactly.
    pub fn b_and_grad_b2(&self, p: Vec2) -> (Vec2, Vec2) {
        let mut b = Vec2::ZERO;
        // Jacobian accumulators; dipole terms are symmetric, the clamped
        // forms are not, so keep all four.
        let (mut jxx, mut jxy, mut jyx, mut jyy) = (0.0, 0.0, 0.0, 0.0);
        for e in &self.elements {
            match *e {
                Element::Dipole { pos, moment, r_min } => {
                    let dp = p - pos;
                    let len = dp.len();
                    if len >= r_min {
                        let d = len;
                        let n = dp / d;
                        let nm = n.dot(moment);
                        let d3 = d * d * d;
                        b += (n * (3.0 * nm) - moment) / d3;
                        // J = (3/d^4)[(n.m)I + n m^T + m n^T - 5(n.m) n n^T]
                        let c = 3.0 / (d3 * d);
                        jxx += c * (nm + 2.0 * n.x * moment.x - 5.0 * nm * n.x * n.x);
                        jyy += c * (nm + 2.0 * n.y * moment.y - 5.0 * nm * n.y * n.y);
                        let off =
                            c * (n.x * moment.y + moment.x * n.y - 5.0 * nm * n.x * n.y);
                        jxy += off;
                        jyx += off;
                    } else {
                        // Clamped form: B = 3 dp (dp.m)/r_min^5 - m/r_min^3.
                        let r3 = r_min * r_min * r_min;
                        let r5 = r3 * r_min * r_min;
                        let dm = dp.dot(moment);
                        b += dp * (3.0 * dm / r5) - moment / r3;
                        let c = 3.0 / r5;
                        jxx += c * (dm + dp.x * moment.x);
                        jyy += c * (dm + dp.y * moment.y);
                        jxy += c * dp.x * moment.y;
                        jyx += c * dp.y * moment.x;
                    }
                }
                Element::Charge { pos, q, r_min } => {
                    let dp = p - pos;
                    let len = dp.len();
                    if len >= r_min {
                        let d = len;
                        let n = dp / d;
                        let qd3 = q / (d * d * d);
                        b += dp * qd3;
                        // J = (q/d^3)(I - 3 n n^T)
                        jxx += qd3 * (1.0 - 3.0 * n.x * n.x);
                        jyy += qd3 * (1.0 - 3.0 * n.y * n.y);
                        let off = qd3 * (-3.0 * n.x * n.y);
                        jxy += off;
                        jyx += off;
                    } else {
                        // Clamped form: B = q dp / r_min^3, J = (q/r_min^3) I.
                        let qr3 = q / (r_min * r_min * r_min);
                        b += dp * qr3;
                        jxx += qr3;
                        jyy += qr3;
                    }
                }
            }
        }
        let grad = Vec2::new(
            2.0 * (b.x * jxx + b.y * jyx),
            2.0 * (b.x * jxy + b.y * jyy),
        );
        (b, grad)
    }

    /// grad(|B|^2), analytic.
    pub fn grad_b2(&self, p: Vec2) -> Vec2 {
        self.b_and_grad_b2(p).1
    }

    /// Central-difference reference for --grad-check; keep in sync with the
    /// analytic version when adding element types.
    pub fn grad_b2_numeric(&self, p: Vec2) -> Vec2 {
        let b2 = |q: Vec2| self.b(q).len_sq();
        Vec2::new(
            (b2(p + Vec2::new(GRAD_EPS, 0.0)) - b2(p - Vec2::new(GRAD_EPS, 0.0))) / (2.0 * GRAD_EPS),
            (b2(p + Vec2::new(0.0, GRAD_EPS)) - b2(p - Vec2::new(0.0, GRAD_EPS))) / (2.0 * GRAD_EPS),
        )
    }
}
