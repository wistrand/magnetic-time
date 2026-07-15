//! Interactive eframe app: renders the shared framebuffer to a texture each
//! frame, plus the dev side panel.

use eframe::egui;

use crate::clock::{format_time, ClockSource};
use crate::field::{
    build_layouts, FieldSources, HandMagnets, LayoutSpec, MagnetKind, SpecShape,
};
use crate::render::{draw_clock, DebugViews, Framebuffer, Style};
use crate::sim::{Sim, SimParams};
use crate::vec2::Vec2;

/// Wall-clock budget for catch-up physics per frame. If stepping to "now"
/// would take longer (huge speed multiplier or a stall), the particles skip
/// the excess display time; the hands stay truthful to the clock.
const STEP_BUDGET: web_time::Duration = web_time::Duration::from_millis(12);

/// A complete externally-set configuration, applied live. Used by the web
/// component (attribute changes land here); native runs never push one.
#[derive(Clone, Copy)]
pub struct AppConfig {
    pub specs: [LayoutSpec; 3],
    pub style: Style,
    pub speed: f64,
    pub sim: SimParams,
    pub show_panel: bool,
}

/// Single-slot channel for pushing an AppConfig into a running app.
pub type PendingConfig = std::rc::Rc<std::cell::RefCell<Option<AppConfig>>>;

pub struct ClockApp {
    clock: ClockSource,
    speed: f64,
    specs: [LayoutSpec; 3],
    layouts: [HandMagnets; 3],
    views: DebugViews,
    style: Style,
    show_panel: bool,
    /// External config updates, drained each frame.
    pending: Option<PendingConfig>,
    /// Active pointer magnet: position in clock units plus screen position
    /// for the feedback ring. Set while the primary button/touch is down
    /// over the dial.
    pointer: Option<(Vec2, egui::Pos2)>,
    sim: Sim,
    /// Display time the sim has been stepped to.
    sim_time: f64,
    fb: Framebuffer,
    texture: Option<egui::TextureHandle>,
    dump_status: Option<String>,
}

impl ClockApp {
    pub fn new(
        clock: ClockSource,
        views: DebugViews,
        style: Style,
        params: SimParams,
        specs: [LayoutSpec; 3],
        show_panel: bool,
        pending: Option<PendingConfig>,
    ) -> Self {
        let speed = clock.multiplier();
        let sim_time = clock.now();
        Self {
            clock,
            speed,
            specs,
            layouts: build_layouts(&specs),
            views,
            style,
            show_panel,
            pending,
            pointer: None,
            sim: Sim::new(params),
            sim_time,
            fb: Framebuffer::new(8, 8),
            texture: None,
            dump_status: None,
        }
    }

    /// Apply an externally pushed configuration, preserving particle state
    /// (count changes go through Sim::set_count).
    fn apply_config(&mut self, cfg: AppConfig) {
        self.specs = cfg.specs;
        self.layouts = build_layouts(&self.specs);
        self.style = cfg.style;
        self.show_panel = cfg.show_panel;
        if (cfg.speed - self.clock.multiplier()).abs() > f64::EPSILON {
            self.clock.set_multiplier(cfg.speed);
        }
        self.speed = cfg.speed;
        let cur_count = self.sim.params.count;
        self.sim.params = SimParams {
            count: cur_count,
            ..cfg.sim
        };
        if cfg.sim.count != cur_count {
            self.sim.set_count(cfg.sim.count);
        }
    }

    /// Field sources for a display time, including the pointer magnet while
    /// it is down.
    fn sources_at(&self, t: f64) -> FieldSources {
        let mut sources =
            FieldSources::at_time(&self.layouts, t, self.sim.params.field_clamp);
        if let Some((world, _)) = self.pointer {
            let p = &self.sim.params;
            if p.pointer_strength > 0.0 {
                sources.add_pointer(world, p.pointer_strength, p.pointer_radius);
            }
        }
        sources
    }

