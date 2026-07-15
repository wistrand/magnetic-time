//! Software rasterizer and clock-face drawing. This is the shared render path:
//! both the interactive window and the headless dump draw through here, so
//! dumped bitmaps are faithful to the screen (see CLAUDE.md invariants).

#[cfg(not(target_arch = "wasm32"))]
use std::io::BufWriter;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use crate::field::FieldSources;
use crate::hands;
use crate::sim::Sim;
use crate::vec2::Vec2;

const TAU: f64 = std::f64::consts::TAU;

pub type Color = [u8; 4];

/// Default background (the original dark look).
pub const DEFAULT_BG: [u8; 3] = [16, 18, 26];

/// Face colors derived from the background so any bg works: contrast colors
/// lerp toward white on dark backgrounds and toward black on light ones.
/// `dark` also selects the particle blend mode (additive glow vs subtractive
/// ink).
struct Theme {
    bg: Color,
    dial: Color,
    rim: Color,
    tick_major: Color,
    tick_minor: Color,
    hand: Color,
    second: Color,
    dark: bool,
}

impl Theme {
    fn from_bg(bg: [u8; 3]) -> Self {
        let lum = 0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32;
        let dark = lum < 128.0;
        let target = if dark { 255.0 } else { 0.0 };
        let toward = |t: f32| -> Color {
            let c = bg.map(|v| (v as f32 + (target - v as f32) * t) as u8);
            [c[0], c[1], c[2], 255]
        };
        Self {
            bg: [bg[0], bg[1], bg[2], 255],
            dial: toward(0.05),
            rim: toward(0.35),
            tick_major: toward(0.68),
            tick_minor: toward(0.38),
            hand: toward(0.88),
            second: if dark {
                [225, 75, 60, 255]
            } else {
                [195, 40, 30, 255]
            },
            dark,
        }
    }
}

/// Parse "rrggbb" or "#rrggbb" into a color.
pub fn parse_color(s: &str) -> Result<[u8; 3], String> {
    let hex = s.trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("bad color '{s}', expected rrggbb hex"));
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
    Ok([byte(0), byte(2), byte(4)])
}
const QUIVER: Color = [80, 200, 255, 255];
const POLE_N: Color = [235, 70, 70, 255];
const POLE_S: Color = [70, 110, 245, 255];
const HASH_CELL: [u8; 3] = [120, 255, 150];

/// Particle color scale: `base` for unmagnetized dots, lerped toward `hot`
/// as magnetization saturates. Additive blending pushes dense areas toward
/// white regardless, so palettes read as a tint, not an absolute color.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    Ice,
    Ember,
    Emerald,
    Violet,
    Mono,
}

impl Palette {
    pub const ALL: [Palette; 5] = [
        Palette::Ice,
        Palette::Ember,
        Palette::Emerald,
        Palette::Violet,
        Palette::Mono,
    ];

    pub fn parse(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|p| p.name() == s)
            .ok_or_else(|| format!("unknown palette '{s}' (ice, ember, emerald, violet, mono)"))
    }

    pub fn name(self) -> &'static str {
        match self {
            Palette::Ice => "ice",
            Palette::Ember => "ember",
            Palette::Emerald => "emerald",
            Palette::Violet => "violet",
            Palette::Mono => "mono",
        }
    }

    fn base(self) -> [u8; 3] {
        match self {
            Palette::Ice => [125, 170, 255],
            Palette::Ember => [255, 120, 40],
            Palette::Emerald => [70, 215, 140],
            Palette::Violet => [185, 110, 255],
            Palette::Mono => [160, 165, 180],
        }
    }

    /// Stroke color at full magnetization. On dark backgrounds strokes glow
    /// toward near-white; on light ones they deepen toward a dark saturated
    /// tone (near-white ink would be invisible).
    fn hot(self, dark: bool) -> [u8; 3] {
        if dark {
            match self {
                Palette::Ice => [230, 240, 255],
                Palette::Ember => [255, 235, 190],
                Palette::Emerald => [225, 255, 235],
                Palette::Violet => [245, 225, 255],
                Palette::Mono => [255, 255, 255],
            }
        } else {
            match self {
                Palette::Ice => [30, 60, 160],
                Palette::Ember => [180, 60, 10],
                Palette::Emerald => [10, 120, 60],
                Palette::Violet => [110, 30, 170],
                Palette::Mono => [20, 20, 25],
            }
        }
    }
}

