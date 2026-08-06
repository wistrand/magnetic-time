//! Interactive eframe app: renders the shared framebuffer to a texture each
//! frame, plus the dev side panel.

use eframe::egui;

use crate::clock::{format_time, ClockSource};
use crate::field::{Face, FaceConfigs, FaceKind, FieldSources, MagnetKind, SpecShape};
use crate::render::{draw_clock, DebugViews, FaceLayer, Framebuffer, Style};
use crate::sim::{Sim, SimParams};
use crate::vec2::Vec2;

/// Wall-clock budget for catch-up physics per frame. If stepping to "now"
/// would take longer (huge speed multiplier or a stall), the particles skip
/// the excess display time; the hands stay truthful to the clock.
const STEP_BUDGET: web_time::Duration = web_time::Duration::from_millis(12);

/// Camera obstacle-field resolution (grid cells per side).
const CAM_RES: usize = 128;

/// How camera luma feeds the sim.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// Thresholded into an obstacle wall layer (dark = wall).
    Wall,
    /// Signed intensity pushes particles along a fixed direction:
    /// v += dir * (luma - threshold) * force. Threshold is the neutral
    /// level; brighter pushes along the direction, darker pushes opposite.
    Flow,
}

/// Camera field configuration (`--camera`): luma from a V4L2 device becomes
/// a wall layer or a directional flow field. Native interactive only.
pub struct CameraConfig {
    /// V4L2 device path, e.g. /dev/video0.
    pub path: String,
    pub mode: CameraMode,
    /// Wall: open where luma/255 > threshold. Flow: the neutral level.
    /// `invert` flips (wall: bright = wall; flow: sign flip).
    pub threshold: f64,
    pub invert: bool,
    /// Flow: push direction, degrees clockwise from 3 o'clock (y down;
    /// 90 = toward 6 o'clock).
    pub dir_deg: f64,
    /// Flow: speed at full intensity, units/s.
    pub force: f64,
}

/// Live camera state: the reader thread fills `latest` (a latest-frame
/// mailbox); the app thread drains it, thresholds, and rebuilds the sim's
/// wall grid.
struct CameraState {
    cfg: CameraConfig,
    latest: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>,
    /// Last received luma frame (kept so threshold edits re-process
    /// without waiting for the next frame).
    frame: Option<Vec<u8>>,
    /// Rebuild the wall grid (new frame or changed threshold/invert).
    dirty: bool,
}