    /// Step the sim in fixed dt up to the current display time, bounded by a
    /// wall-clock budget.
    fn step_sim_to(&mut self, now: f64) {
        let dt = self.sim.params.dt;
        // Display time since last sim step, midnight wrap handled.
        let gap = (now - self.sim_time).rem_euclid(24.0 * 3600.0);
        let steps = (gap / dt) as usize;
        let start = web_time::Instant::now();
        for _ in 0..steps {
            if start.elapsed() > STEP_BUDGET {
                // Out of budget: drop the remaining display time.
                self.sim_time = now;
                return;
            }
            let sources = self.sources_at(self.sim_time);
            self.sim.step(&sources);
            self.sim_time += dt;
        }
        self.sim_time = self.sim_time.rem_euclid(24.0 * 3600.0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dump_frame(&mut self) {
        let path = std::path::PathBuf::from("docs/debug/interactive.png");
        self.dump_status = Some(match crate::render::write_png(&path, &self.fb) {
            Ok(()) => format!("wrote {}", path.display()),
            Err(e) => format!("dump failed: {e}"),
        });
    }

    fn dev_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("dev")
            .resizable(false)
            .default_width(180.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.dev_panel_contents(ui);
                });
            });
    }

    fn dev_panel_contents(&mut self, ui: &mut egui::Ui) {
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
                ui.separator();
                ui.label("magnets");
                let mut specs_changed = false;
                for (i, name) in ["hour", "minute", "second"].iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(*name);
                        let spec = &mut self.specs[i];
                        egui::ComboBox::from_id_salt(("magnets", i))
                            .selected_text(spec.label())
                            .show_ui(ui, |ui| {
                                for (kind, label) in [
                                    (MagnetKind::Tip, "tip"),
                                    (MagnetKind::Strip, "strip"),
                                    (MagnetKind::Alt, "alt"),
                                ] {
                                    if ui
                                        .selectable_value(&mut spec.kind, kind, label)
                                        .changed()
                                    {
                                        specs_changed = true;
                                    }
                                }
                            });
                        if spec.kind != MagnetKind::Tip {
                            let mut n = spec.n.max(2);
                            if ui
                                .add(egui::DragValue::new(&mut n).range(2..=16))
                                .changed()
                            {
                                specs_changed = true;
                            }
                            spec.n = n;
                        }
                        if ui
                            .add(
                                egui::DragValue::new(&mut spec.strength)
                                    .range(0.0..=8.0)
                                    .speed(0.05)
                                    .prefix("s "),
                            )
                            .changed()
                        {
                            specs_changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        let spec = &mut self.specs[i];
                        ui.add_space(12.0);
                        let shape_name = match spec.shape {
                            SpecShape::Point => "point",
                            SpecShape::Disc { .. } => "disc",
                            SpecShape::Rect { .. } => "rect",
                        };
                        egui::ComboBox::from_id_salt(("shape", i))
                            .selected_text(shape_name)
                            .show_ui(ui, |ui| {
                                for (name, shape) in [
                                    ("point", SpecShape::Point),
                                    ("disc", SpecShape::Disc { radius: 0.04 }),
                                    (
                                        "rect",
                                        SpecShape::Rect {
                                            len_frac: 1.0,
                                            half_wid: 0.015,
                                        },
                                    ),
                                ] {
                                    let selected = shape_name == name;
                                    if ui.selectable_label(selected, name).clicked() && !selected {
                                        spec.shape = shape;
                                        specs_changed = true;
                                    }
                                }
                            });
                        match &mut spec.shape {
                            SpecShape::Point => {}
                            SpecShape::Disc { radius } => {
                                if ui
                                    .add(
                                        egui::DragValue::new(radius)
                                            .range(0.005..=0.3)
                                            .speed(0.002)
                                            .prefix("r "),
                                    )
                                    .changed()
                                {
                                    specs_changed = true;
                                }
                            }
                            SpecShape::Rect { len_frac, half_wid } => {
                                // Length is a fraction of the hand length
                                // (1 = full hand, >1 overhangs the hub).
                                for (v, prefix, max, speed) in [
                                    (len_frac, "l ", 2.0, 0.01),
                                    (half_wid, "w ", 0.3, 0.002),
                                ] {
                                    if ui
                                        .add(
                                            egui::DragValue::new(v)
                                                .range(0.0..=max)
                                                .speed(speed)
                                                .prefix(prefix),
                                        )
                                        .changed()
                                    {
                                        specs_changed = true;
                                    }
                                }
                            }
                        }
                    });
                }
                if specs_changed {
                    self.layouts = build_layouts(&self.specs);
                }
                ui.separator();
                ui.label("particles");
                {
                    let p = &mut self.sim.params;
                    ui.add(
                        egui::Slider::new(&mut p.mobility, crate::sim::bounds::MOBILITY.ui())
                            .logarithmic(true)
                            .text("mobility"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.max_speed, crate::sim::bounds::MAX_SPEED.ui())
                            .logarithmic(true)
                            .text("max speed"),
                    );
                    ui.add(egui::Slider::new(&mut p.noise, crate::sim::bounds::NOISE.ui()).text("noise"));
                    ui.add(
                        egui::Slider::new(&mut p.repulsion_strength, crate::sim::bounds::REPULSION_STRENGTH.ui())
                            .text("repulsion"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.repulsion_radius, crate::sim::bounds::REPULSION_RADIUS.ui())
                            .logarithmic(true)
                            .text("repulsion radius"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.chain_strength, crate::sim::bounds::CHAIN_STRENGTH.ui())
                            .text("chain strength"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.b_sat, crate::sim::bounds::B_SAT.ui())
                            .logarithmic(true)
                            .text("chain threshold |B|"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.chain_spacing, crate::sim::bounds::CHAIN_SPACING.ui())
                            .text("chain spacing"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.chain_range, crate::sim::bounds::CHAIN_RANGE.ui())
                            .text("chain range"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.chain_compress, crate::sim::bounds::CHAIN_COMPRESS.ui())
                            .text("chain compression"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.chain_cone, crate::sim::bounds::CHAIN_CONE.ui())
                            .text("chain cone (exp, 0 = off)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.chain_speed_cap, crate::sim::bounds::CHAIN_SPEED_CAP.ui())
                            .logarithmic(true)
                            .text("chain speed cap"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.chain_max_neighbors, 4..=192)
                            .logarithmic(true)
                            .text("chain neighbors"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.dt, crate::sim::bounds::DT.ui())
                            .logarithmic(true)
                            .text("dt (s)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.field_clamp, crate::sim::bounds::FIELD_CLAMP.ui())
                            .logarithmic(true)
                            .text("field clamp"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.fluid_scale, crate::sim::bounds::FLUID_SCALE.ui())
                            .logarithmic(true)
                            .text("fluid scale"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.drag_coupling, crate::sim::bounds::DRAG_COUPLING.ui())
                            .text("drag coupling"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.pointer_strength, crate::sim::bounds::POINTER_STRENGTH.ui())
                            .text("pointer strength"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.pointer_radius, crate::sim::bounds::POINTER_RADIUS.ui())
                            .text("pointer radius"),
                    );
                    ui.add(
                        egui::Slider::new(&mut p.pointer_visual, crate::sim::bounds::POINTER_VISUAL.ui())
                            .text("pointer visual"),
                    );
                }
                ui.add(
                    egui::Slider::new(&mut self.style.stroke_len, 0.0..=4.0)
                        .text("stroke length"),
                );
                ui.add(
                    egui::Slider::new(&mut self.style.max_px, 0..=2048)
                        .text("res cap px (0 = off)"),
                );
                ui.checkbox(&mut self.style.show_hands, "show hands");
                ui.horizontal(|ui| {
                    ui.label("background");
                    ui.color_edit_button_srgb(&mut self.style.bg);
                });
                ui.horizontal(|ui| {
                    ui.label("palette");
                    egui::ComboBox::from_id_salt("palette")
                        .selected_text(self.style.palette.name())
                        .show_ui(ui, |ui| {
                            for p in crate::render::Palette::ALL {
                                ui.selectable_value(&mut self.style.palette, p, p.name());
                            }
                        });
                });
                let mut count = self.sim.params.count;
                if ui
                    .add(
                        egui::Slider::new(&mut count, 500..=50000)
                            .logarithmic(true)
                            .text("count"),
                    )
                    .changed()
                {
                    self.sim.set_count(count);
                }
                if ui.button("reset particles").clicked() {
                    self.sim = Sim::new(self.sim.params);
                }
                ui.separator();
                ui.label("debug views");
                ui.checkbox(&mut self.views.field, "field |B|");
                ui.checkbox(&mut self.views.quiver, "force quiver");
                ui.checkbox(&mut self.views.dipoles, "dipoles");
                ui.checkbox(&mut self.views.velocity, "velocity color");
                ui.checkbox(&mut self.views.hash, "hash occupancy");
                ui.checkbox(&mut self.views.chains, "chain bonds");
                ui.separator();
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("dump frame").clicked() {
                    self.dump_frame();
                }
                if let Some(status) = &self.dump_status {
                    ui.label(status.clone());
                }
    }
}

