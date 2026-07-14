//! Interactive eframe app: renders the shared framebuffer to a texture each
//! frame, plus the dev side panel.

use std::path::PathBuf;

use eframe::egui;

use crate::clock::{format_time, ClockSource};
use crate::render::{draw_clock, write_png, Framebuffer, BG};

pub struct ClockApp {
    clock: ClockSource,
    speed: f64,
    fb: Framebuffer,
    texture: Option<egui::TextureHandle>,
    dump_status: Option<String>,
}

impl ClockApp {
    pub fn new(clock: ClockSource) -> Self {
        let speed = clock.multiplier();
        Self {
            clock,
            speed,
            fb: Framebuffer::new(8, 8),
            texture: None,
            dump_status: None,
        }
    }

    fn dump_frame(&mut self) {
        let path = PathBuf::from("docs/debug/interactive.png");
        self.dump_status = Some(match write_png(&path, &self.fb) {
            Ok(()) => format!("wrote {}", path.display()),
            Err(e) => format!("dump failed: {e}"),
        });
    }
}

impl eframe::App for ClockApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::right("dev")
            .resizable(false)
            .default_width(180.0)
            .show(ctx, |ui| {
                ui.heading("dev");
                ui.label(format!("time  {}", format_time(self.clock.now())));
                ui.add(
                    egui::Slider::new(&mut self.speed, 0.1..=10000.0)
                        .logarithmic(true)
                        .text("speed"),
                );
                if (self.speed - self.clock.multiplier()).abs() > f64::EPSILON {
                    self.clock.set_multiplier(self.speed);
                }
                if ui.button("dump frame").clicked() {
                    self.dump_frame();
                }
                if let Some(status) = &self.dump_status {
                    ui.label(status.clone());
                }
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(BG[0], BG[1], BG[2])))
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                let side_pts = avail.width().min(avail.height()).max(64.0);
                let ppp = ctx.pixels_per_point();
                let px = (side_pts * ppp).round().max(64.0) as u32;

                self.fb.resize(px, px);
                draw_clock(&mut self.fb, self.clock.now());

                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [px as usize, px as usize],
                    &self.fb.pixels,
                );
                match &mut self.texture {
                    Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                    None => {
                        self.texture =
                            Some(ctx.load_texture("clock", image, egui::TextureOptions::LINEAR))
                    }
                }
                let tex = self.texture.as_ref().unwrap();

                let rect = egui::Rect::from_center_size(
                    avail.center(),
                    egui::vec2(side_pts, side_pts),
                );
                ui.painter().image(
                    tex.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            });

        // Idle egui repaints only on input; without this the clock freezes.
        ctx.request_repaint();
    }
}
