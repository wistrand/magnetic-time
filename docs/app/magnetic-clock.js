// <magnetic-clock> web component wrapping the magnetic-time wasm build.
// Attribute grammar matches the native CLI flags; see the project README.
//
//   <script type="module" src="magnetic-clock.js"></script>
//   <magnetic-clock particles="12000" speed="60" show-hands
//                   magnets="tip" shapes="rect:1x0.03"
//                   strengths="0.1,0.05,0.6"></magnetic-clock>
//
// Boolean attributes: show-hands, dev-panel (presence = on).
// All attributes are live; changing one reconfigures the running sim.

import init, { WebHandle } from "./pkg/magnetic-time.js";

let wasmInit;
const ensureWasm = () => (wasmInit ??= init());

// Application order matters: "magnets" resets per-hand strength/shape, so
// "strengths" and "shapes" are re-applied after it.
const ATTRS = [
  "face", "seg-strength", "tide-strength", "magnets", "strengths", "shapes",
  "particles", "speed", "stroke-len", "palette", "bg", "show-hands", "fps", "dev-panel",
  "mobility", "max-speed", "noise", "repulsion",
  "chain-strength", "chain-spacing", "chain-range", "chain-compress", "drag",
  "pointer-strength", "pointer-radius", "pointer-visual", "pointer-repel", "max-px",
  "fluid-scale", "heatmap",
];

class MagneticClock extends HTMLElement {
  static observedAttributes = ATTRS;

  async connectedCallback() {
    if (!this.shadowRoot) {
      const shadow = this.attachShadow({ mode: "open" });
      const style = document.createElement("style");
      style.textContent =
        ":host { display: block; background: #10121a; } " +
        "canvas { display: block; width: 100%; height: 100%; outline: none; }";
      this.canvas = document.createElement("canvas");
      shadow.append(style, this.canvas);
    }
    await ensureWasm();
    if (!this.isConnected || this.handle) return;
    this.handle = new WebHandle();
    this.applyAll();
    await this.handle.start(this.canvas);
  }

  disconnectedCallback() {
    this.handle?.destroy();
    this.handle = null;
  }

  attributeChangedCallback() {
    // Re-apply everything so magnets/strengths/shapes stay consistent.
    if (this.handle) this.applyAll();
  }

  // Save the current configuration as a JSON preset string (or null before
  // the clock has started). Persist it however you like, e.g. localStorage.
  savePreset() {
    return this.handle ? this.handle.get_preset() : null;
  }

  // Apply a JSON preset string previously produced by savePreset().
  loadPreset(json) {
    this.handle?.set_preset(json);
  }

  applyAll() {
    const h = this.handle;
    const num = (name) => {
      const v = parseFloat(this.getAttribute(name));
      return Number.isFinite(v) ? v : null;
    };
    for (const name of ATTRS) {
      try {
        if (name === "show-hands") {
          h.set_show_hands(this.hasAttribute(name));
          continue;
        }
        if (name === "fps") {
          h.set_show_fps(this.hasAttribute(name));
          continue;
        }
        if (name === "pointer-repel") {
          h.set_pointer_repel(this.hasAttribute(name));
          continue;
        }
        if (name === "dev-panel") {
          h.set_dev_panel(this.hasAttribute(name));
          continue;
        }
        if (!this.hasAttribute(name)) continue;
        if (name === "face") {
          h.set_face(this.getAttribute(name));
          continue;
        }
        if (name === "magnets") {
          h.set_magnets(this.getAttribute(name));
          continue;
        }
        if (name === "strengths") {
          h.set_strengths(this.getAttribute(name));
          continue;
        }
        if (name === "shapes") {
          h.set_shapes(this.getAttribute(name));
          continue;
        }
        if (name === "palette") {
          h.set_palette(this.getAttribute(name));
          continue;
        }
        if (name === "bg") {
          h.set_bg(this.getAttribute(name));
          this.style.background = "#" + this.getAttribute(name).replace("#", "");
          continue;
        }
        const v = num(name);
        if (v === null) continue;
        switch (name) {
          case "particles": h.set_particles(v); break;
          case "speed": h.set_speed(v); break;
          case "stroke-len": h.set_stroke_len(v); break;
          case "mobility": h.set_mobility(v); break;
          case "max-speed": h.set_max_speed(v); break;
          case "noise": h.set_noise(v); break;
          case "repulsion": h.set_repulsion(v); break;
          case "seg-strength": h.set_seg_strength(v); break;
          case "tide-strength": h.set_tide_strength(v); break;
          case "chain-strength": h.set_chain_strength(v); break;
          case "chain-spacing": h.set_chain_spacing(v); break;
          case "chain-range": h.set_chain_range(v); break;
          case "chain-compress": h.set_chain_compress(v); break;
          case "drag": h.set_drag(v); break;
          case "pointer-strength": h.set_pointer_strength(v); break;
          case "pointer-radius": h.set_pointer_radius(v); break;
          case "pointer-visual": h.set_pointer_visual(v); break;
          case "max-px": h.set_max_px(v); break;
          case "heatmap": h.set_heatmap(v); break;
          case "fluid-scale": h.set_fluid_scale(v); break;
        }
      } catch (e) {
        console.warn(`magnetic-clock: attribute ${name}:`, e);
      }
    }
  }
}

customElements.define("magnetic-clock", MagneticClock);