/// Pipe grayscale frames from the device via an ffmpeg subprocess
/// (center-cropped square, CAM_RES x CAM_RES, mirrored so it behaves like a
/// mirror). A subprocess keeps the project free of V4L/camera crates; the
/// thread exits when ffmpeg does (device unplugged, no ffmpeg installed).
#[cfg(not(target_arch = "wasm32"))]
fn spawn_camera_reader(
    path: String,
    latest: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>,
) {
    std::thread::spawn(move || {
        use std::io::Read;
        let vf = format!(
            "crop=min(iw\\,ih):min(iw\\,ih),scale={CAM_RES}:{CAM_RES},hflip,format=gray"
        );
        let child = std::process::Command::new("ffmpeg")
            .args([
                "-loglevel", "quiet", "-f", "v4l2", "-i", &path, "-vf", &vf, "-f",
                "rawvideo", "-",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return };
        let Some(mut out) = child.stdout.take() else { return };
        let mut buf = vec![0u8; CAM_RES * CAM_RES];
        while out.read_exact(&mut buf).is_ok() {
            if let Ok(mut slot) = latest.lock() {
                *slot = Some(buf.clone());
            }
        }
        let _ = child.kill();
    });
}

/// A complete externally-set configuration, applied live. Used by the web
/// component (attribute changes land here); native runs never push one.
#[derive(Clone, Copy)]
pub struct AppConfig {
    pub face: FaceConfigs,
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
    /// Every face's config plus the active selector; the panel edits this.
    face_cfg: FaceConfigs,
    /// The live face built from `face_cfg`, rebuilt on change.
    face: Face,
    views: DebugViews,
    style: Style,
    show_panel: bool,
    /// External config updates, drained each frame.
    pending: Option<PendingConfig>,
    /// Active pointer magnets: position in clock units plus screen position
    /// for the feedback ring, one per touch (or one for the mouse while the
    /// primary button is down over the dial).
    pointers: Vec<(Vec2, egui::Pos2)>,
    /// Raw touch points by id, tracked from egui Touch events. While any are
    /// active they drive `pointers` and the synthesized mouse pointer is
    /// ignored (egui mirrors the first touch into it).
    touches: std::collections::BTreeMap<u64, egui::Pos2>,
    sim: Sim,
    /// Display time the sim has been stepped to.
    sim_time: f64,
    fb: Framebuffer,
    /// Cached static face layer (bg, dial, rim, ticks); see render::FaceLayer.
    face_layer: FaceLayer,
    texture: Option<egui::TextureHandle>,
    dump_status: Option<String>,
    /// JSON preset file path edited in the dev panel, and the last save/load
    /// result message.
    preset_path: String,
    preset_status: Option<String>,
    /// Dish shape editor text (CLI grammar) and its last parse error.
    dish_text: String,
    dish_status: Option<String>,
    /// Camera obstacle field, when started with --camera. Never Some on
    /// wasm.
    camera: Option<CameraState>,
    /// Frame-pacing anchor for the fps cap (native; wasm uses repaint
    /// scheduling only).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    last_frame: web_time::Instant,
    /// Smoothed measured frame time for the FPS overlay (0 = no sample yet).
    fps_dt: f32,
    /// Persist config changes to preset::autosave_path() (throttled, on
    /// change) so the next start restores them. Native only.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    autosave: bool,
    /// Frames since the last autosave check (throttle counter).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    autosave_frames: u32,
    /// Last JSON written, to skip redundant disk writes.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    autosave_last: String,
}

impl ClockApp {
    pub fn new(
        clock: ClockSource,
        views: DebugViews,
        style: Style,
        params: SimParams,
        face_cfg: FaceConfigs,
        show_panel: bool,
        autosave: bool,
        camera: Option<CameraConfig>,
        pending: Option<PendingConfig>,
    ) -> Self {
        let camera = camera.map(|cfg| {
            let latest = std::sync::Arc::new(std::sync::Mutex::new(None));
            #[cfg(not(target_arch = "wasm32"))]
            spawn_camera_reader(cfg.path.clone(), latest.clone());
            CameraState {
                cfg,
                latest,
                frame: None,
                dirty: false,
            }
        });
        let speed = clock.multiplier();
        let sim_time = clock.now();
        Self {
            clock,
            speed,
            face_cfg,
            face: face_cfg.build(),
            views,
            style,
            show_panel,
            pending,
            pointers: Vec::new(),
            touches: std::collections::BTreeMap::new(),
            sim: Sim::new(params),
            sim_time,
            fb: Framebuffer::new(8, 8),
            face_layer: FaceLayer::default(),
            texture: None,
            dump_status: None,
            preset_path: "preset.json".to_string(),
            preset_status: None,
            dish_text: params.dish.label(),
            dish_status: None,
            camera,
            last_frame: web_time::Instant::now(),
            fps_dt: 0.0,
            autosave,
            autosave_frames: 0,
            autosave_last: String::new(),
        }
    }

    /// Apply a JSON preset string to the live configuration (dev panel load,
    /// or any caller). Rebuilds the face and resizes particles if the count
    /// changed.
    fn apply_preset(&mut self, json: &str) -> Result<(), String> {
        let old_count = self.sim.params.count;
        crate::preset::apply_json(
            json,
            &mut self.face_cfg,
            &mut self.sim.params,
            &mut self.style,
            &mut self.speed,
        )?;
        self.rebuild_face();
        if self.sim.params.count != old_count {
            self.sim.set_count(self.sim.params.count);
        }
        self.dish_text = self.sim.params.dish.label();
        Ok(())
    }

    /// Rebuild the live face after its inputs (specs, mode, seg config)
    /// change. Cheap: called on edit, not per frame.
    fn rebuild_face(&mut self) {
        self.face = self.face_cfg.build();
    }

    /// Apply an externally pushed configuration, preserving particle state
    /// (count changes go through Sim::set_count).
    fn apply_config(&mut self, cfg: AppConfig) {
        self.face_cfg = cfg.face;
        self.rebuild_face();
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
        self.dish_text = self.sim.params.dish.label();
    }

    /// Field sources for a display time, including a magnet per active
    /// pointer (touches, or the mouse while down).
    fn sources_at(&self, t: f64) -> FieldSources {
        let mut sources =
            FieldSources::at_time(&self.face, t, self.sim.params.field_clamp);
        let p = &self.sim.params;
        if p.pointer_strength > 0.0 {
            for &(world, _) in &self.pointers {
                sources.add_pointer(world, p.pointer_strength, p.pointer_radius, p.pointer_repel);
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

    /// Throttled write of the current config to the autosave file, skipped
    /// when nothing changed. ~2s at 60fps; frame-count based so no wall time.
    #[cfg(not(target_arch = "wasm32"))]
    fn autosave_tick(&mut self) {
        if !self.autosave {
            return;
        }
        self.autosave_frames += 1;
        if self.autosave_frames < 120 {
            return;
        }
        self.autosave_frames = 0;
        let json =
            crate::preset::to_json(&self.face_cfg, &self.sim.params, &self.style, self.speed);
        if json == self.autosave_last {
            return;
        }
        let path = crate::preset::autosave_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, &json).is_ok() {
            self.autosave_last = json;
        }
    }

    /// Drain the camera mailbox and rebuild the sim's wall grid from the
    /// newest frame (or after a threshold/invert edit). Cheap: threshold +
    /// two euclidean distance transforms at CAM_RES.
    fn camera_tick(&mut self) {
        let Some(cam) = &mut self.camera else { return };
        if let Ok(mut slot) = cam.latest.lock() {
            if let Some(f) = slot.take() {
                cam.frame = Some(f);
                cam.dirty = true;
            }
        }
        if !cam.dirty {
            return;
        }
        cam.dirty = false;
        if let Some(frame) = &cam.frame {
            match cam.cfg.mode {
                CameraMode::Wall => {
                    let thr = (cam.cfg.threshold * 255.0) as u8;
                    let open: Vec<bool> =
                        frame.iter().map(|&l| (l > thr) != cam.cfg.invert).collect();
                    self.sim.wall_grid =
                        Some(crate::dish::WallGrid::from_open_mask(&open, CAM_RES));
                    self.sim.flow = None;
                }
                CameraMode::Flow => {
                    let thr = cam.cfg.threshold as f32;
                    let sign = if cam.cfg.invert { -1.0f32 } else { 1.0 };
                    let vals: Vec<f32> = frame
                        .iter()
                        .map(|&l| (((l as f32 / 255.0) - thr) * 2.0).clamp(-1.0, 1.0) * sign)
                        .collect();
                    let a = cam.cfg.dir_deg.to_radians();
                    self.sim.flow = Some(crate::sim::FlowField {
                        grid: crate::dish::ScalarGrid::from_values(vals, CAM_RES),
                        dir: crate::vec2::Vec2f::new(a.cos() as f32, a.sin() as f32),
                        gain: cam.cfg.force as f32,
                    });
                    self.sim.wall_grid = None;
                }
            }
        }
    }

    fn dev_panel(&mut self, ctx: &egui::Context) {
        if self.style.panel_overlay {
            // Draggable by the title bar; position is remembered for the
            // session (egui memory), not across restarts. Default: right
            // edge, vertically centered.
            let screen = ctx.screen_rect();
            let max_h = screen.height() * 0.8;
            // Title-bar close button; same effect as the 12 o'clock tap.
            let mut open = true;
            egui::Window::new("dev")
                .pivot(egui::Align2::RIGHT_CENTER)
                .default_pos(egui::pos2(screen.right() - 20.0, screen.center().y))
                .default_width(180.0)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                        self.dev_panel_contents(ui);
                    });
                });
            if !open {
                self.show_panel = false;
            }
        } else {
            egui::SidePanel::right("dev")
                .resizable(false)
                .default_width(180.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.dev_panel_contents(ui);
                    });
                });
        }
    }

    /// Per-hand magnet layout controls (hands face). Returns whether any
    /// spec changed, so the caller can rebuild the face.
    fn magnet_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        for (i, name) in ["hour", "minute", "second"].iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(*name);
                let spec = &mut self.face_cfg.hands[i];
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
                                changed = true;
                            }
                        }
                    });
                if spec.kind != MagnetKind::Tip {
                    let mut n = spec.n.max(2);
                    if ui
                        .add(egui::DragValue::new(&mut n).range(2..=16))
                        .changed()
                    {
                        changed = true;
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
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                let spec = &mut self.face_cfg.hands[i];
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
                                changed = true;
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
                            changed = true;
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
                                changed = true;
                            }
                        }
                    }
                }
            });
        }
        changed
    }

    fn dev_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("dev");
            ui.checkbox(&mut self.style.panel_overlay, "overlay");
        });
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
        let mut face_changed = false;
        ui.horizontal(|ui| {
            ui.label("face");
            for (kind, label) in [
                (FaceKind::Hands, "hands"),
                (FaceKind::Seg, "seg"),
                (FaceKind::Tide, "tide"),
            ] {
                let sel = self.face_cfg.kind == kind;
                if ui.selectable_label(sel, label).clicked() && !sel {
                    self.face_cfg.kind = kind;
                    face_changed = true;
                }
            }
        });
        if self.face_cfg.kind == FaceKind::Seg {
            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.face_cfg.seg.with_seconds, "seconds").changed() {
                    face_changed = true;
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut self.face_cfg.seg.strength)
                            .range(0.0..=2.0)
                            .speed(0.01)
                            .prefix("s "),
                    )
                    .changed()
                {
                    face_changed = true;
                }
            });
        }
        if self.face_cfg.kind == FaceKind::Tide {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::DragValue::new(&mut self.face_cfg.tide.strength)
                            .range(0.0..=2.0)
                            .speed(0.01)
                            .prefix("s "),
                    )
                    .changed()
                {
                    face_changed = true;
                }
            });
        }
        let hands_mode = self.face_cfg.kind == FaceKind::Hands;
        let mut specs_changed = false;
        if hands_mode {
            ui.collapsing("magnets", |ui| {
                specs_changed = self.magnet_controls(ui);
            });
        }
        if specs_changed || face_changed {
            self.rebuild_face();
        }
        ui.separator();
        // Most-used controls near the top: particle count, reset, and the
        // common look. Deeper physics is grouped into collapsibles below.
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
        ui.horizontal(|ui| {
            ui.label("dish");
            ui.add(egui::TextEdit::singleline(&mut self.dish_text).desired_width(140.0));
            if ui.button("apply").clicked() {
                match crate::dish::Dish::parse(self.dish_text.trim()) {
                    Ok(d) => {
                        self.sim.params.dish = d;
                        self.dish_status = None;
                    }
                    Err(e) => self.dish_status = Some(e),
                }
            }
        });
        if let Some(status) = &self.dish_status {
            ui.label(status.clone());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.horizontal(|ui| {
                ui.label("preset");
                ui.add(egui::TextEdit::singleline(&mut self.preset_path).desired_width(96.0));
                if ui.button("save").clicked() {
                    let json = crate::preset::to_json(
                        &self.face_cfg,
                        &self.sim.params,
                        &self.style,
                        self.speed,
                    );
                    self.preset_status = Some(match std::fs::write(&self.preset_path, json) {
                        Ok(()) => format!("saved {}", self.preset_path),
                        Err(e) => format!("{}: {e}", self.preset_path),
                    });
                }
                if ui.button("load").clicked() {
                    self.preset_status = Some(match std::fs::read_to_string(&self.preset_path) {
                        Ok(text) => match self.apply_preset(&text) {
                            Ok(()) => format!("loaded {}", self.preset_path),
                            Err(e) => e,
                        },
                        Err(e) => format!("{}: {e}", self.preset_path),
                    });
                }
            });
            if let Some(status) = &self.preset_status {
                ui.label(status.clone());
            }
            if ui.checkbox(&mut self.autosave, "autosave").changed() {
                if self.autosave {
                    // Write promptly on enable; the tick handles the rest.
                    self.autosave_frames = 119;
                } else {
                    let _ = std::fs::remove_file(crate::preset::autosave_path());
                    self.autosave_last.clear();
                }
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.style.show_hands, "show hands/magnets");
            ui.checkbox(&mut self.style.show_face, "face");
            ui.checkbox(&mut self.style.show_fps, "fps");
            ui.checkbox(&mut self.style.fps_center, "centered");
        });
        ui.add(egui::Slider::new(&mut self.style.stroke_len, 0.0..=4.0).text("stroke length"));
        ui.add(
            egui::Slider::new(&mut self.style.heatmap_res, 0..=400)
                .text("heatmap res (0 = strokes)"),
        );
        // Palette: a start -> end color ramp (OKLab); background separate.
        ui.horizontal(|ui| {
            ui.label("colors");
            ui.color_edit_button_srgb(&mut self.style.palette.start);
            ui.label("->");
            ui.color_edit_button_srgb(&mut self.style.palette.end);
            ui.label("bg");
            ui.color_edit_button_srgb(&mut self.style.bg);
        });
        ui.horizontal(|ui| {
            let mut on = self.style.outside_bg.is_some();
            if ui.checkbox(&mut on, "outside bg").changed() {
                self.style.outside_bg = on.then_some(self.style.bg);
            }
            if let Some(c) = &mut self.style.outside_bg {
                ui.color_edit_button_srgb(c);
            }
        });
        ui.horizontal(|ui| {
            let mut on = self.style.face_color.is_some();
            if ui.checkbox(&mut on, "face color").changed() {
                self.style.face_color = on.then_some([128, 128, 128]);
            }
            if let Some(c) = &mut self.style.face_color {
                ui.color_edit_button_srgb(c);
            }
        });
        ui.horizontal(|ui| {
            ui.label("preset");
            for (name, p) in crate::render::Palette::PRESETS {
                if ui.small_button(name).clicked() {
                    self.style.palette = p;
                }
            }
        });

        ui.separator();
        ui.label("physics");
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
                egui::Slider::new(&mut p.disturb_every, crate::sim::bounds::DISTURB_EVERY.ui())
                    .text("disturb every s (0 = off)"),
            );
            ui.add(
                egui::Slider::new(&mut p.disturb_force, crate::sim::bounds::DISTURB_FORCE.ui())
                    .text("disturb force"),
            );
            ui.add(
                egui::Slider::new(&mut p.chain_strength, crate::sim::bounds::CHAIN_STRENGTH.ui())
                    .text("chain strength"),
            );
            ui.add(
                egui::Slider::new(
                    &mut p.repulsion_strength,
                    crate::sim::bounds::REPULSION_STRENGTH.ui(),
                )
                .text("repulsion"),
            );
            ui.add(
                egui::Slider::new(&mut p.fluid_scale, crate::sim::bounds::FLUID_SCALE.ui())
                    .logarithmic(true)
                    .text("fluid scale"),
            );
        }
        ui.collapsing("chain detail", |ui| {
            let p = &mut self.sim.params;
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
        });
        ui.collapsing("field & fluid", |ui| {
            let p = &mut self.sim.params;
            ui.add(
                egui::Slider::new(
                    &mut p.repulsion_radius,
                    crate::sim::bounds::REPULSION_RADIUS.ui(),
                )
                .logarithmic(true)
                .text("repulsion radius"),
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
                egui::Slider::new(&mut p.drag_coupling, crate::sim::bounds::DRAG_COUPLING.ui())
                    .text("drag coupling"),
            );
        });
        ui.collapsing("pointer / touch", |ui| {
            let p = &mut self.sim.params;
            ui.add(
                egui::Slider::new(
                    &mut p.pointer_strength,
                    crate::sim::bounds::POINTER_STRENGTH.ui(),
                )
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
            ui.checkbox(&mut p.pointer_repel, "pointer repels");
        });
        if let Some(cam) = &mut self.camera {
            ui.collapsing("camera", |ui| {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("mode");
                    changed |= ui
                        .selectable_value(&mut cam.cfg.mode, CameraMode::Wall, "wall")
                        .clicked();
                    changed |= ui
                        .selectable_value(&mut cam.cfg.mode, CameraMode::Flow, "flow")
                        .clicked();
                });
                let thr_label = match cam.cfg.mode {
                    CameraMode::Wall => "threshold",
                    CameraMode::Flow => "neutral level",
                };
                changed |= ui
                    .add(egui::Slider::new(&mut cam.cfg.threshold, 0.0..=1.0).text(thr_label))
                    .changed();
                changed |= ui.checkbox(&mut cam.cfg.invert, "invert").changed();
                if cam.cfg.mode == CameraMode::Flow {
                    changed |= ui
                        .add(egui::Slider::new(&mut cam.cfg.dir_deg, 0.0..=360.0).text("dir deg"))
                        .changed();
                    changed |= ui
                        .add(egui::Slider::new(&mut cam.cfg.force, 0.0..=0.3).text("force"))
                        .changed();
                }
                if changed {
                    cam.dirty = true;
                }
                ui.label(if cam.frame.is_some() {
                    "receiving frames"
                } else {
                    "waiting for frames"
                });
            });
        }
        ui.collapsing("render", |ui| {
            ui.add(
                egui::Slider::new(&mut self.style.max_px, 0..=2048)
                    .text("res cap px (0 = off)"),
            );
            ui.add(egui::Slider::new(&mut self.style.pad, 0.0..=0.45).text("dial padding"));
            ui.add(
                egui::Slider::new(&mut self.style.fps_cap, 0..=240).text("fps cap (0 = off)"),
            );
            ui.add(egui::Slider::new(&mut self.style.rotate, 0.0..=360.0).text("rotate deg"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.style.flip_x, "flip x");
                ui.checkbox(&mut self.style.flip_y, "flip y");
            });
        });

        ui.separator();
        ui.collapsing("debug views", |ui| {
            ui.checkbox(&mut self.views.field, "field |B|");
            ui.checkbox(&mut self.views.quiver, "force quiver");
            ui.checkbox(&mut self.views.dipoles, "dipoles");
            ui.checkbox(&mut self.views.velocity, "velocity color");
            ui.checkbox(&mut self.views.hash, "hash occupancy");
            ui.checkbox(&mut self.views.chains, "chain bonds");
            ui.checkbox(&mut self.views.camera, "camera walls");
        });
        ui.separator();
        #[cfg(not(target_arch = "wasm32"))]
        ui.horizontal(|ui| {
            if ui.button("dump frame").clicked() {
                self.dump_frame();
            }
            if ui.button("exit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        if let Some(status) = &self.dump_status {
            ui.label(status.clone());
        }
    }
}