impl eframe::App for ClockApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let pushed = self
            .pending
            .as_ref()
            .and_then(|pending| pending.borrow_mut().take());
        if let Some(cfg) = pushed {
            self.apply_config(cfg);
        }

        if self.show_panel {
            self.dev_panel(ctx);
        }

        let bg = self.style.bg;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(bg[0], bg[1], bg[2])))
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                let side_pts = avail.width().min(avail.height()).max(64.0);
                let ppp = ctx.pixels_per_point();
                let mut px = (side_pts * ppp).round().max(64.0) as u32;
                if self.style.max_px > 0 {
                    px = px.min(self.style.max_px).max(64);
                }

                // Pointer magnet: primary button/touch held over the dial.
                // Dial radius in points matches Map's 0.94 factor.
                let dial_r_pts = side_pts / 2.0 * 0.94;
                let center = avail.center();
                let to_world = |pos: egui::Pos2| {
                    Vec2::new(
                        ((pos.x - center.x) / dial_r_pts) as f64,
                        ((pos.y - center.y) / dial_r_pts) as f64,
                    )
                };
                // Hotspot around the 12 o'clock tick: tapping it toggles the
                // dev panel (the only way in for the panel-less web
                // component), and the pointer magnet is suppressed there so
                // the tap does not stir the particles.
                let in_hotspot =
                    |w: Vec2| (w - Vec2::new(0.0, -0.9)).len() < 0.15;

                if ctx.input(|i| i.pointer.primary_clicked()) {
                    if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                        if avail.contains(pos) && in_hotspot(to_world(pos)) {
                            self.show_panel = !self.show_panel;
                        }
                    }
                }

                self.pointer = ctx.input(|i| {
                    if !i.pointer.primary_down() {
                        return None;
                    }
                    let pos = i.pointer.interact_pos()?;
                    if !avail.contains(pos) {
                        return None;
                    }
                    let world = to_world(pos);
                    (world.len() <= 1.05 && !in_hotspot(world)).then_some((world, pos))
                });

                self.fb.resize(px, px);
                let now = self.clock.now();
                self.step_sim_to(now);
                let sources = self.sources_at(now);
                draw_clock(
                    &mut self.fb,
                    now,
                    &sources,
                    self.views,
                    self.style,
                    Some(&self.sim),
                );

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

                // Feedback ring around the pointer magnet.
                if let Some((_, screen)) = self.pointer {
                    if self.sim.params.pointer_strength > 0.0 {
                        ui.painter().circle_stroke(
                            screen,
                            (self.sim.params.pointer_radius * dial_r_pts as f64) as f32,
                            egui::Stroke::new(
                                1.5_f32,
                                egui::Color32::from_rgba_unmultiplied(128, 128, 128, 140),
                            ),
                        );
                    }
                }
            });

        // Idle egui repaints only on input; without this the clock freezes.
        ctx.request_repaint();
    }
}
