//! Browser API for the `<magnetic-clock>` web component. The JS wrapper
//! (docs/app/magnetic-clock.js) creates a `WebHandle`, feeds element
//! attributes through the setters, and starts the app on its canvas. Setter
//! values reuse the CLI grammar (see field.rs parsers), so attributes and
//! flags speak the same language.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use crate::app::{AppConfig, ClockApp, PendingConfig};
use crate::clock::ClockSource;
use crate::field;
use crate::render::{DebugViews, Style};
use crate::sim::SimParams;

/// Component defaults: the rings preset, panel hidden, count reduced for the
/// single-threaded browser sim.
fn web_defaults() -> AppConfig {
    AppConfig {
        specs: field::default_specs(),
        style: Style::default(),
        speed: 1.0,
        sim: SimParams {
            count: 15000,
            ..Default::default()
        },
        show_panel: false,
    }
}

#[wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
    /// The desired configuration; setters patch this.
    config: Rc<RefCell<AppConfig>>,
    /// Single-slot channel the running app drains each frame.
    pending: PendingConfig,
}

fn js_err(e: String) -> JsValue {
    JsValue::from_str(&e)
}

#[wasm_bindgen]
impl WebHandle {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self {
            runner: eframe::WebRunner::new(),
            config: Rc::new(RefCell::new(web_defaults())),
            pending: Rc::new(RefCell::new(None)),
        }
    }

    /// Start rendering on the given canvas. Attributes set before this call
    /// are part of the initial configuration.
    pub async fn start(&self, canvas: web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
        let cfg = *self.config.borrow();
        let pending = self.pending.clone();
        self.runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |_cc| {
                    Ok(Box::new(ClockApp::new(
                        ClockSource::wall(cfg.speed),
                        DebugViews::default(),
                        cfg.style,
                        cfg.sim,
                        cfg.specs,
                        cfg.show_panel,
                        Some(pending),
                    )))
                }),
            )
            .await
    }

    /// Stop the app and release its WebGL context.
    pub fn destroy(&self) {
        self.runner.destroy();
    }

    fn push(&self) {
        *self.pending.borrow_mut() = Some(*self.config.borrow());
    }

    /// Layout kinds per hand ("tip", "strip:N", "alt:N"). Resets strengths
    /// and shapes; the component re-applies those attributes afterwards.
    pub fn set_magnets(&self, v: &str) -> Result<(), JsValue> {
        self.config.borrow_mut().specs = field::parse_magnets(v).map_err(js_err)?;
        self.push();
        Ok(())
    }

    /// Per-hand strengths ("0.6" or "0.1,0.05,0.6").
    pub fn set_strengths(&self, v: &str) -> Result<(), JsValue> {
        let s = field::parse_strengths(v).map_err(js_err)?;
        for (spec, strength) in self.config.borrow_mut().specs.iter_mut().zip(s) {
            spec.strength = strength;
        }
        self.push();
        Ok(())
    }

    /// Per-hand shapes ("point", "disc:R", "rect:FxW").
    pub fn set_shapes(&self, v: &str) -> Result<(), JsValue> {
        let s = field::parse_shapes(v).map_err(js_err)?;
        for (spec, shape) in self.config.borrow_mut().specs.iter_mut().zip(s) {
            spec.shape = shape;
        }
        self.push();
        Ok(())
    }

    pub fn set_particles(&self, n: u32) {
        self.config.borrow_mut().sim.count = (n as usize).clamp(100, 100_000);
        self.push();
    }

    pub fn set_speed(&self, v: f64) {
        self.config.borrow_mut().speed = v.clamp(0.0, 100_000.0);
        self.push();
    }

    /// Particle color scale ("ice", "ember", "emerald", "violet", "mono").
    pub fn set_palette(&self, v: &str) -> Result<(), JsValue> {
        self.config.borrow_mut().style.palette =
            crate::render::Palette::parse(v).map_err(js_err)?;
        self.push();
        Ok(())
    }

    pub fn set_stroke_len(&self, v: f64) {
        self.config.borrow_mut().style.stroke_len = v.clamp(0.0, 8.0);
        self.push();
    }

    pub fn set_show_hands(&self, on: bool) {
        self.config.borrow_mut().style.show_hands = on;
        self.push();
    }

    pub fn set_dev_panel(&self, on: bool) {
        self.config.borrow_mut().show_panel = on;
        self.push();
    }

    pub fn set_mobility(&self, v: f64) {
        self.config.borrow_mut().sim.mobility = v.max(0.0);
        self.push();
    }

    pub fn set_max_speed(&self, v: f64) {
        self.config.borrow_mut().sim.max_speed = v.max(0.0);
        self.push();
    }

    pub fn set_noise(&self, v: f64) {
        self.config.borrow_mut().sim.noise = v.max(0.0);
        self.push();
    }

    pub fn set_repulsion(&self, v: f64) {
        self.config.borrow_mut().sim.repulsion_strength = v.max(0.0);
        self.push();
    }

    pub fn set_chain_strength(&self, v: f64) {
        self.config.borrow_mut().sim.chain_strength = v.max(0.0);
        self.push();
    }

    pub fn set_chain_spacing(&self, v: f64) {
        self.config.borrow_mut().sim.chain_spacing = v.max(0.0);
        self.push();
    }

    pub fn set_chain_range(&self, v: f64) {
        self.config.borrow_mut().sim.chain_range = v.max(0.0);
        self.push();
    }

    pub fn set_chain_compress(&self, v: f64) {
        self.config.borrow_mut().sim.chain_compress = v.clamp(0.0, 1.0);
        self.push();
    }

    pub fn set_drag(&self, v: f64) {
        self.config.borrow_mut().sim.drag_coupling = v.clamp(0.0, 1.0);
        self.push();
    }
}
