// CLI parsing, headless mode, and grad-check are native-only; their helpers
// are intentionally unused in the browser build.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

mod app;
mod clock;
mod field;
mod hands;
mod preset;
mod render;
mod sim;
mod vec2;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;

#[cfg(not(target_arch = "wasm32"))]
use clock::ClockSource;
#[cfg(not(target_arch = "wasm32"))]
use render::DebugViews;

#[cfg(not(target_arch = "wasm32"))]
struct Options {
    headless: bool,
    /// Start time, seconds since midnight. None = wall clock.
    time: Option<f64>,
    /// Display seconds to advance before rendering (headless).
    sim_seconds: f64,
    dump: Option<PathBuf>,
    /// Headless: also write particle positions and local field samples as
    /// CSV (x,y,dir_x,dir_y,w in dial units), for measurement scripts that
    /// must not go through the renderer (stroke/dot merging biases image-
    /// based estimators).
    dump_positions: Option<PathBuf>,
    /// Framebuffer side in pixels (headless).
    size: u32,
    /// Initial time-speed multiplier (interactive).
    speed: f64,
    views: DebugViews,
    style: render::Style,
    sim: sim::SimParams,
    /// The active face and every face's configuration (hand layout, seg
    /// readout). One struct so faces stay self-contained; see field.rs.
    face: field::FaceConfigs,
    /// Write the resolved configuration as a JSON preset and exit.
    save_preset: Option<PathBuf>,
    /// Start with the dev panel shown (interactive). Toggle at runtime with
    /// the 12 o'clock tick either way.
    show_panel: bool,
    /// Verify the analytic gradient against central differences and exit.
    grad_check: bool,
    /// Headless annealing: run the first `anneal_for` sim-seconds with
    /// chain_strength = `anneal_from`, then switch to --chain-strength for
    /// the remainder. For hysteresis experiments.
    anneal_from: f64,
    anneal_for: f64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for Options {
    fn default() -> Self {
        Self {
            headless: false,
            time: None,
            sim_seconds: 0.0,
            dump: None,
            dump_positions: None,
            size: 800,
            speed: 1.0,
            views: DebugViews::default(),
            style: render::Style::default(),
            sim: sim::SimParams::default(),
            face: field::FaceConfigs::default(),
            save_preset: None,
            show_panel: true,
            grad_check: false,
            anneal_from: 0.0,
            anneal_for: 0.0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
const USAGE: &str = "usage: magnetic-time [--headless --dump PATH] [--time HH:MM:SS]
                     [--sim-seconds N] [--size PX] [--speed N]
                     [--dump-positions PATH]  headless: also write particle
                     positions + local field as CSV (x,y,dir_x,dir_y,w)
                     [--view field,quiver,dipoles,velocity,hash]
                     [--particles N] [--seed N] [--stroke-len F]
                     [--palette ice|ember|emerald|violet|mono] [--bg RRGGBB]
                     [--max-px N]  cap interactive render resolution (0 = off)
                     [--hide-hands | --show-hands]  (default: hidden)
                     [--no-dev-panel]  start with the dev panel hidden
                     (interactive; tap the 12 o'clock tick to toggle)
                     [--fps]  show a frame-rate overlay (interactive)
                     [--mobility F] [--max-speed F] [--noise F] [--repulsion F]
                     [--repulsion-radius F] [--chain-speed-cap F]
                     [--chain-neighbors N] [--dt F] [--field-clamp F] [--fluid-scale F]
                     [--chain-strength F] [--chain-spacing F] [--chain-range F]
                     [--chain-compress F] [--drag F]
                     [--chain-cone DEG]  experimental: restrict chain attraction
                     to +-DEG of the moment axis (0 = physical 54.7 cone)
                     [--pointer-strength F] [--pointer-radius F]  touch/mouse magnet
                     [--pointer-visual F]  pointer weight in stroke color/orientation
                     [--anneal-from F --anneal-for SECONDS]  headless: run the
                     first SECONDS at chain-strength F, then switch
                     [--grad-check]  verify analytic field gradient, then exit
                     [--preset PATH]  load a JSON preset (before other flags to
                     use as a base; later flags override)
                     [--save-preset PATH]  write the resolved config as a JSON
                     preset and exit
                     [--face hands|seg|seg-hms|tide]  hands (default), a digital
                     seven-segment readout (seg = HH:MM, seg-hms = HH:MM:SS),
                     or the tide arcs (concentric filling gauges)
                     [--seg-strength F]  per-segment bar magnet strength
                     [--tide-strength F]  per-arc bar magnet strength
                     [--magnets HOUR,MINUTE,SECOND]  each tip | strip:N | alt:N;
                     one value applies to all hands (hands face only)
                     [--strengths HOUR,MINUTE,SECOND]  per-magnet moment scale;
                     one value applies to all hands
                     [--shapes HOUR,MINUTE,SECOND]  each point | disc:R | rect:FxW,
                     F = length as fraction of hand length (0..2, 1 = full hand);
                     one value applies to all hands";

#[cfg(not(target_arch = "wasm32"))]
fn parse_args() -> Result<Options, String> {
    let mut opts = Options::default();
    // Applied after the loop so --strengths/--shapes work in any flag order.
    let mut strengths: Option<[f64; 3]> = None;
    let mut shapes: Option<[field::SpecShape; 3]> = None;
    let mut args = std::env::args().skip(1);
    let value = |name: &str, args: &mut dyn Iterator<Item = String>| {
        args.next().ok_or(format!("{name} needs a value"))
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => opts.headless = true,
            "--time" => opts.time = Some(clock::parse_time(&value("--time", &mut args)?)?),
            "--sim-seconds" => {
                opts.sim_seconds = value("--sim-seconds", &mut args)?
                    .parse()
                    .map_err(|e| format!("--sim-seconds: {e}"))?
            }
            "--dump" => opts.dump = Some(PathBuf::from(value("--dump", &mut args)?)),
            "--dump-positions" => {
                opts.dump_positions = Some(PathBuf::from(value("--dump-positions", &mut args)?))
            }
            "--size" => {
                opts.size = value("--size", &mut args)?
                    .parse()
                    .map_err(|e| format!("--size: {e}"))?
            }
            "--speed" => {
                opts.speed = value("--speed", &mut args)?
                    .parse()
                    .map_err(|e| format!("--speed: {e}"))?
            }
            "--view" => opts.views = DebugViews::parse(&value("--view", &mut args)?)?,
            "--particles" => {
                opts.sim.count = value("--particles", &mut args)?
                    .parse()
                    .map_err(|e| format!("--particles: {e}"))?
            }
            "--seed" => {
                opts.sim.seed = value("--seed", &mut args)?
                    .parse()
                    .map_err(|e| format!("--seed: {e}"))?
            }
            "--mobility" => {
                opts.sim.mobility = value("--mobility", &mut args)?
                    .parse()
                    .map_err(|e| format!("--mobility: {e}"))?
            }
            "--max-speed" => {
                opts.sim.max_speed = value("--max-speed", &mut args)?
                    .parse()
                    .map_err(|e| format!("--max-speed: {e}"))?
            }
            "--noise" => {
                opts.sim.noise = value("--noise", &mut args)?
                    .parse()
                    .map_err(|e| format!("--noise: {e}"))?
            }
            "--repulsion" => {
                opts.sim.repulsion_strength = value("--repulsion", &mut args)?
                    .parse()
                    .map_err(|e| format!("--repulsion: {e}"))?
            }
            "--chain-strength" => {
                opts.sim.chain_strength = value("--chain-strength", &mut args)?
                    .parse()
                    .map_err(|e| format!("--chain-strength: {e}"))?
            }
            "--chain-spacing" => {
                opts.sim.chain_spacing = value("--chain-spacing", &mut args)?
                    .parse()
                    .map_err(|e| format!("--chain-spacing: {e}"))?
            }
            "--chain-range" => {
                opts.sim.chain_range = value("--chain-range", &mut args)?
                    .parse()
                    .map_err(|e| format!("--chain-range: {e}"))?
            }
            "--chain-compress" => {
                opts.sim.chain_compress = value("--chain-compress", &mut args)?
                    .parse()
                    .map_err(|e| format!("--chain-compress: {e}"))?
            }
            "--chain-cone" => {
                opts.sim.chain_cone = value("--chain-cone", &mut args)?
                    .parse()
                    .map_err(|e| format!("--chain-cone: {e}"))?
            }
            "--chain-speed-cap" => {
                opts.sim.chain_speed_cap = value("--chain-speed-cap", &mut args)?
                    .parse()
                    .map_err(|e| format!("--chain-speed-cap: {e}"))?
            }
            "--chain-neighbors" => {
                opts.sim.chain_max_neighbors = value("--chain-neighbors", &mut args)?
                    .parse()
                    .map_err(|e| format!("--chain-neighbors: {e}"))?
            }
            "--repulsion-radius" => {
                opts.sim.repulsion_radius = value("--repulsion-radius", &mut args)?
                    .parse()
                    .map_err(|e| format!("--repulsion-radius: {e}"))?
            }
            "--fluid-scale" => {
                opts.sim.fluid_scale = value("--fluid-scale", &mut args)?
                    .parse()
                    .map_err(|e| format!("--fluid-scale: {e}"))?
            }
            "--field-clamp" => {
                opts.sim.field_clamp = value("--field-clamp", &mut args)?
                    .parse()
                    .map_err(|e| format!("--field-clamp: {e}"))?
            }
            "--dt" => {
                opts.sim.dt = value("--dt", &mut args)?
                    .parse()
                    .map_err(|e| format!("--dt: {e}"))?
            }
            "--drag" => {
                opts.sim.drag_coupling = value("--drag", &mut args)?
                    .parse()
                    .map_err(|e| format!("--drag: {e}"))?
            }
            "--pointer-strength" => {
                opts.sim.pointer_strength = value("--pointer-strength", &mut args)?
                    .parse()
                    .map_err(|e| format!("--pointer-strength: {e}"))?
            }
            "--pointer-radius" => {
                opts.sim.pointer_radius = value("--pointer-radius", &mut args)?
                    .parse()
                    .map_err(|e| format!("--pointer-radius: {e}"))?
            }
            "--pointer-visual" => {
                opts.sim.pointer_visual = value("--pointer-visual", &mut args)?
                    .parse()
                    .map_err(|e| format!("--pointer-visual: {e}"))?
            }
            "--preset" => {
                let path = value("--preset", &mut args)?;
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("--preset {path}: {e}"))?;
                preset::apply_json(
                    &text,
                    &mut opts.face,
                    &mut opts.sim,
                    &mut opts.style,
                    &mut opts.speed,
                )?;
            }
            "--save-preset" => {
                opts.save_preset = Some(PathBuf::from(value("--save-preset", &mut args)?))
            }
            "--magnets" => {
                opts.face.hands = field::parse_magnets(&value("--magnets", &mut args)?)?
            }
            "--face" => {
                let v = value("--face", &mut args)?;
                match v.as_str() {
                    "hands" => opts.face.kind = field::FaceKind::Hands,
                    "seg" => {
                        opts.face.kind = field::FaceKind::Seg;
                        opts.face.seg.with_seconds = false;
                    }
                    "seg-hms" => {
                        opts.face.kind = field::FaceKind::Seg;
                        opts.face.seg.with_seconds = true;
                    }
                    "tide" => opts.face.kind = field::FaceKind::Tide,
                    other => {
                        return Err(format!(
                            "--face: unknown '{other}' (hands, seg, seg-hms, tide)"
                        ))
                    }
                }
            }
            "--seg-strength" => {
                opts.face.seg.strength = value("--seg-strength", &mut args)?
                    .parse()
                    .map_err(|e| format!("--seg-strength: {e}"))?
            }
            "--tide-strength" => {
                opts.face.tide.strength = value("--tide-strength", &mut args)?
                    .parse()
                    .map_err(|e| format!("--tide-strength: {e}"))?
            }
            "--strengths" => {
                strengths = Some(field::parse_strengths(&value("--strengths", &mut args)?)?)
            }
            "--shapes" => shapes = Some(field::parse_shapes(&value("--shapes", &mut args)?)?),
            "--stroke-len" => {
                opts.style.stroke_len = value("--stroke-len", &mut args)?
                    .parse()
                    .map_err(|e| format!("--stroke-len: {e}"))?
            }
            "--palette" => {
                opts.style.palette = render::Palette::parse(&value("--palette", &mut args)?)?
            }
            "--bg" => opts.style.bg = render::parse_color(&value("--bg", &mut args)?)?,
            "--max-px" => {
                opts.style.max_px = value("--max-px", &mut args)?
                    .parse()
                    .map_err(|e| format!("--max-px: {e}"))?
            }
            "--hide-hands" => opts.style.show_hands = false,
            "--show-hands" => opts.style.show_hands = true,
            "--no-dev-panel" => opts.show_panel = false,
            "--fps" => opts.style.show_fps = true,
            "--grad-check" => opts.grad_check = true,
            "--anneal-from" => {
                opts.anneal_from = value("--anneal-from", &mut args)?
                    .parse()
                    .map_err(|e| format!("--anneal-from: {e}"))?
            }
            "--anneal-for" => {
                opts.anneal_for = value("--anneal-for", &mut args)?
                    .parse()
                    .map_err(|e| format!("--anneal-for: {e}"))?
            }
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument '{other}'\n{USAGE}")),
        }
    }
    if let Some(s) = strengths {
        for (spec, strength) in opts.face.hands.iter_mut().zip(s) {
            spec.strength = strength;
        }
    }
    if let Some(s) = shapes {
        for (spec, shape) in opts.face.hands.iter_mut().zip(s) {
            spec.shape = shape;
        }
    }
    if opts.headless && opts.dump.is_none() {
        return Err("--headless requires --dump PATH".to_string());
    }
    // Range-check the sim parameters (the CLI is the one input path with no
    // per-control clamp; see SimParams::validate). Also the few Options-level
    // numbers that feed the sim.
    opts.sim.validate()?;
    if !opts.sim_seconds.is_finite() || opts.sim_seconds < 0.0 {
        return Err(format!("--sim-seconds must be finite and >= 0, got {}", opts.sim_seconds));
    }
    if opts.size == 0 {
        return Err("--size must be >= 1".to_string());
    }
    if !opts.speed.is_finite() {
        return Err(format!("--speed must be finite, got {}", opts.speed));
    }
    if opts.anneal_for > 0.0 && (!opts.anneal_from.is_finite() || opts.anneal_from < 0.0) {
        return Err(format!(
            "--anneal-from must be finite and >= 0, got {}",
            opts.anneal_from
        ));
    }
    Ok(opts)
}

#[cfg(not(target_arch = "wasm32"))]
/// Compare the analytic grad(|B|^2) against a central-difference reference
/// at random dish points. Large outliers right at r_min clamp boundaries are
/// expected (the numeric stencil straddles the kink; the analytic value is
/// the correct one-sided derivative there).
fn run_grad_check(opts: &Options) {
    let face = opts.face.build();
    let t = opts.time.unwrap_or(10.0 * 3600.0 + 8.0 * 60.0 + 30.0);
    let sources = field::FieldSources::at_time(&face, t, opts.sim.field_clamp);
    let mut rng = sim::Rng::new(42);
    let (mut max_rel, mut sum, mut bad) = (0.0f64, 0.0f64, 0u32);
    const N: u32 = 20000;
    for _ in 0..N {
        let a = rng.f64() * std::f64::consts::TAU;
        let r = rng.f64().sqrt() * 0.92;
        let p = vec2::Vec2::new(a.cos() * r, a.sin() * r);
        let ga = sources.b_and_grad_b2(p).1;
        let gn = sources.grad_b2_numeric(p);
        let denom = ga.len().max(gn.len()).max(1e-9);
        let rel = (ga - gn).len() / denom;
        sum += rel;
        if rel > max_rel {
            max_rel = rel;
        }
        if rel > 1e-2 {
            bad += 1;
        }
    }
    println!(
        "grad-check: {N} points, mean rel err {:.2e}, max {:.2e}, >1% at {bad} points",
        sum / N as f64,
        max_rel
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn run_headless(opts: &Options) -> Result<(), String> {
    let start = opts.time.unwrap_or_else(|| ClockSource::wall(1.0).now());
    let face = opts.face.build();
    let mut particle_sim = sim::Sim::new(opts.sim);
    let t = if opts.anneal_for > 0.0 {
        // Two-phase run for hysteresis experiments: anneal at one chain
        // strength, then switch to the requested one.
        let pre = opts.anneal_for.min(opts.sim_seconds);
        particle_sim.params.chain_strength = opts.anneal_from;
        let mid = particle_sim.advance(&face, start, pre);
        particle_sim.params.chain_strength = opts.sim.chain_strength;
        particle_sim.advance(&face, mid, opts.sim_seconds - pre)
    } else {
        particle_sim.advance(&face, start, opts.sim_seconds)
    };
    let sources = field::FieldSources::at_time(&face, t, opts.sim.field_clamp);
    let mut fb = render::Framebuffer::new(opts.size, opts.size);
    render::draw_clock(
        &mut fb,
        t,
        &face,
        &sources,
        opts.views,
        opts.style,
        Some(&particle_sim),
    );
    let path = opts.dump.as_ref().unwrap();
    render::write_png(path, &fb)?;
    println!("wrote {} ({}x{}, time {})", path.display(), fb.width, fb.height, clock::format_time(t));
    if let Some(ppath) = &opts.dump_positions {
        let mut out = String::with_capacity(particle_sim.pos.len() * 48);
        out.push_str("x,y,dir_x,dir_y,w\n");
        for (p, f) in particle_sim.pos.iter().zip(&particle_sim.field) {
            out.push_str(&format!(
                "{:.6},{:.6},{:.4},{:.4},{:.4}\n",
                p.x, p.y, f.dir.x, f.dir.y, f.w
            ));
        }
        std::fs::write(ppath, out).map_err(|e| format!("{}: {e}", ppath.display()))?;
        println!("wrote {} ({} particles)", ppath.display(), particle_sim.pos.len());
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };

    if opts.grad_check {
        run_grad_check(&opts);
        return ExitCode::SUCCESS;
    }

    if let Some(path) = &opts.save_preset {
        let json = preset::to_json(&opts.face, &opts.sim, &opts.style, opts.speed);
        return match std::fs::write(path, json) {
            Ok(()) => {
                println!("wrote preset {}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                ExitCode::FAILURE
            }
        };
    }

    if opts.headless {
        return match run_headless(&opts) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("{msg}");
                ExitCode::FAILURE
            }
        };
    }

    let clock = match opts.time {
        Some(t) => ClockSource::at(t, opts.speed),
        None => ClockSource::wall(opts.speed),
    };
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 820.0])
            .with_title("magnetic-time"),
        ..Default::default()
    };
    match eframe::run_native(
        "magnetic-time",
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(app::ClockApp::new(
                clock,
                opts.views,
                opts.style,
                opts.sim,
                opts.face,
                opts.show_panel,
                None,
            )))
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("eframe: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Browser builds are driven entirely through `web::WebHandle` (see
/// docs/app/magnetic-clock.js); nothing happens at module load.
#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
}
