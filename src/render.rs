//! Software rasterizer and clock-face drawing. This is the shared render path:
//! both the interactive window and the headless dump draw through here, so
//! dumped bitmaps are faithful to the screen (see CLAUDE.md invariants).

use std::io::BufWriter;
use std::path::Path;

const TAU: f64 = std::f64::consts::TAU;

pub type Color = [u8; 4];

pub const BG: Color = [16, 18, 26, 255];
const DIAL: Color = [24, 27, 38, 255];
const RIM: Color = [90, 96, 120, 255];
const TICK_MAJOR: Color = [185, 190, 205, 255];
const TICK_MINOR: Color = [105, 110, 130, 255];
const HAND: Color = [225, 228, 238, 255];
const HAND_SECOND: Color = [225, 75, 60, 255];

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

/// Draw the full clock (face and hands) for a display time in seconds since
/// midnight. Layout is proportional to the buffer size.
pub fn draw_clock(fb: &mut Framebuffer, time_secs: f64) {
    fb.clear(BG);
    let c = fb.width.min(fb.height) as f64 / 2.0;
    let (cx, cy) = (fb.width as f64 / 2.0, fb.height as f64 / 2.0);
    let r = c * 0.94;

    fb.disc(cx, cy, r, DIAL);
    fb.ring(cx, cy, r, r * 0.02, RIM);

    // Ticks: 60 minor, every fifth major.
    for i in 0..60 {
        let a = i as f64 / 60.0 * TAU - TAU / 4.0;
        let (major, r0, hw) = if i % 5 == 0 {
            (true, 0.88, r * 0.010)
        } else {
            (false, 0.93, r * 0.004)
        };
        let color = if major { TICK_MAJOR } else { TICK_MINOR };
        fb.capsule(
            cx + a.cos() * r * r0,
            cy + a.sin() * r * r0,
            cx + a.cos() * r * 0.97,
            cy + a.sin() * r * 0.97,
            hw,
            color,
        );
    }

    // Hand angles, all smooth (no ticking).
    let s = time_secs % 60.0;
    let m = (time_secs / 60.0) % 60.0;
    let h = (time_secs / 3600.0) % 12.0;
    let hand = |fb: &mut Framebuffer, frac: f64, len: f64, tail: f64, hw: f64, color: Color| {
        let a = frac * TAU - TAU / 4.0;
        fb.capsule(
            cx - a.cos() * r * tail,
            cy - a.sin() * r * tail,
            cx + a.cos() * r * len,
            cy + a.sin() * r * len,
            hw,
            color,
        );
    };
    hand(fb, h / 12.0, 0.52, 0.06, r * 0.030, HAND);
    hand(fb, m / 60.0, 0.78, 0.06, r * 0.020, HAND);
    hand(fb, s / 60.0, 0.88, 0.14, r * 0.007, HAND_SECOND);

    // Hub.
    fb.disc(cx, cy, r * 0.028, HAND);
    fb.disc(cx, cy, r * 0.014, HAND_SECOND);
}

/// Write the framebuffer as a PNG, creating parent directories.
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
