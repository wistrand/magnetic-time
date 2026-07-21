//! JSON presets: serialize the whole clock configuration to a flat JSON
//! object and apply one back. Flat (one level; values are number, bool, or
//! string) keeps the parser small so the project stays serde-free. Applying is
//! lenient: unknown keys are ignored and missing keys keep their current
//! value, so presets are forward/backward compatible and partial presets work.
//! Numeric sim params are clamped to their `bounds` on the way in.

use std::collections::HashMap;

use crate::field::{parse_shape, FaceConfigs, FaceKind, LayoutSpec, SpecShape};
use crate::render::{parse_color, Palette, Style};
use crate::sim::{bounds, SimParams};

/// Current schema version, written as `"version"`. Bumped only if a key's
/// meaning changes incompatibly; readers ignore it (lenient apply covers the
/// compatible cases).
const VERSION: i64 = 1;

fn fmt_num(v: f64) -> String {
    // Whole numbers without a trailing ".0"; everything else via Display
    // (decimal, never scientific, so always valid JSON).
    if v.is_finite() && v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn shape_str(shape: SpecShape) -> String {
    match shape {
        SpecShape::Point => "point".to_string(),
        SpecShape::Disc { radius } => format!("disc:{radius}"),
        // parse_shape halves the width, so emit the full width.
        SpecShape::Rect { len_frac, half_wid } => format!("rect:{len_frac}x{}", half_wid * 2.0),
    }
}

fn face_str(kind: FaceKind) -> &'static str {
    match kind {
        FaceKind::Hands => "hands",
        FaceKind::Seg => "seg",
        FaceKind::Tide => "tide",
    }
}

fn num_line(e: &mut Vec<String>, k: &str, v: f64) {
    e.push(format!("{}: {}", q(k), fmt_num(v)));
}

/// Serialize a configuration to a flat JSON object (pretty, one key per line).
pub fn to_json(face: &FaceConfigs, sim: &SimParams, style: &Style, speed: f64) -> String {
    let mut e: Vec<String> = Vec::new();

    num_line(&mut e, "version", VERSION as f64);
    num_line(&mut e, "speed", speed);

    // Sim params.
    num_line(&mut e, "count", sim.count as f64);
    num_line(&mut e, "seed", sim.seed as f64);
    num_line(&mut e, "dt", sim.dt);
    num_line(&mut e, "mobility", sim.mobility);
    num_line(&mut e, "max_speed", sim.max_speed);
    num_line(&mut e, "noise", sim.noise);
    num_line(&mut e, "repulsion_radius", sim.repulsion_radius);
    num_line(&mut e, "repulsion_strength", sim.repulsion_strength);
    num_line(&mut e, "chain_strength", sim.chain_strength);
    num_line(&mut e, "b_sat", sim.b_sat);
    num_line(&mut e, "chain_spacing", sim.chain_spacing);
    num_line(&mut e, "chain_range", sim.chain_range);
    num_line(&mut e, "chain_compress", sim.chain_compress);
    num_line(&mut e, "chain_cone", sim.chain_cone);
    num_line(&mut e, "chain_speed_cap", sim.chain_speed_cap);
    num_line(&mut e, "chain_max_neighbors", sim.chain_max_neighbors as f64);
    num_line(&mut e, "drag_coupling", sim.drag_coupling);
    num_line(&mut e, "pointer_strength", sim.pointer_strength);
    num_line(&mut e, "pointer_radius", sim.pointer_radius);
    num_line(&mut e, "pointer_visual", sim.pointer_visual);
    num_line(&mut e, "field_clamp", sim.field_clamp);
    num_line(&mut e, "fluid_scale", sim.fluid_scale);

    // Face.
    e.push(format!("{}: {}", q("face"), q(face_str(face.kind))));
    num_line(&mut e, "seg_strength", face.seg.strength);
    e.push(format!("{}: {}", q("seg_seconds"), face.seg.with_seconds));
    num_line(&mut e, "seg_half_wid", face.seg.half_wid);
    num_line(&mut e, "tide_strength", face.tide.strength);
    num_line(&mut e, "tide_half_wid", face.tide.half_wid);
    for (i, h) in face.hands.iter().enumerate() {
        e.push(format!("{}: {}", q(&format!("hand{i}_layout")), q(&h.label())));
        num_line(&mut e, &format!("hand{i}_strength"), h.strength);
        e.push(format!(
            "{}: {}",
            q(&format!("hand{i}_shape")),
            q(&shape_str(h.shape))
        ));
    }

    // Style.
    e.push(format!("{}: {}", q("palette"), q(style.palette.name())));
    e.push(format!(
        "{}: {}",
        q("bg"),
        q(&format!("{:02x}{:02x}{:02x}", style.bg[0], style.bg[1], style.bg[2]))
    ));
    num_line(&mut e, "stroke_len", style.stroke_len);
    e.push(format!("{}: {}", q("show_hands"), style.show_hands));
    e.push(format!("{}: {}", q("show_fps"), style.show_fps));
    num_line(&mut e, "max_px", style.max_px as f64);
    num_line(&mut e, "heatmap_res", style.heatmap_res as f64);

    let body = e
        .iter()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join(",\n");
    format!("{{\n{body}\n}}\n")
}