/// Visual tunables that don't affect the simulation.
#[derive(Clone, Copy)]
pub struct Style {
    /// Stroke length multiplier for magnetized particles; 0 draws dots only.
    pub stroke_len: f64,
    /// Draw the hands and hub (the field ignores this; magnets keep moving).
    pub show_hands: bool,
    /// Particle color scale.
    pub palette: Palette,
    /// Background color; all face colors and the particle blend mode derive
    /// from it (see Theme).
    pub bg: [u8; 3],
    /// Interactive render-buffer cap (pixels per side); 0 = native
    /// resolution. The texture upscales linearly, trading sharpness for
    /// raster cost. Headless --size is unaffected.
    pub max_px: u32,
}

// Part of the owner-tuned "rings" preset: hands hidden, time read from the
// particle structures alone. --show-hands turns them back on.
impl Default for Style {
    fn default() -> Self {
        Self {
            stroke_len: 0.6,
            show_hands: false,
            palette: Palette::Ice,
            bg: DEFAULT_BG,
            max_px: 700,
        }
    }
}

/// Toggleable debug overlays.
#[derive(Clone, Copy, Default)]
pub struct DebugViews {
    /// |B| heatmap under the hands.
    pub field: bool,
    /// grad(|B|^2) arrows over the hands.
    pub quiver: bool,
    /// Dipole position/polarity markers.
    pub dipoles: bool,
    /// Color particles by speed instead of the normal look.
    pub velocity: bool,
    /// Spatial-hash occupancy overlay.
    pub hash: bool,
    /// Lines between chain-interacting pairs.
    pub chains: bool,
}

impl DebugViews {
    /// Parse a comma-separated view list ("field,quiver").
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut v = Self::default();
        for name in s.split(',').filter(|n| !n.is_empty()) {
            match name {
                "field" => v.field = true,
                "quiver" => v.quiver = true,
                "dipoles" => v.dipoles = true,
                "velocity" => v.velocity = true,
                "hash" => v.hash = true,
                "chains" => v.chains = true,
                other => {
                    return Err(format!(
                        "unknown view '{other}' (field, quiver, dipoles, velocity, hash, chains)"
                    ))
                }
            }
        }
        Ok(v)
    }
}

