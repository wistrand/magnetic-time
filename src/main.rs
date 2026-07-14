mod app;
mod clock;
mod render;

use std::path::PathBuf;
use std::process::ExitCode;

use clock::ClockSource;

struct Options {
    headless: bool,
    /// Start time, seconds since midnight. None = wall clock.
    time: Option<f64>,
    /// Display seconds to advance before rendering (headless).
    sim_seconds: f64,
    dump: Option<PathBuf>,
    /// Framebuffer side in pixels (headless).
    size: u32,
    /// Initial time-speed multiplier (interactive).
    speed: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            headless: false,
            time: None,
            sim_seconds: 0.0,
            dump: None,
            size: 800,
            speed: 1.0,
        }
    }
}

const USAGE: &str = "usage: magnetic-time [--headless --dump PATH] [--time HH:MM:SS]
                     [--sim-seconds N] [--size PX] [--speed N]";

fn parse_args() -> Result<Options, String> {
    let mut opts = Options::default();
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
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument '{other}'\n{USAGE}")),
        }
    }
    if opts.headless && opts.dump.is_none() {
        return Err("--headless requires --dump PATH".to_string());
    }
    Ok(opts)
}

fn run_headless(opts: &Options) -> Result<(), String> {
    let start = opts.time.unwrap_or_else(|| ClockSource::wall(1.0).now());
    // Phase 1: no simulation yet, advancing time is just addition. Once the
    // particle sim exists this becomes a fixed-dt stepping loop.
    let t = start + opts.sim_seconds;
    let mut fb = render::Framebuffer::new(opts.size, opts.size);
    render::draw_clock(&mut fb, t);
    let path = opts.dump.as_ref().unwrap();
    render::write_png(path, &fb)?;
    println!("wrote {} ({}x{}, time {})", path.display(), fb.width, fb.height, clock::format_time(t));
    Ok(())
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };

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
        Box::new(|_cc| Ok(Box::new(app::ClockApp::new(clock)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("eframe: {e}");
            ExitCode::FAILURE
        }
    }
}