enum Val {
    Num(f64),
    Bool(bool),
    Str(String),
}

/// Parse a flat JSON object into a key -> value map. Tolerant of extra
/// whitespace; only the shapes `to_json` emits (and hand edits of them) are
/// supported, not arbitrary nested JSON.
fn parse_flat(s: &str) -> Result<HashMap<String, Val>, String> {
    let b: Vec<char> = s.chars().collect();
    let n = b.len();
    let mut i = 0usize;
    let mut map = HashMap::new();

    let read_string = |b: &[char], i: &mut usize| -> Result<String, String> {
        // Assumes b[*i] == '"'.
        *i += 1;
        let mut out = String::new();
        while *i < n && b[*i] != '"' {
            if b[*i] == '\\' && *i + 1 < n {
                *i += 1;
            }
            out.push(b[*i]);
            *i += 1;
        }
        if *i >= n {
            return Err("preset: unterminated string".into());
        }
        *i += 1; // closing quote
        Ok(out)
    };

    while i < n && b[i].is_whitespace() {
        i += 1;
    }
    if i >= n || b[i] != '{' {
        return Err("preset: expected '{'".into());
    }
    i += 1;
    loop {
        while i < n && b[i].is_whitespace() {
            i += 1;
        }
        if i < n && b[i] == '}' {
            break;
        }
        if i >= n {
            return Err("preset: unterminated object".into());
        }
        if b[i] != '"' {
            return Err(format!("preset: expected a \"key\" near char {i}"));
        }
        let key = read_string(&b, &mut i)?;
        while i < n && b[i].is_whitespace() {
            i += 1;
        }
        if i >= n || b[i] != ':' {
            return Err(format!("preset: expected ':' after key '{key}'"));
        }
        i += 1;
        while i < n && b[i].is_whitespace() {
            i += 1;
        }
        if i >= n {
            return Err(format!("preset: missing value for '{key}'"));
        }
        let val = if b[i] == '"' {
            Val::Str(read_string(&b, &mut i)?)
        } else {
            let start = i;
            while i < n && b[i] != ',' && b[i] != '}' && !b[i].is_whitespace() {
                i += 1;
            }
            let tok: String = b[start..i].iter().collect();
            match tok.as_str() {
                "true" => Val::Bool(true),
                "false" => Val::Bool(false),
                _ => Val::Num(
                    tok.parse()
                        .map_err(|_| format!("preset: bad value '{tok}' for '{key}'"))?,
                ),
            }
        };
        map.insert(key, val);
        while i < n && b[i].is_whitespace() {
            i += 1;
        }
        if i < n && b[i] == ',' {
            i += 1;
            continue;
        }
        if i < n && b[i] == '}' {
            break;
        }
        if i >= n {
            break;
        }
        return Err(format!("preset: expected ',' or '}}' near char {i}"));
    }
    Ok(map)
}