pub struct Framebuffer {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major.
    pub pixels: Vec<u8>,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            self.pixels = vec![0; (width * height * 4) as usize];
        }
    }

    pub fn clear(&mut self, color: Color) {
        for px in self.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&color);
        }
    }

    /// Source-over blend of `color` at pixel (x, y) with coverage 0..=1.
    fn blend(&mut self, x: i32, y: i32, color: Color, coverage: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let a = (color[3] as f32 / 255.0) * coverage.clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        for c in 0..3 {
            let dst = self.pixels[i + c] as f32;
            self.pixels[i + c] = (color[c] as f32 * a + dst * (1.0 - a)).round() as u8;
        }
        let dst_a = self.pixels[i + 3] as f32 / 255.0;
        self.pixels[i + 3] = ((a + dst_a * (1.0 - a)) * 255.0).round() as u8;
    }

    /// Rasterize any shape given as a signed distance function (negative
    /// inside), with 1px anti-aliased edge, over an integer bounding box.
    fn fill_sdf(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        color: Color,
        sdf: impl Fn(f64, f64) -> f64,
    ) {
        let xa = (x0.floor() as i32).max(0);
        let ya = (y0.floor() as i32).max(0);
        let xb = (x1.ceil() as i32).min(self.width as i32 - 1);
        let yb = (y1.ceil() as i32).min(self.height as i32 - 1);
        for y in ya..=yb {
            for x in xa..=xb {
                let d = sdf(x as f64 + 0.5, y as f64 + 0.5);
                let cov = (0.5 - d) as f32;
                if cov > 0.0 {
                    self.blend(x, y, color, cov);
                }
            }
        }
    }

    /// Additive (saturating) blend of an RGB color scaled by `f` 0..=1.
    fn blend_add(&mut self, x: i32, y: i32, color: [u8; 3], f: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || f <= 0.0 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        for c in 0..3 {
            let v = self.pixels[i + c] as f32 + color[c] as f32 * f.min(1.0);
            self.pixels[i + c] = v.min(255.0) as u8;
        }
        self.pixels[i + 3] = 255;
    }

    /// Particle "ink" blend: additive glow on dark backgrounds, subtractive
    /// ink on light ones (subtracting the color's complement tints the pixel
    /// toward the color, darkening as it accumulates).
    fn blend_ink(&mut self, x: i32, y: i32, color: [u8; 3], f: f32, dark: bool) {
        if dark {
            self.blend_add(x, y, color, f);
            return;
        }
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || f <= 0.0 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        for c in 0..3 {
            let v = self.pixels[i + c] as f32 - (255.0 - color[c] as f32) * f.min(1.0);
            self.pixels[i + c] = v.max(0.0) as u8;
        }
        self.pixels[i + 3] = 255;
    }

    /// Soft particle dot: intensity `gain` at the center falling to 0 at
    /// radius r, blended with the ink mode (additive on dark, subtractive on
    /// light). Overlapping dots saturate instead of clipping.
    pub fn dot_ink(&mut self, cx: f64, cy: f64, r: f64, color: [u8; 3], gain: f32, dark: bool) {
        let xa = ((cx - r).floor() as i32).max(0);
        let ya = ((cy - r).floor() as i32).max(0);
        let xb = ((cx + r).ceil() as i32).min(self.width as i32 - 1);
        let yb = ((cy + r).ceil() as i32).min(self.height as i32 - 1);
        for y in ya..=yb {
            for x in xa..=xb {
                let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                let f = ((1.0 - d / r).max(0.0)) as f32 * gain;
                self.blend_ink(x, y, color, f, dark);
            }
        }
    }

    /// Soft particle stroke: a capsule with intensity `gain` on the axis
    /// falling to 0 at half-width `hw`, blended with the ink mode.
    #[allow(clippy::too_many_arguments)]
    pub fn capsule_ink(
        &mut self,
        ax: f64,
        ay: f64,
        bx: f64,
        by: f64,
        hw: f64,
        color: [u8; 3],
        gain: f32,
        dark: bool,
    ) {
        let pad = hw + 1.0;
        let xa = ((ax.min(bx) - pad).floor() as i32).max(0);
        let ya = ((ay.min(by) - pad).floor() as i32).max(0);
        let xb = ((ax.max(bx) + pad).ceil() as i32).min(self.width as i32 - 1);
        let yb = ((ay.max(by) + pad).ceil() as i32).min(self.height as i32 - 1);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = (dx * dx + dy * dy).max(1e-12);
        for y in ya..=yb {
            for x in xa..=xb {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
                let d = ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt();
                let f = ((1.0 - d / hw).max(0.0)) as f32 * gain;
                self.blend_ink(x, y, color, f, dark);
            }
        }
    }

    pub fn disc(&mut self, cx: f64, cy: f64, r: f64, color: Color) {
        self.fill_sdf(cx - r - 1.0, cy - r - 1.0, cx + r + 1.0, cy + r + 1.0, color, |x, y| {
            ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r
        });
    }

    pub fn ring(&mut self, cx: f64, cy: f64, r: f64, thickness: f64, color: Color) {
        let out = r + thickness / 2.0 + 1.0;
        self.fill_sdf(cx - out, cy - out, cx + out, cy + out, color, |x, y| {
            (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r).abs() - thickness / 2.0
        });
    }

    /// Line segment with round caps (capsule), `hw` = half width.
    pub fn capsule(&mut self, ax: f64, ay: f64, bx: f64, by: f64, hw: f64, color: Color) {
        let pad = hw + 1.0;
        let (x0, x1) = (ax.min(bx) - pad, ax.max(bx) + pad);
        let (y0, y1) = (ay.min(by) - pad, ay.max(by) + pad);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = (dx * dx + dy * dy).max(1e-12);
        self.fill_sdf(x0, y0, x1, y1, color, |x, y| {
            let t = (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0);
            let (px, py) = (ax + t * dx, ay + t * dy);
            ((x - px).powi(2) + (y - py).powi(2)).sqrt() - hw
        });
    }
}