impl eframe::App for ClockApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Frame pacing (fps cap): sleep off the remainder of the period.
        // Wall-clock, but pacing only; the fixed-dt sim is untouched (the
        // catch-up loop absorbs any cadence). Native only; wasm frames are
        // driven by requestAnimationFrame and the repaint delay below.
        #[cfg(not(target_arch = "wasm32"))]
        if self.style.fps_cap > 0 {
            let period = web_time::Duration::from_secs_f64(1.0 / self.style.fps_cap as f64);
            let elapsed = self.last_frame.elapsed();
            if elapsed < period {
                std::thread::sleep(period - elapsed);
            }
            self.last_frame = web_time::Instant::now();
        }

        let pushed = self
            .pending
            .as_ref()
            .and_then(|pending| pending.borrow_mut().take());
        if let Some(cfg) = pushed {
            self.apply_config(cfg);
        }

        // The escape hatch for --fullscreen/--kiosk runs.
        #[cfg(not(target_arch = "wasm32"))]
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        self.camera_tick();

        if self.show_panel {
            self.dev_panel(ctx);
        }

        if self.style.show_fps {
            // Measured frame time, self-smoothed; overlaid as a corner label
            // (the pixel buffer has no text). Floats above the clock, panel
            // or not. Not egui's stable_dt: under the fps cap repaints are
            // scheduled with request_repaint_after, and stable_dt then
            // reports egui's PREDICTED dt (the vsync guess), showing e.g. 60
            // while frames actually run at the cap.
            let dt = ctx.input(|i| i.unstable_dt).clamp(1e-6, 1.0);
            self.fps_dt = if self.fps_dt > 0.0 {
                self.fps_dt * 0.9 + dt * 0.1
            } else {
                dt
            };
            let fps = 1.0 / self.fps_dt;
            let (align, offset) = if self.style.fps_center {
                (egui::Align2::CENTER_TOP, egui::vec2(0.0, 6.0))
            } else {
                (egui::Align2::LEFT_TOP, egui::vec2(6.0, 6.0))
            };
            egui::Area::new(egui::Id::new("fps_overlay"))
                .anchor(align, offset)
                .interactable(false)
                .show(ctx, |ui| {
                    // Fixed width (right-padded to 3 digits) and no wrapping so
                    // the chip does not resize or re-wrap as the count changes.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{fps:>3.0} fps"))
                                .monospace()
                                .color(egui::Color32::from_rgb(180, 200, 255))
                                .background_color(egui::Color32::from_black_alpha(130)),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        }

        let bg = self.style.outside_bg.unwrap_or(self.style.bg);
        let fill = if self.style.transparent_bg {
            egui::Color32::TRANSPARENT
        } else {
            egui::Color32::from_rgb(bg[0], bg[1], bg[2])
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(fill))
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                // Per-side margin as a fraction of the short side; the dial
                // square shrinks, the frame fill covers the rest.
                let avail = avail.shrink(self.style.pad.clamp(0.0, 0.45) as f32
                    * avail.width().min(avail.height()));
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
                // Inverse of the view rotation: pointer positions must reach
                // the sim in unrotated world space (the hotspot check then
                // follows the rotated 12 o'clock tick for free).
                let rot = self.style.rotate.to_radians();
                let (rc, rs) = (rot.cos(), rot.sin());
                let fx = if self.style.flip_x { -1.0 } else { 1.0 };
                let fy = if self.style.flip_y { -1.0 } else { 1.0 };
                let to_world = move |pos: egui::Pos2| {
                    let dx = ((pos.x - center.x) / dial_r_pts) as f64;
                    let dy = ((pos.y - center.y) / dial_r_pts) as f64;
                    Vec2::new((dx * rc + dy * rs) * fx, (-dx * rs + dy * rc) * fy)
                };
                // Hotspot around the 12 o'clock tick: tapping it toggles the
                // dev panel (the only way in for the panel-less web
                // component), and the pointer magnet is suppressed there so
                // the tap does not stir the particles.
                let in_hotspot =
                    |w: Vec2| (w - Vec2::new(0.0, -0.9)).len() < 0.15;
                // Pointer events reach the sim only when egui does not own
                // them: not while a widget interaction is in progress (a
                // slider or color-picker drag keeps ownership even when the
                // cursor strays over the dial), and not when the cursor is
                // over a floating egui layer (the overlay panel, picker
                // popups). Docked panels live in the background layer and
                // are already excluded by `avail`.
                let widget_active = ctx.is_using_pointer();
                let egui_owns = |pos: egui::Pos2| {
                    ctx.layer_id_at(pos)
                        .is_some_and(|l| l.order != egui::Order::Background)
                };

                let (clicked, double_clicked, primary_down, ipos) = ctx.input(|i| {
                    // Track raw touch points; egui also mirrors the first
                    // touch into the synthesized pointer, so while any touch
                    // is active the touch map is the sole magnet source.
                    for e in &i.events {
                        if let egui::Event::Touch { id, phase, pos, .. } = e {
                            match phase {
                                egui::TouchPhase::Start | egui::TouchPhase::Move => {
                                    self.touches.insert(id.0, *pos);
                                }
                                egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                                    self.touches.remove(&id.0);
                                }
                            }
                        }
                    }
                    (
                        i.pointer.primary_clicked(),
                        i.pointer.button_double_clicked(egui::PointerButton::Primary),
                        i.pointer.primary_down(),
                        i.pointer.interact_pos(),
                    )
                });

                if clicked && !widget_active {
                    if let Some(pos) = ipos {
                        if avail.contains(pos) && !egui_owns(pos) && in_hotspot(to_world(pos)) {
                            self.show_panel = !self.show_panel;
                        }
                    }
                }

                // Double-click/tap on the dial: fire a disturbance burst,
                // same envelope as the periodic one.
                if double_clicked && !widget_active {
                    if let Some(pos) = ipos {
                        if avail.contains(pos) && !egui_owns(pos) {
                            let world = to_world(pos);
                            if self.sim.params.dish.sdf(world) <= 0.05 && !in_hotspot(world) {
                                self.sim.disturb();
                            }
                        }
                    }
                }

                // A magnet per pointer: every active touch, or the mouse
                // while its button is down and no touches are active.
                self.pointers.clear();
                if !widget_active {
                    // Same 0.05 margin the circle always had (sdf = len - 1).
                    let pdish = self.sim.params.dish;
                    let mut accept = |pos: egui::Pos2| {
                        if !avail.contains(pos) || egui_owns(pos) {
                            return;
                        }
                        let world = to_world(pos);
                        if pdish.sdf(world) <= 0.05 && !in_hotspot(world) {
                            self.pointers.push((world, pos));
                        }
                    };
                    if self.touches.is_empty() {
                        if primary_down {
                            if let Some(pos) = ipos {
                                accept(pos);
                            }
                        }
                    } else {
                        for &pos in self.touches.values() {
                            accept(pos);
                        }
                    }
                }

                self.fb.resize(px, px);
                let now = self.clock.now();
                self.step_sim_to(now);
                let sources = self.sources_at(now);
                draw_clock(
                    &mut self.fb,
                    now,
                    &self.face,
                    &sources,
                    self.views,
                    self.style,
                    Some(&self.sim),
                    Some(&mut self.face_layer),
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

                // Feedback ring around each pointer magnet.
                if self.sim.params.pointer_strength > 0.0 {
                    for &(_, screen) in &self.pointers {
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

        #[cfg(not(target_arch = "wasm32"))]
        self.autosave_tick();

        // Idle egui repaints only on input; without this the clock freezes.
        // With a cap, schedule the next repaint a period out instead of
        // immediately (input can still wake earlier; the sleep above holds
        // the cap on native regardless).
        if self.style.fps_cap > 0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                1.0 / self.style.fps_cap as f64,
            ));
        } else {
            ctx.request_repaint();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.style.transparent_bg {
            [0.0; 4]
        } else {
            // eframe's default; only visible where nothing else paints.
            egui::Color32::from_rgba_unmultiplied(12, 12, 12, 180).to_normalized_gamma_f32()
        }
    }
}