/// Apply a JSON preset over the given configuration in place. Missing keys are
/// left unchanged; numeric sim params are clamped to their `bounds`.
pub fn apply_json(
    json: &str,
    face: &mut FaceConfigs,
    sim: &mut SimParams,
    style: &mut Style,
    speed: &mut f64,
) -> Result<(), String> {
    let m = parse_flat(json)?;
    let num = |k: &str| -> Option<f64> {
        match m.get(k) {
            Some(Val::Num(v)) => Some(*v),
            _ => None,
        }
    };
    let text = |k: &str| -> Option<&str> {
        match m.get(k) {
            Some(Val::Str(v)) => Some(v.as_str()),
            _ => None,
        }
    };
    let flag = |k: &str| -> Option<bool> {
        match m.get(k) {
            Some(Val::Bool(v)) => Some(*v),
            _ => None,
        }
    };

    if let Some(v) = num("speed") {
        *speed = v.clamp(0.0, 100_000.0);
    }

    if let Some(v) = num("count") {
        sim.count = (v as usize).max(1);
    }
    if let Some(v) = num("seed") {
        sim.seed = v as u64;
    }
    if let Some(v) = num("chain_max_neighbors") {
        sim.chain_max_neighbors = (v as u32).max(1);
    }
    // f64 sim params, clamped to their interactive/valid bounds.
    if let Some(v) = num("dt") {
        sim.dt = bounds::DT.clamp(v);
    }
    if let Some(v) = num("mobility") {
        sim.mobility = bounds::MOBILITY.clamp(v);
    }
    if let Some(v) = num("max_speed") {
        sim.max_speed = bounds::MAX_SPEED.clamp(v);
    }
    if let Some(v) = num("noise") {
        sim.noise = bounds::NOISE.clamp(v);
    }
    if let Some(v) = num("repulsion_radius") {
        sim.repulsion_radius = bounds::REPULSION_RADIUS.clamp(v);
    }
    if let Some(v) = num("repulsion_strength") {
        sim.repulsion_strength = bounds::REPULSION_STRENGTH.clamp(v);
    }
    if let Some(v) = num("chain_strength") {
        sim.chain_strength = bounds::CHAIN_STRENGTH.clamp(v);
    }
    if let Some(v) = num("b_sat") {
        sim.b_sat = bounds::B_SAT.clamp(v);
    }
    if let Some(v) = num("chain_spacing") {
        sim.chain_spacing = bounds::CHAIN_SPACING.clamp(v);
    }
    if let Some(v) = num("chain_range") {
        sim.chain_range = bounds::CHAIN_RANGE.clamp(v);
    }
    if let Some(v) = num("chain_compress") {
        sim.chain_compress = bounds::CHAIN_COMPRESS.clamp(v);
    }
    if let Some(v) = num("chain_cone") {
        sim.chain_cone = bounds::CHAIN_CONE.clamp(v);
    }
    if let Some(v) = num("chain_speed_cap") {
        sim.chain_speed_cap = bounds::CHAIN_SPEED_CAP.clamp(v);
    }
    if let Some(v) = num("drag_coupling") {
        sim.drag_coupling = bounds::DRAG_COUPLING.clamp(v);
    }
    if let Some(v) = num("pointer_strength") {
        sim.pointer_strength = bounds::POINTER_STRENGTH.clamp(v);
    }
    if let Some(v) = num("pointer_radius") {
        sim.pointer_radius = bounds::POINTER_RADIUS.clamp(v);
    }
    if let Some(v) = num("pointer_visual") {
        sim.pointer_visual = bounds::POINTER_VISUAL.clamp(v);
    }
    if let Some(v) = num("field_clamp") {
        sim.field_clamp = bounds::FIELD_CLAMP.clamp(v);
    }
    if let Some(v) = num("fluid_scale") {
        sim.fluid_scale = bounds::FLUID_SCALE.clamp(v);
    }

    // Face.
    if let Some(v) = text("face") {
        face.kind = match v {
            "seg" => FaceKind::Seg,
            "tide" => FaceKind::Tide,
            _ => FaceKind::Hands,
        };
    }
    if let Some(v) = num("seg_strength") {
        face.seg.strength = v.max(0.0);
    }
    if let Some(v) = flag("seg_seconds") {
        face.seg.with_seconds = v;
    }
    if let Some(v) = num("seg_half_wid") {
        face.seg.half_wid = v.max(0.0025);
    }
    if let Some(v) = num("tide_strength") {
        face.tide.strength = v.max(0.0);
    }
    if let Some(v) = num("tide_half_wid") {
        face.tide.half_wid = v.max(0.0025);
    }
    for i in 0..3 {
        if let Some(v) = text(&format!("hand{i}_layout")) {
            if let Ok(spec) = LayoutSpec::parse(v) {
                face.hands[i].kind = spec.kind;
                face.hands[i].n = spec.n;
            }
        }
        if let Some(v) = num(&format!("hand{i}_strength")) {
            face.hands[i].strength = v;
        }
        if let Some(v) = text(&format!("hand{i}_shape")) {
            if let Ok(shape) = parse_shape(v) {
                face.hands[i].shape = shape;
            }
        }
    }

    // Style.
    if let Some(v) = text("palette") {
        if let Ok(p) = Palette::parse(v) {
            style.palette = p;
        }
    }
    if let Some(v) = text("bg") {
        if let Ok(c) = parse_color(v) {
            style.bg = c;
        }
    }
    if let Some(v) = num("stroke_len") {
        style.stroke_len = v.clamp(0.0, 8.0);
    }
    if let Some(v) = flag("show_hands") {
        style.show_hands = v;
    }
    if let Some(v) = flag("show_fps") {
        style.show_fps = v;
    }
    if let Some(v) = num("max_px") {
        style.max_px = v.max(0.0) as u32;
    }
    if let Some(v) = num("heatmap_res") {
        style.heatmap_res = v.clamp(0.0, 1024.0) as u32;
    }

    Ok(())
}