/// Pixel mapping for clock-face units (center origin, dial radius 1.0).
struct Map {
    cx: f64,
    cy: f64,
    r: f64,
}

impl Map {
    fn of(fb: &Framebuffer) -> Self {
        let c = fb.width.min(fb.height) as f64 / 2.0;
        Self {
            cx: fb.width as f64 / 2.0,
            cy: fb.height as f64 / 2.0,
            r: c * 0.94,
        }
    }

    fn px(&self, p: Vec2) -> (f64, f64) {
        (self.cx + p.x * self.r, self.cy + p.y * self.r)
    }

    /// Pixel center back to clock-face units.
    fn world(&self, x: usize, y: usize) -> Vec2 {
        Vec2::new(
            (x as f64 + 0.5 - self.cx) / self.r,
            (y as f64 + 0.5 - self.cy) / self.r,
        )
    }
}

/// Draw the full clock (face, debug overlays, hands, particles) for a display
/// time in seconds since midnight. Layout is proportional to the buffer size.
pub fn draw_clock(
    fb: &mut Framebuffer,
    time_secs: f64,
    sources: &FieldSources,
    views: DebugViews,
    style: Style,
    sim: Option<&Sim>,
) {
    let theme = Theme::from_bg(style.bg);
    fb.clear(theme.bg);
    let m = Map::of(fb);
    let (cx, cy, r) = (m.cx, m.cy, m.r);

    fb.disc(cx, cy, r, theme.dial);
    fb.ring(cx, cy, r, r * 0.02, theme.rim);

    // Ticks: 60 minor, every fifth major.
    for i in 0..60 {
        let a = i as f64 / 60.0 * TAU - TAU / 4.0;
        let (major, r0, hw) = if i % 5 == 0 {
            (true, 0.88, r * 0.010)
        } else {
            (false, 0.93, r * 0.004)
        };
        let color = if major { theme.tick_major } else { theme.tick_minor };
        fb.capsule(
            cx + a.cos() * r * r0,
            cy + a.sin() * r * r0,
            cx + a.cos() * r * 0.97,
            cy + a.sin() * r * 0.97,
            hw,
            color,
        );
    }

    if views.field {
        draw_field_heatmap(fb, &m, sources);
    }

    if style.show_hands {
        let angles = hands::angles(time_secs);
        let widths = [r * 0.030, r * 0.020, r * 0.007];
        let tails = [0.06, 0.06, 0.14];
        let colors = [theme.hand, theme.hand, theme.second];
        for i in 0..3 {
            let a = angles[i];
            fb.capsule(
                cx - a.cos() * r * tails[i],
                cy - a.sin() * r * tails[i],
                cx + a.cos() * r * hands::LEN[i],
                cy + a.sin() * r * hands::LEN[i],
                widths[i],
                colors[i],
            );
        }
        fb.disc(cx, cy, r * 0.028, theme.hand);
        fb.disc(cx, cy, r * 0.014, theme.second);
    }

    // Particles float above the hands.
    if let Some(sim) = sim {
        if views.chains {
            for (a, b) in sim.chain_bonds() {
                let (ax, ay) = m.px(a);
                let (bx, by) = m.px(b);
                fb.capsule_ink(ax, ay, bx, by, 1.0, HASH_CELL, 0.5, theme.dark);
            }
        }
        draw_particles(fb, &m, sim, views, style, theme.dark);
        if views.hash {
            draw_hash_cells(fb, &m, sim);
        }
    }

    if views.quiver {
        draw_quiver(fb, &m, sources);
    }
    if views.dipoles {
        draw_dipole_markers(fb, &m, sources);
    }
}

/// |B| heatmap over the dial, log-scaled and self-normalized per frame.
/// Debug-only view, so the per-frame normalization (which shifts as hands
/// move) is fine.
fn draw_field_heatmap(fb: &mut Framebuffer, m: &Map, sources: &FieldSources) {
    let (w, h) = (fb.width as usize, fb.height as usize);
    let mut vals = vec![0.0f32; w * h];
    let mut vmax = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            let p = m.world(x, y);
            if p.len_sq() > 0.96 * 0.96 {
                continue;
            }
            let v = sources.b(p).len() as f32;
            vals[y * w + x] = v;
            vmax = vmax.max(v);
        }
    }
    if vmax <= 0.0 {
        return;
    }
    let norm = (1.0 + vmax).ln();
    for y in 0..h {
        for x in 0..w {
            let v = vals[y * w + x];
            if v <= 0.0 {
                continue;
            }
            let t = (1.0 + v).ln() / norm;
            fb.blend(x as i32, y as i32, heat_color(t), t.min(1.0) * 0.9);
        }
    }
}

/// Dark-to-hot colormap for t in 0..=1.
fn heat_color(t: f32) -> Color {
    const STOPS: [(f32, [f32; 3]); 4] = [
        (0.0, [10.0, 10.0, 35.0]),
        (0.35, [70.0, 25.0, 120.0]),
        (0.7, [210.0, 85.0, 40.0]),
        (1.0, [255.0, 235.0, 160.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    let mut c = STOPS[STOPS.len() - 1].1;
    for i in 0..STOPS.len() - 1 {
        let (t0, c0) = STOPS[i];
        let (t1, c1) = STOPS[i + 1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            c = [0, 1, 2].map(|k| c0[k] + (c1[k] - c0[k]) * f);
            break;
        }
    }
    [c[0] as u8, c[1] as u8, c[2] as u8, 255]
}

/// grad(|B|^2) direction arrows on a grid, length log-scaled and
/// self-normalized per frame.
fn draw_quiver(fb: &mut Framebuffer, m: &Map, sources: &FieldSources) {
    const N: i32 = 24;
    let step = 2.0 / N as f64;
    let mut arrows = Vec::new();
    let mut gmax = 0.0f64;
    for gy in 0..N {
        for gx in 0..N {
            let p = Vec2::new(-1.0 + (gx as f64 + 0.5) * step, -1.0 + (gy as f64 + 0.5) * step);
            if p.len_sq() > 0.92 * 0.92 {
                continue;
            }
            let g = sources.grad_b2(p);
            gmax = gmax.max(g.len());
            arrows.push((p, g));
        }
    }
    if gmax <= 0.0 {
        return;
    }
    let norm = (1.0 + gmax).ln();
    for (p, g) in arrows {
        let mag = g.len();
        if mag <= 0.0 {
            continue;
        }
        let len = step * 0.85 * (1.0 + mag).ln() / norm;
        let tip = p + g.normalized() * len;
        let (ax, ay) = m.px(p);
        let (bx, by) = m.px(tip);
        fb.capsule(ax, ay, bx, by, m.r * 0.002, QUIVER);
        fb.disc(bx, by, m.r * 0.005, QUIVER);
    }
}

fn draw_particles(
    fb: &mut Framebuffer,
    m: &Map,
    sim: &Sim,
    views: DebugViews,
    style: Style,
    dark: bool,
) {
    let pr = (m.r * 0.006).max(1.3);
    let max_speed = sim.params.max_speed;
    let base = style.palette.base();
    let hot = style.palette.hot(dark);
    for i in 0..sim.pos.len() {
        let (x, y) = m.px(sim.pos[i]);
        if views.velocity {
            let t = (sim.vel[i].len() / max_speed).min(1.0) as f32;
            let c = heat_color(t);
            fb.dot_ink(x, y, pr, [c[0], c[1], c[2]], 0.9, dark);
            continue;
        }
        let w = sim.field[i].w_disp as f32;
        if w > 0.15 && style.stroke_len > 0.0 {
            // Magnetized: a short stroke along the local field. Aligned
            // neighbors visually fuse into chains / spike-like filaments.
            let c = [0, 1, 2]
                .map(|k| (base[k] as f32 + (hot[k] as f32 - base[k] as f32) * w) as u8);
            let hl = pr * (1.2 + 2.6 * w as f64) * style.stroke_len;
            let d = sim.field[i].dir;
            let (dx, dy) = (d.x * hl, d.y * hl);
            fb.capsule_ink(x - dx, y - dy, x + dx, y + dy, pr * 0.6, c, 0.4 + 0.35 * w, dark);
        } else {
            fb.dot_ink(x, y, pr, base, 0.55, dark);
        }
    }
}

/// Spatial-hash occupancy: brighter cell = more particles.
fn draw_hash_cells(fb: &mut Framebuffer, m: &Map, sim: &Sim) {
    let dims = sim.hash_dims();
    let cell_w = 2.0 / dims as f64;
    for (gx, gy, n) in sim.hash_cells() {
        let p0 = Vec2::new(-1.0 + gx as f64 * cell_w, -1.0 + gy as f64 * cell_w);
        let (x0, y0) = m.px(p0);
        let (x1, y1) = m.px(p0 + Vec2::new(cell_w, cell_w));
        let f = (n as f32 / 16.0).min(1.0) * 0.45;
        for y in (y0 as i32).max(0)..(y1 as i32).min(fb.height as i32) {
            for x in (x0 as i32).max(0)..(x1 as i32).min(fb.width as i32) {
                fb.blend_add(x, y, HASH_CELL, f);
            }
        }
    }
}

/// One marker per magnet: red (north) / blue (south) stubs along the moment
/// axis, plus the magnet's physical outline for extended shapes.
fn draw_dipole_markers(fb: &mut Framebuffer, m: &Map, sources: &FieldSources) {
    use crate::field::MagnetShape;
    for mk in &sources.markers {
        let (x, y) = m.px(mk.pos);
        let (nx, ny) = m.px(mk.pos + mk.dir * 0.05);
        let (sx, sy) = m.px(mk.pos - mk.dir * 0.05);
        fb.capsule(x, y, nx, ny, m.r * 0.006, POLE_N);
        fb.capsule(x, y, sx, sy, m.r * 0.006, POLE_S);
        fb.disc(x, y, m.r * 0.009, [255, 255, 255, 255]);
        match mk.shape {
            MagnetShape::Point => {}
            MagnetShape::Disc { radius } => {
                fb.ring(x, y, radius * m.r, m.r * 0.004, [255, 255, 255, 255]);
            }
            MagnetShape::Rect { half_len, half_wid } => {
                let perp = crate::vec2::Vec2::new(-mk.dir.y, mk.dir.x);
                let corner = |a: f64, b: f64| m.px(mk.pos + mk.dir * a + perp * b);
                let (ax, ay) = corner(half_len, half_wid);
                let (bx, by) = corner(half_len, -half_wid);
                let (cx2, cy2) = corner(-half_len, -half_wid);
                let (dx2, dy2) = corner(-half_len, half_wid);
                // North face red, south face blue, sides white.
                fb.capsule(ax, ay, bx, by, m.r * 0.004, POLE_N);
                fb.capsule(cx2, cy2, dx2, dy2, m.r * 0.004, POLE_S);
                fb.capsule(bx, by, cx2, cy2, m.r * 0.003, [255, 255, 255, 255]);
                fb.capsule(dx2, dy2, ax, ay, m.r * 0.003, [255, 255, 255, 255]);
            }
        }
    }
}

/// Write the framebuffer as a PNG, creating parent directories.
/// Native only; the browser build has no filesystem.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_png(path: &Path, fb: &Framebuffer) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        }
    }
    let file = std::fs::File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), fb.width, fb.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&fb.pixels).map_err(|e| e.to_string())?;
    Ok(())
}
