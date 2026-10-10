//! Rendering of the Chroma Studio layers, with the parameters as Synapse writes them in the logs.
//!
//! Each layer paints on its own LEDs over what is already there (alpha < 1 = lets the layers
//! below show through). Audio Meter and Reactive are in `effect.rs` because they depend on the
//! meter and on the clicks.
//!
//! Everything that follows is measured on the Chroma Studio preview, which shows the frames of
//! Razer's engine (2026-09-19): speed, geometry, shape over time. The few estimates left
//! are marked as such.

use serde_json::Value;
use std::collections::HashMap;
use std::f64::consts::TAU;

pub type Rgb = (u8, u8, u8);
pub type Stops = Vec<(f64, Rgb)>;

pub const BLACK: Rgb = (0, 0, 0);
const DEV_W: f64 = 200.0; // the device's canvas in Chroma Studio (px)
const DEV_H: f64 = 320.0;
const CELL: f64 = 40.0; // side of one cell of the LED grid on the editor's canvas (px)
// The engine takes 2 rows off the centre of wheel and tidal (`recalculateCenterPos`, on by default):
// measured, the centre of the mouse ends up at row 2 instead of row 4.
const CENTER_ROW_SHIFT: f64 = 2.0;

// Wave: the engine advances in 40 ms ticks and with width 100% an LED repeats the gradient every
// floor(625 / speed) ticks (speed 10 -> 2.48 s, 15 -> 1.64 s, 25 -> 1.00 s, 50 -> 0.48 s); the
// period grows in proportion to the width and does not depend on angle and split.
const WAVE_TICK: f64 = 0.04;
const WAVE_TICKS: f64 = 625.0;
const TIDAL_PERIOD: f64 = 50.0; // s * speed: speed 10 -> 5.0 s, 15 -> 3.33 s, 30 -> 1.66 s
// Wheel: "speed" = degrees per second (60 -> one cycle in 6 s, 120 -> 3 s).
// Ripple: the rings start from the centre of the MOUSE (not of the region) and the front advances by
// `speed` cells per second. An LED at distance D (in cells, rounded) lights up when the
// front r reaches D-1, is full from D to D+W-1 (W = width/100) and goes off at D+W. The colour is
// entry max(floor(r), D) of a table of 6+W colours derived from the gradient (`ripple_table`).
// With speed > 12.5 (more than half a cell every 40 ms) the engine does not fade: LED full for
// D <= floor(r) <= D+W-1, off otherwise. Speed 50 behaves like 25.
// A new ring every floor((20 + 2W) / (0.08 * speed)) + 1 steps of 80 ms (measured on 13
// combinations of speed 1..35 and width 100..400: speed 1 -> 22.08 s, 10 -> 2.24 s, 20 -> 1.12 s).
const RIPPLE_TABLE: f64 = 6.0;
const RIPPLE_TRAVEL: f64 = 20.0;
const RIPPLE_STEP: f64 = 0.08;
// Starlight: at most density*10% of the LEDs lit; about 6/duration stars are born per second;
// each star lives between 20% and 100% of the duration with a fixed random brightness (20..87%).
const STAR_SPAWN: f64 = 6.0;
const STAR_LIFE_MIN: f64 = 0.2;
const STAR_LIGHT: (f64, f64) = (0.2, 0.87);
// Fire: each LED has a fixed tint (a quarter, half, three quarters or all color2) and an intensity
// that flickers on its own (about every 0.25 s) around a mean that falls going up from the bottom
// of the region (bottom row = back of the mouse) towards the top.
const FIRE_STEP: f64 = 0.25;
const FIRE_HEAT: [(f64, f64); 7] = [(0.0, 0.48), (1.0 / 6.0, 0.44), (1.0 / 3.0, 0.42), (0.5, 0.22), (2.0 / 3.0, 0.15), (5.0 / 6.0, 0.03), (1.0, 0.03)];
const FIRE_MAX: f64 = 0.85;

pub fn rgb(c: u32) -> Rgb {
    (((c >> 16) & 255) as u8, ((c >> 8) & 255) as u8, (c & 255) as u8)
}

pub fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let ch = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t) as i64 as u8;
    (ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
}

fn dim(c: Rgb, k: f64) -> Rgb {
    lerp(BLACK, c, k)
}

/// stops: [(0..100, rgb)], pos: 0..1
pub fn gradient(stops: &[(f64, Rgb)], pos: f64) -> Rgb {
    let p = pos * 100.0;
    for w in stops.windows(2) {
        let ((s0, c0), (s1, c1)) = (w[0], w[1]);
        if p <= s1 {
            return lerp(c0, c1, if s1 > s0 { (p - s0) / (s1 - s0) } else { 0.0 });
        }
    }
    stops.last().map(|s| s.1).unwrap_or(BLACK)
}

/// Table of `n` colours of the ripple, as the engine builds it (measured with gradients of 2 and 3
/// stops in different positions): each segment between two stops takes the entries from round(stop * n / 100)
/// to the next one; in the inner segments the end stop falls on the segment's last entry, in the last
/// segment it does not (entry j of L: j / (L - 1) in the inner segments, j / L in the last).
fn ripple_table(stops: &[(f64, Rgb)], n: usize) -> Vec<Rgb> {
    let mut st: Stops = stops.to_vec();
    st.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (Some(&first), Some(&last)) = (st.first(), st.last()) else { return vec![BLACK; n.max(1)] };
    if first.0 > 0.0 {
        st.insert(0, (0.0, first.1));
    }
    if last.0 < 100.0 {
        st.push((100.0, last.1));
    }
    let at = |s: f64| ((s * n as f64 / 100.0 + 0.5).floor() as usize).min(n);
    let mut table = Vec::with_capacity(n);
    let segs = st.len() - 1;
    for (k, w) in st.windows(2).enumerate() {
        let len = at(w[1].0) - at(w[0].0);
        let div = if k + 1 == segs { len } else { len.saturating_sub(1) };
        for j in 0..len {
            table.push(lerp(w[0].1, w[1].1, if div > 0 { j as f64 / div as f64 } else { 0.0 }));
        }
    }
    table.resize(n.max(1), last.1);
    table
}

/// Linear interpolation in a table [(x, y)] sorted by x.
fn table(t: &[(f64, f64)], x: f64) -> f64 {
    for w in t.windows(2) {
        if x <= w[1].0 {
            let k = if w[1].0 > w[0].0 { ((x - w[0].0) / (w[1].0 - w[0].0)).max(0.0) } else { 0.0 };
            return w[0].1 + (w[1].1 - w[0].1) * k;
        }
    }
    t.last().map_or(0.0, |v| v.1)
}

/// Xorshift generator: more than enough for random stars and waves, with no dependencies.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    pub fn f64(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Random colour as Razer's engine picks them (measured on breathing and starlight):
    /// each channel is 0, 128 or 255, at least one is 255.
    pub fn color(&mut self) -> Rgb {
        loop {
            let mut ch = || [0u8, 128, 255][((self.f64() * 3.0) as usize).min(2)];
            let c = (ch(), ch(), ch());
            if c.0 == 255 || c.1 == 255 || c.2 == 255 {
                return c;
            }
        }
    }
    /// Random colour of the Reactive (measured with real clicks): each channel 0, 128 or 255, never black
    /// (0,0,128 and 128,128,0 came up too).
    pub fn any_color(&mut self) -> Rgb {
        loop {
            let mut ch = || [0u8, 128, 255][((self.f64() * 3.0) as usize).min(2)];
            let c = (ch(), ch(), ch());
            if c != BLACK {
                return c;
            }
        }
    }
}

/// Position on the canvas of every physical LED, derived from the Chroma Studio grid
/// ({(row, column): LED index}). Razer's engine works on the LED grid, with cells
/// of 40 px on the editor's canvas (`centerPointX` = px / 40), and puts each LED on the index
/// of its cell (measured: the centre of the wheel falls exactly on whole rows and columns).
pub struct Canvas {
    pub pos: Vec<(f64, f64)>,
    middle: (f64, f64), // centre of the mouse's grid: the ripple's rings start from here
}

impl Canvas {
    pub fn new(grid: &HashMap<(i64, i64), usize>, leds: usize) -> Self {
        let mut pos = vec![(DEV_W / 2.0, DEV_H / 2.0); leds];
        for (&(r, c), &i) in grid {
            if i < leds {
                pos[i] = (c as f64 * CELL, r as f64 * CELL);
            }
        }
        let mut canvas = Canvas { pos, middle: (0.0, 0.0) };
        canvas.middle = canvas.region(&grid.values().cloned().filter(|&i| i < leds).collect::<Vec<_>>()).0;
        canvas
    }

    /// Row and column of LED `i` in the Chroma Studio grid.
    pub fn cell(&self, i: usize) -> (i64, i64) {
        let (x, y) = self.pos[i];
        ((y / CELL).round() as i64, (x / CELL).round() as i64)
    }

    /// Position of the LED relative to `center` with each axis divided by its extent in
    /// `size`: the engine normalises rows and columns separately (measured: at 45 degrees a wave
    /// advances by 1/5 * 0.71 per column and 1/7 * 0.71 per row on a region of 5x7 cells).
    fn norm(&self, i: usize, center: (f64, f64), size: (f64, f64)) -> (f64, f64) {
        let (x, y) = self.pos[i];
        ((x - center.0) / size.0, (y - center.1) / size.1)
    }

    /// Bounding box of the cells of a region's LEDs: centre and size (px). Razer's engine
    /// spreads the wave over the region, not over the whole mouse: with width 100% one wave covers the
    /// rows of the region (measured: region of 3 rows -> 1/3 of a period per row, of 4 -> 1/4).
    fn region(&self, leds: &[usize]) -> ((f64, f64), (f64, f64)) {
        if leds.is_empty() {
            return ((DEV_W / 2.0, DEV_H / 2.0), (DEV_W, DEV_H));
        }
        let (xs, ys): (Vec<f64>, Vec<f64>) = leds.iter().map(|&i| self.pos[i]).unzip();
        let lo = |v: &[f64]| v.iter().cloned().fold(f64::INFINITY, f64::min) - CELL / 2.0;
        let hi = |v: &[f64]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + CELL / 2.0;
        let (x0, x1, y0, y1) = (lo(&xs), hi(&xs), lo(&ys), hi(&ys));
        (((x0 + x1) / 2.0, (y0 + y1) / 2.0), (x1 - x0, y1 - y0))
    }
}

/// Wave along `angle` (degrees as on a compass: 0 = upwards, 90 = to the right) in the
/// normalised coordinates of `Canvas::norm`.
fn along((u, v): (f64, f64), angle: f64) -> f64 {
    let a = angle.to_radians();
    u * a.sin() - v * a.cos()
}

/// Centre of wheel and tidal on the mouse's canvas, as the engine uses it: `centerPointX/Y` is
/// the point chosen in Chroma Studio in 40 px cells of the editor's canvas (rounded), the
/// mouse is at `deviceWithCenterPoint.x/y` px, and the engine takes off 2 rows. None = the data is missing.
fn center_point(lp: &Value) -> Option<(f64, f64)> {
    let dev = &lp["deviceWithCenterPoint"];
    let (dx, dy) = (dev["x"].as_f64()?, dev["y"].as_f64()?);
    let cell = |k: &str, k2: &str| lp[k].as_f64().or_else(|| lp[k2].as_f64().map(|v| (v / CELL).round()));
    let (cx, cy) = (cell("centerPointX", "centerPointX2")?, cell("centerPointY", "centerPointY2")?);
    Some(((cx - (dx / CELL).round()) * CELL, (cy - (dy / CELL).round() - CENTER_ROW_SHIFT) * CELL))
}

fn num(p: &Value, key: &str, default: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn flag(p: &Value, key: &str) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn color(p: &Value, key: &str) -> Option<Rgb> {
    p.get(key).and_then(Value::as_u64).map(|c| rgb(c as u32))
}

pub fn stops(p: &Value) -> Option<Stops> {
    let list = p.get("colorStops")?.as_array()?;
    Some(list.iter().map(|s| (num(s, "Stop", 0.0), rgb(num(s, "Color", 0.0) as u32))).collect())
}

/// When an effect started by a click stops (`playbackEnd`).
#[derive(Clone, Copy, PartialEq)]
enum End {
    Never,
    After(u32),  // after N cycles (`cycles`)
    Toggle(u32), // like After, but a click while it runs stops it
}

#[derive(Clone, Copy)]
struct Star {
    t0: f64,
    life: f64,
    light: f64,
    color: Option<Rgb>, // Some = random colour: it brightens and then dims
}

enum Kind {
    Static(Option<Rgb>), // None = empty colour: the layer covers nothing
    /// The whole region in the same colour, one cycle of the gradient every `period` s.
    Spectrum { stops: Stops, period: f64 },
    /// Wave, Tidal and Wheel: the gradient scrolls, `phase` is the position of each LED of the region.
    /// `pause`: cycles of darkness after each pass (wave). `clear_black`: black lets the
    /// layers below show through (tidal). `per_led`: started by a click, each LED finishes its own cycle
    /// (wave, tidal); otherwise everything goes off together after N cycles (wheel).
    Scroll { stops: Stops, phase: Vec<f64>, hz: f64, pause: f64, clear_black: bool, per_led: bool },
    /// Rings from the centre of the mouse: `dist` is the distance of each LED in cells, `table` the colours
    /// by radius, `rings` the times at which they started, `next` when the next one starts
    /// (infinite = waits for a click).
    Ripple { table: Vec<Rgb>, speed: f64, width: f64, period: f64, dist: Vec<f64>, rings: Vec<f64>, next: f64 },
    /// Stars born at random: the gradient is the life of a star, `max` how many at once.
    Starlight { stops: Stops, rnd: bool, life: f64, max: usize, rate: f64, stars: Vec<Option<Star>>, last: Option<f64> },
    /// `rnd`: one breath per `period` with a random colour; otherwise one cycle per `period`:
    /// dark, breath of color, dark, breath of color2, dark.
    Breathing { fixed: Option<Stops>, c1: Rgb, c2: Rgb, rnd: bool, period: f64, cycle: i64, stops: Stops },
    /// Fixed colour per LED, flickering intensity: `knots` = (value before, value after,
    /// start of the segment) per LED, `heat` = mean intensity per LED.
    Fire { colors: Vec<Rgb>, heat: Vec<f64>, knots: Vec<(f64, f64, f64)> },
}

pub struct Layer {
    pub leds: Vec<usize>,
    pub on_press: bool, // starts with a click (for the ripple: rings from the centre at every click)
    end: End,
    running: bool,
    start: f64, // time of the click that started it: the animation restarts from the beginning
    lead: f64,  // s already elapsed at the click (wave: one tick of the engine)
    kind: Kind,
}

fn breathing_stops(c1: Rgb, c2: Rgb) -> Stops {
    vec![(0.0, BLACK), (12.0, BLACK), (25.0, c1), (38.0, c1), (50.0, BLACK), (62.0, BLACK), (75.0, c2), (88.0, c2), (100.0, BLACK)]
}

/// A single breath, with the same shape as each of the two in the normal cycle.
fn one_breath(c: Rgb) -> Stops {
    vec![(0.0, BLACK), (24.0, BLACK), (50.0, c), (76.0, c), (100.0, BLACK)]
}

impl Layer {
    /// `name`: the layer's name in the logs; `p`: the region's parameters; `lp`: the layer's parameters.
    /// None = unsupported layer type.
    pub fn new(name: &str, leds: Vec<usize>, p: &Value, lp: &Value, canvas: &Canvas, rng: &mut Rng) -> Option<Layer> {
        // "onSelectedKeys": on the mouse no button is tied to an LED (LedInputMap), so
        // any click counts, like "onPress"
        let on_press = matches!(p.get("playbackStart").and_then(Value::as_str), Some("onPress" | "onSelectedKeys"));
        let end = match p.get("playbackEnd").and_then(Value::as_str) {
            _ if !on_press => End::Never,
            Some("toggle") => End::Toggle(num(p, "cycles", 1.0).max(1.0) as u32),
            Some("never") => End::Never,
            _ => End::After(num(p, "cycles", 1.0).max(1.0) as u32),
        };
        let mut running = !on_press;
        let mut lead = 0.0;
        let (middle, size) = canvas.region(&leds);
        let kind = match name {
            "static" => Kind::Static(color(p, "color").or_else(|| color(p, "color2"))),
            "spectrum" => Kind::Spectrum { stops: stops(p)?, period: num(p, "duration", 37740.0).max(100.0) / 1000.0 },
            "wave" | "tidal" => {
                let st = match (name, stops(p)) {
                    (_, Some(s)) => s,
                    ("tidal", None) if flag(p, "randomColor") => {
                        let (c1, c2) = (rng.color(), rng.color());
                        vec![(0.0, BLACK), (15.0, BLACK), (30.0, c1), (40.0, BLACK), (65.0, BLACK), (70.0, c2), (90.0, BLACK), (100.0, BLACK)]
                    }
                    ("tidal", None) => {
                        let (c1, c2) = (color(p, "color").unwrap_or(rgb(0x00FF00)), color(p, "color2").unwrap_or(rgb(0x0000FF)));
                        vec![(0.0, BLACK), (10.0, BLACK), (25.0, c1), (40.0, BLACK), (60.0, BLACK), (75.0, c2), (90.0, BLACK), (100.0, BLACK)]
                    }
                    _ => return None,
                };
                let tidal = name == "tidal";
                let length = num(p, "width", 100.0).max(1.0) / 100.0; // wavelengths per region
                let speed = num(p, "speed", if tidal { 10.0 } else { 15.0 });
                let period = match (tidal, speed > 0.0) {
                    (_, false) => f64::INFINITY,
                    (true, true) => TIDAL_PERIOD / speed * length,
                    (false, true) => (WAVE_TICKS / speed).floor().max(1.0) * WAVE_TICK * length,
                };
                let hz = 1.0 / period;
                let pause = if tidal { 0.0 } else { num(p, "pause", 0.0).max(0.0) / 1000.0 * hz }; // ms -> cycles
                // boolean `counterclockwise` in the region (the mouse's quick effects) = wave that goes round in a circle
                let spin = p.get("counterclockwise").and_then(Value::as_bool).map(|ccw| if ccw { -1.0 } else { 1.0 });
                let phase = leds
                    .iter()
                    .map(|&i| {
                        let pos = if let Some(dir) = spin {
                            let (x, y) = canvas.pos[i];
                            (y - middle.1).atan2(x - middle.0) / TAU * dir
                        } else if tidal {
                            // tidal (measured): always mirrored around the chosen centre, with the
                            // direction rotated by 90 degrees from the angle; Outward moves away
                            // from the centre, Inward (`counterclockwise`) moves back to it
                            let center = center_point(lp).unwrap_or(middle);
                            let d = along(canvas.norm(i, center, size), num(p, "angle", 160.0) - 90.0).abs();
                            if flag(lp, "counterclockwise") { -d } else { d }
                        } else {
                            // wave: with split the two halves start from the centre of the region
                            let d = along(canvas.norm(i, middle, size), num(p, "angle", 90.0));
                            if flag(p, "split") { d.abs() } else { d + 0.5 }
                        };
                        pos / length
                    })
                    .collect::<Vec<f64>>();
                // wave started by a click (measured with real clicks): at the click the engine is already
                // one tick ahead
                if !tidal && spin.is_none() {
                    lead = WAVE_TICK;
                }
                Kind::Scroll { stops: st, phase, hz, pause, clear_black: tidal, per_led: true }
            }
            "wheel" => {
                let (cx, cy) = center_point(lp).unwrap_or(middle);
                let dir = if flag(lp, "counterclockwise") { -1.0 } else { 1.0 };
                // the angle starts from the left of the centre (measured with real clicks: at the click the gradient is
                // half a turn ahead of the angle counted from the right)
                let phase = leds.iter().map(|&i| (canvas.pos[i].1 - cy).atan2(canvas.pos[i].0 - cx) / TAU + 0.5).collect();
                // started by a click (measured with real clicks, speed 180): it does N whole cycles and
                // goes off all together
                Kind::Scroll { stops: stops(p)?, phase, hz: num(p, "speed", 60.0) / 360.0 * dir, pause: 0.0, clear_black: false, per_led: false }
            }
            "ripple" => {
                running = true;
                let (speed, width) = (num(p, "speed", 10.0).max(1.0), num(p, "width", 300.0).max(100.0) / 100.0);
                // measured: speed 50 (the maximum) behaves like 25, same period and same step; 45..49 do not
                let speed = if speed >= 50.0 { 25.0 } else { speed };
                let c = canvas.middle;
                let dist = leds.iter().map(|&i| ((canvas.pos[i].0 - c.0) / CELL).hypot((canvas.pos[i].1 - c.1) / CELL).round()).collect();
                let steps = ((RIPPLE_TRAVEL + 2.0 * width) / (RIPPLE_STEP * speed) + 1e-9).floor() + 1.0;
                Kind::Ripple {
                    table: ripple_table(&stops(p)?, (RIPPLE_TABLE + width).round() as usize),
                    speed,
                    width,
                    period: steps * RIPPLE_STEP,
                    dist,
                    rings: Vec::new(),
                    next: if on_press { f64::INFINITY } else { 0.0 }, // with a click: no rings until a button is pressed
                }
            }
            "starlight" => {
                let dens = num(p, "density", 1.0).clamp(0.0, 10.0);
                let life = num(p, "duration", 1700.0).max(100.0) / 1000.0;
                Kind::Starlight {
                    stops: stops(p)?,
                    rnd: flag(p, "randomColor"),
                    life,
                    max: if dens > 0.0 { ((leds.len() as f64 * dens / 10.0).round() as usize).max(1) } else { 0 },
                    rate: STAR_SPAWN / life,
                    stars: vec![None; leds.len()],
                    last: None,
                }
            }
            "breathing" => {
                let rnd = flag(p, "randomColor");
                let c1 = color(p, "color").unwrap_or(rgb(0x00FF00));
                // with random colours the engine halves the duration: one breath per half duration,
                // each in a new colour (measured: same breathing rhythm as the normal cycle)
                let duration = num(p, "duration", 14000.0) / if rnd { 2.0 } else { 1.0 };
                Kind::Breathing {
                    fixed: if rnd { None } else { stops(p) },
                    c1,
                    c2: color(p, "color2").unwrap_or(c1),
                    rnd,
                    period: duration.max(200.0) / 1000.0,
                    cycle: -1,
                    stops: Vec::new(),
                }
            }
            "fire" => {
                let c1 = color(p, "color").unwrap_or(rgb(0xFF0000));
                let c2 = color(p, "color2").unwrap_or(c1);
                let (top, bottom) = (middle.1 - size.1 / 2.0 + CELL / 2.0, middle.1 + size.1 / 2.0 - CELL / 2.0);
                let colors = leds.iter().map(|_| lerp(c1, c2, (1.0 + (rng.f64() * 4.0).floor().min(3.0)) / 4.0)).collect();
                let heat = leds
                    .iter()
                    .map(|&i| table(&FIRE_HEAT, if bottom > top { (bottom - canvas.pos[i].1) / (bottom - top) } else { 0.0 }))
                    .collect();
                Kind::Fire { colors, heat, knots: vec![(0.0, 0.0, f64::NEG_INFINITY); leds.len()] }
            }
            _ => return None,
        };
        Some(Layer { leds, on_press, end, running, start: 0.0, lead, kind })
    }

    pub fn press(&mut self, now: f64) {
        if let Kind::Ripple { rings, period, next, .. } = &mut self.kind {
            match self.end {
                // toggle: the click turns the emission of rings on or off
                End::Toggle(_) => *next = if next.is_finite() { f64::INFINITY } else { now },
                // one ring per cycle, one every period
                End::After(n) => rings.extend((0..n).map(|k| now + k as f64 * *period)),
                End::Never => rings.push(now),
            }
            return;
        }
        // measured with real clicks (wheel, breathing): with After a click while it runs does not count
        match self.end {
            End::Toggle(_) if self.running => {
                self.running = false;
                return;
            }
            End::After(_) if self.running => return,
            _ => {}
        }
        self.running = true;
        self.start = now;
    }

    /// Duration of one cycle, to stop the effects started by a click after N cycles.
    fn cycle(&self) -> Option<f64> {
        match &self.kind {
            Kind::Scroll { hz, pause, .. } if *hz != 0.0 => Some((1.0 + pause) / hz.abs()),
            Kind::Breathing { period, .. } => Some(*period),
            _ => None,
        }
    }

    pub fn paint(&mut self, now: f64, frame: &mut [Rgb], rng: &mut Rng) {
        if !self.running {
            return;
        }
        let on_press = self.on_press;
        // started by a click: the animation restarts from the beginning and stops after N cycles
        let t = if on_press { now - self.start + self.lead } else { now };

        let cycles = match (on_press, self.end) {
            (true, End::After(n) | End::Toggle(n)) => Some(n as f64),
            _ => None,
        };
        let scroll = matches!(self.kind, Kind::Scroll { per_led: true, .. });
        if let (Some(n), Some(c), false) = (cycles, self.cycle(), scroll) {
            if t >= n * c {
                self.running = false;
                return;
            }
        }
        let mut lit_any = false;
        for (n, &i) in self.leds.iter().enumerate() {
            let out: Option<(Rgb, f64)> = match &mut self.kind {
                Kind::Static(c) => c.map(|c| (c, 1.0)),
                Kind::Spectrum { stops, period } => Some((gradient(stops, now.rem_euclid(*period) / *period), 1.0)),
                Kind::Scroll { stops, phase, hz, pause, clear_black, per_led } => {
                    // u = how much of the gradient has already passed over the LED; with the pause, after one
                    // pass the LED stays off for `pause` cycles
                    let raw = t * *hz - phase[n];
                    // started by a click with N cycles (measured with real clicks, wave speed 25): each LED
                    // restarts from the point of the gradient it is at, scrolls to the start of the
                    // gradient, does another N-1 whole cycles and goes off
                    let over = *per_led && cycles.is_some_and(|k| {
                        let per = 1.0 + *pause;
                        let b0 = self.lead * *hz - phase[n]; // where the LED was at the click
                        let (a, b) = if *hz >= 0.0 { (raw, b0) } else { (-raw, -b0) };
                        a >= ((b / per - 1e-9).ceil() + k - 1.0) * per
                    });
                    lit_any |= !over;
                    let u = raw.rem_euclid(1.0 + *pause);
                    let c = gradient(stops, (-u).rem_euclid(1.0));
                    (!over && u < 1.0 && !(*clear_black && c == BLACK)).then_some((c, 1.0))
                }
                Kind::Ripple { table, speed, width, period, dist, rings, next } => {
                    // the front starts from the centre of the mouse (measured with real clicks: the first ring
                    // reaches the nearest LED, 2 cells away, 0.1 s after the click at speed 10)
                    let start = 0.0;
                    if n == 0 {
                        if now >= *next {
                            rings.push(now);
                            *next = now + *period;
                        }
                        let far = dist.iter().cloned().fold(0.0, f64::max);
                        rings.retain(|&t0| start + (now - t0) * *speed < far + *width);
                    }
                    let d = dist[n];
                    let mut best: Option<(Rgb, f64)> = None;
                    for &t0 in rings.iter().filter(|&&t0| t0 <= now) {
                        let r = start + (now - t0) * *speed;
                        let light = if *speed * RIPPLE_STEP / 2.0 > 0.5 {
                            // above half a cell per 40 ms tick no fading: fully lit
                            // while the front is between D and D+W
                            if r.floor() >= d && r.floor() <= d + *width - 1.0 { 1.0 } else { 0.0 }
                        } else {
                            (r - (d - 1.0)).min(d + *width - r).min(1.0)
                        };
                        // overlapping rings (clicks close together, measured): the most recent one wins
                        if light > 0.0 {
                            let k = r.floor().max(d).clamp(0.0, (table.len() - 1) as f64) as usize;
                            best = Some((table[k], light));
                        }
                    }
                    // where the ring passes it covers the layers below, dimmed (measured over a static)
                    best.map(|(c, light)| (dim(c, light), 1.0))
                }
                Kind::Starlight { stops, rnd, life, max, rate, stars, last } => {
                    if n == 0 {
                        let dt = last.map_or(0.0, |l| (now - l).min(0.5));
                        *last = Some(now);
                        for s in stars.iter_mut() {
                            if matches!(s, Some(st) if now - st.t0 >= st.life) {
                                *s = None;
                            }
                        }
                        let lit = stars.iter().filter(|x| x.is_some()).count();
                        if lit < *max && rng.f64() < *rate * dt {
                            let free: Vec<usize> = (0..stars.len()).filter(|&s| stars[s].is_none()).collect();
                            let s = free[((rng.f64() * free.len() as f64) as usize).min(free.len() - 1)];
                            stars[s] = Some(Star {
                                t0: now,
                                life: *life * (STAR_LIFE_MIN + (1.0 - STAR_LIFE_MIN) * rng.f64()),
                                light: STAR_LIGHT.0 + (STAR_LIGHT.1 - STAR_LIGHT.0) * rng.f64(),
                                color: if *rnd { Some(rng.color()) } else { None },
                            });
                        }
                    }
                    stars[n].map(|st| {
                        let k = (now - st.t0) / st.life;
                        match st.color {
                            // random colour: the brightness rises and falls; otherwise the gradient scrolls at fixed brightness
                            Some(c) => (dim(c, st.light * (1.0 - (2.0 * k - 1.0).abs())), 1.0),
                            None => (dim(gradient(stops, k), st.light), 1.0),
                        }
                    })
                }
                Kind::Breathing { fixed, c1, c2, rnd, period, cycle, stops } => {
                    let c = (t / *period).floor() as i64;
                    if c != *cycle {
                        *cycle = c;
                        *stops = match fixed {
                            Some(s) => s.clone(),
                            None if *rnd => one_breath(rng.color()),
                            None => breathing_stops(*c1, *c2),
                        };
                    }
                    Some((gradient(stops, (t / *period).rem_euclid(1.0)), 1.0))
                }
                Kind::Fire { colors, heat, knots } => {
                    let (from, to, t0) = &mut knots[n];
                    if now - *t0 >= FIRE_STEP {
                        // new segment: it starts from where it was and goes towards a random value around the mean
                        let at = if t0.is_finite() { *to } else { heat[n] };
                        *from = at;
                        *to = (heat[n] * (0.2 + 1.6 * rng.f64())).min(FIRE_MAX);
                        *t0 = if t0.is_finite() && now - *t0 < 2.0 * FIRE_STEP { *t0 + FIRE_STEP } else { now };
                    }
                    let k = ((now - *t0) / FIRE_STEP).clamp(0.0, 1.0);
                    let s = (1.0 - (k * std::f64::consts::PI).cos()) / 2.0; // smooth transition
                    Some((dim(colors[n], *from + (*to - *from) * s), 1.0))
                }
            };
            if let Some((c, a)) = out {
                frame[i] = if a >= 1.0 { c } else { lerp(frame[i], c, a) };
            }
        }
        if scroll && cycles.is_some() && !lit_any {
            self.running = false; // all the LEDs have finished their cycles: a click restarts it
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn canvas() -> Canvas {
        Canvas::new(&HashMap::from([((0, 2), 0), ((6, 4), 1)]), 2)
    }

    fn layer(name: &str, p: Value, lp: Value) -> Layer {
        Layer::new(name, vec![0, 1], &p, &lp, &canvas(), &mut Rng::new(1)).unwrap()
    }

    fn lit(l: &mut Layer, now: f64) -> bool {
        let mut frame = vec![BLACK; 2];
        l.paint(now, &mut frame, &mut Rng::new(1));
        frame.iter().any(|&c| c != BLACK)
    }

    #[test]
    fn led_on_the_index_of_its_cell() {
        assert_eq!(canvas().pos, vec![(80.0, 0.0), (160.0, 240.0)]);
    }

    #[test]
    fn center_in_editor_cells_minus_the_mouse_and_two_rows() {
        let dev = json!({"x": 4920, "y": 4840});
        // centre of the mouse chosen in Chroma Studio: measured column 3, row 2
        assert_eq!(center_point(&json!({"centerPointX": 126, "centerPointY": 125, "deviceWithCenterPoint": dev})), Some((120.0, 80.0)));
        assert_eq!(center_point(&json!({"centerPointX2": 5068, "centerPointY2": 4976, "deviceWithCenterPoint": dev})), Some((160.0, 40.0)));
        assert_eq!(center_point(&json!({"centerPointX": 127})), None);
    }

    /// All 11 LEDs of the Basilisk V3 35K, in the order of Chroma Studio's SVG.
    const ALL: [(i64, i64); 11] = [(0, 2), (5, 2), (1, 0), (2, 0), (3, 0), (4, 0), (6, 0), (6, 4), (4, 4), (2, 4), (0, 4)];

    /// Phases of the LEDs of a layer spread over the whole mouse.
    fn phases(name: &str, p: Value, lp: Value) -> Vec<f64> {
        let grid: HashMap<(i64, i64), usize> = ALL.iter().enumerate().map(|(n, &rc)| (rc, n)).collect();
        let canvas = Canvas::new(&grid, ALL.len());
        let l = Layer::new(name, (0..ALL.len()).collect(), &p, &lp, &canvas, &mut Rng::new(1)).unwrap();
        let Kind::Scroll { phase, .. } = l.kind else { unreachable!() };
        phase
    }

    /// Compares with the lags measured on Razer's engine (in periods, relative to LED 1,0), up to
    /// a whole cycle and with the tolerance of the 40 ms ticks.
    fn same_lags(phase: &[f64], measured: [f64; 11], tol: f64) {
        for n in 0..11 {
            let d = (phase[n] - phase[2] - measured[n]).rem_euclid(1.0);
            assert!(d.min(1.0 - d) < tol, "LED {:?}: phase {} instead of {}", ALL[n], (phase[n] - phase[2]).rem_euclid(1.0), measured[n]);
        }
    }

    fn center_lp(ccw: bool) -> Value {
        json!({"centerPointX": 126, "centerPointY": 125, "counterclockwise": ccw, "deviceWithCenterPoint": {"x": 4920, "y": 4840}})
    }

    #[test]
    fn geometry_measured_on_razer() {
        let st = json!([{"Stop": 0, "Color": 0xFF0000}, {"Stop": 100, "Color": 0x0000FF}]);
        let wave = |angle: f64, split: bool| phases("wave", json!({"speed": 15, "width": 100, "angle": angle, "split": split, "colorStops": st}), json!({}));
        same_lags(&wave(90.0, false), [0.388, 0.388, 0.0, 0.0, 0.0, 0.0, 0.0, 0.783, 0.783, 0.783, 0.783], 0.03);
        same_lags(&wave(0.0, false), [0.122, 0.415, 0.0, 0.856, 0.707, 0.563, 0.266, 0.266, 0.563, 0.856, 0.122], 0.03);
        same_lags(&wave(45.0, false), [0.366, 0.876, 0.0, 0.902, 0.805, 0.707, 0.510, 0.076, 0.244, 0.441, 0.659], 0.03);
        same_lags(&wave(90.0, true), [0.610, 0.610, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 0.03);
        let wheel = |ccw: bool| phases("wheel", json!({"speed": 60, "colorStops": st}), center_lp(ccw));
        same_lags(&wheel(false), [0.125, 0.747, 0.0, 0.947, 0.899, 0.855, 0.800, 0.660, 0.625, 0.448, 0.273], 0.02);
        let tidal = |angle: f64, split: bool, ccw: bool| {
            phases("tidal", json!({"speed": 15, "color": 0x00FF00, "color2": 0x0000FF, "angle": angle, "split": split}), center_lp(ccw))
        };
        same_lags(&tidal(160.0, true, false), [0.578, 0.815, 0.0, 0.048, 0.097, 0.143, 0.251, 0.493, 0.578, 0.671, 0.769], 0.03);
        same_lags(&tidal(160.0, true, true), [0.421, 0.180, 0.0, 0.948, 0.902, 0.851, 0.745, 0.505, 0.421, 0.324, 0.228], 0.03);
        same_lags(&tidal(90.0, false, false), [0.143, 0.288, 0.0, 0.866, 0.0, 0.143, 0.430, 0.430, 0.143, 0.866, 0.143], 0.03);
    }

    #[test]
    fn random_colors_like_razer() {
        let mut rng = Rng::new(7);
        for _ in 0..500 {
            let c = rng.color();
            assert!([c.0, c.1, c.2].iter().all(|v| [0, 128, 255].contains(v)) && [c.0, c.1, c.2].contains(&255), "{c:?}");
        }
    }

    #[test]
    fn stars_like_razer() {
        let grid: HashMap<(i64, i64), usize> = ALL.iter().enumerate().map(|(n, &rc)| (rc, n)).collect();
        let canvas = Canvas::new(&grid, 11);
        let p = json!({"density": 5, "duration": 1700, "colorStops": [{"Stop": 0, "Color": 0xFF0000}, {"Stop": 100, "Color": 0x0000FF}]});
        let mut l = Layer::new("starlight", (0..11).collect(), &p, &json!({}), &canvas, &mut Rng::new(3)).unwrap();
        let mut rng = Rng::new(9);
        let (mut most, mut lit_sum, mut frames) = (0, 0, 0);
        for k in 0..6000 {
            let mut frame = vec![BLACK; 11];
            l.paint(k as f64 / 60.0, &mut frame, &mut rng);
            let lit = frame.iter().filter(|&&c| c != BLACK).count();
            most = most.max(lit);
            lit_sum += lit;
            frames += 1;
        }
        let mean = lit_sum as f64 / frames as f64;
        // measured on Razer's engine with density 5: never more than 6 out of 11, 3.2 on average
        assert!(most <= 6, "maximum {most}");
        assert!((2.5..4.0).contains(&mean), "mean {mean}");
    }

    fn wave(extra: Value) -> Value {
        let mut p = json!({"speed": 25, "width": 100, "colorStops": [{"Stop": 0, "Color": 0xFF0000}, {"Stop": 100, "Color": 0xFF0000}]});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    }

    /// Wave over the whole mouse started by a click: which LEDs are lit at time `t`.
    fn wave_lit(l: &mut Layer, t: f64) -> Vec<bool> {
        let mut frame = vec![BLACK; ALL.len()];
        l.paint(t, &mut frame, &mut Rng::new(1));
        frame.iter().map(|&c| c != BLACK).collect()
    }

    fn wave_all(extra: Value) -> Layer {
        let grid: HashMap<(i64, i64), usize> = ALL.iter().enumerate().map(|(n, &rc)| (rc, n)).collect();
        let p = wave(json!({"angle": 90, "colorStops": [{"Stop": 0, "Color": 0xFF0000}, {"Stop": 50, "Color": 0x00FF00}, {"Stop": 100, "Color": 0x0000FF}]}));
        let mut p = p;
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        Layer::new("wave", (0..ALL.len()).collect(), &p, &json!({}), &Canvas::new(&grid, ALL.len()), &mut Rng::new(1)).unwrap()
    }

    #[test]
    fn after_n_cycles_each_led_finishes_its_cycle() {
        // measured with real clicks (speed 25, width 100, angle 90, 2 cycles): after the click the left
        // column stays lit ~1.05 s, the middle one ~1.45 s, the right one ~1.8 s
        let mut l = wave_all(json!({"playbackStart": "onPress", "playbackEnd": "after", "cycles": 2}));
        assert!(!wave_lit(&mut l, 5.0).iter().any(|&x| x), "nothing shows before the click");
        l.press(10.0);
        let (left, mid, right) = (2, 0, 10); // (1,0), (0,2), (0,4)
        let on = wave_lit(&mut l, 10.95);
        assert!(on[left] && on[mid] && on[right]);
        let on = wave_lit(&mut l, 11.15);
        assert!(!on[left] && on[mid] && on[right]);
        let on = wave_lit(&mut l, 11.55);
        assert!(!on[mid] && on[right]);
        assert!(!wave_lit(&mut l, 11.95).iter().any(|&x| x), "all off");
        l.press(20.0);
        assert!(wave_lit(&mut l, 20.01)[right], "a new click restarts it");
    }

    #[test]
    fn toggle_one_click_stops_it_another_restarts_it() {
        // measured: with toggle (1 cycle) the wave does its cycle and goes off by itself; a click while
        // it runs turns it off at once, a click while it is off restarts it
        let mut l = wave_all(json!({"playbackStart": "onPress", "playbackEnd": "toggle", "cycles": 1}));
        l.press(1.0);
        assert!(wave_lit(&mut l, 1.3)[10]);
        l.press(1.4);
        assert!(!wave_lit(&mut l, 1.45).iter().any(|&x| x), "a click while it runs turns it off");
        l.press(2.0);
        assert!(wave_lit(&mut l, 2.8)[10]);
        assert!(!wave_lit(&mut l, 2.95).iter().any(|&x| x), "once the cycle is over it goes off by itself");
        l.press(4.0);
        assert!(wave_lit(&mut l, 4.1)[10], "a click after the end restarts it");
    }

    #[test]
    fn wheel_after_n_cycles_goes_off_all_together_and_ignores_clicks() {
        // measured with real clicks (speed 180, 2 cycles): 4 s from the click, the clicks in between do not count
        let p = json!({"speed": 180, "colorStops": [{"Stop": 0, "Color": 0xFF0000}, {"Stop": 100, "Color": 0x0000FF}],
                       "playbackStart": "onPress", "playbackEnd": "after", "cycles": 2});
        let mut l = layer("wheel", p, json!({}));
        assert!(!lit(&mut l, 1.0));
        l.press(10.0);
        l.press(12.0);
        assert!(lit(&mut l, 13.99), "all lit until the end");
        assert!(!lit(&mut l, 14.01), "all off together, counting from the first click");
    }

    #[test]
    fn breathing_after_ignores_clicks_while_running() {
        // measured with real clicks (duration 6000, 2 cycles): 12 s from the first click
        let p = json!({"color": 0xFF0000, "color2": 0x0000FF, "duration": 6000, "playbackStart": "onPress", "playbackEnd": "after", "cycles": 2});
        let mut l = layer("breathing", p, json!({}));
        l.press(0.0);
        l.press(10.0);
        assert!(lit(&mut l, 10.5));
        assert!(!lit(&mut l, 12.5), "over at 12 s, the click at 10 s did not restart it");
    }

    #[test]
    fn on_selected_keys_starts_with_a_click() {
        let mut l = layer("wave", wave(json!({"playbackStart": "onSelectedKeys", "playbackEnd": "after", "cycles": 1})), json!({}));
        assert!(l.on_press && !lit(&mut l, 1.0));
    }

    #[test]
    fn random_never_stops() {
        let mut l = layer("wave", wave(json!({"playbackStart": "random", "playbackEnd": "never", "cycles": -1})), json!({}));
        assert!(lit(&mut l, 1000.0));
    }

    #[test]
    fn static_without_color_is_transparent() {
        let mut l = layer("static", json!({}), json!({}));
        let mut frame = vec![(1, 2, 3); 2];
        l.paint(0.0, &mut frame, &mut Rng::new(1));
        assert_eq!(frame, vec![(1, 2, 3); 2]);
        let mut l = layer("static", json!({"color2": 0x0000FF}), json!({}));
        l.paint(0.0, &mut frame, &mut Rng::new(1));
        assert_eq!(frame, vec![(0, 0, 255); 2]);
    }

    #[test]
    fn random_breathing_lasts_half_as_long() {
        let period = |p: Value| match layer("breathing", p, json!({})).kind {
            Kind::Breathing { period, .. } => period,
            _ => unreachable!(),
        };
        assert_eq!(period(json!({"duration": 14000})), 14.0);
        assert_eq!(period(json!({"duration": 14000, "randomColor": true})), 7.0); // one breath every 7 s
        assert_eq!(period(json!({})), 14.0);
    }

    fn period(l: &Layer) -> f64 {
        match &l.kind {
            Kind::Scroll { hz, .. } => 1.0 / hz.abs(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn periods_measured_on_chroma_studio() {
        let w = |speed: f64, width: f64| period(&layer("wave", wave(json!({"speed": speed, "width": width})), json!({})));
        for (speed, width, measured) in [(10.0, 100.0, 2.48), (15.0, 100.0, 1.64), (30.0, 100.0, 0.80), (45.0, 100.0, 0.52), (50.0, 100.0, 0.48), (10.0, 200.0, 4.96), (10.0, 50.0, 1.24)] {
            assert!((w(speed, width) - measured).abs() < 0.005, "wave {speed}/{width}: {} instead of {measured}", w(speed, width));
        }
        let tidal = layer("tidal", json!({"speed": 15, "color": 0xFF0000, "color2": 0xFF00FF}), json!({}));
        assert!((period(&tidal) - 3.336).abs() < 0.01);
        let wheel = layer("wheel", wave(json!({"speed": 60})), json!({}));
        assert!((period(&wheel) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn pause_turns_the_led_off_after_one_pass() {
        // speed 25 = 1 s per pass, then 2 s off (measured: 2.48 s + 2 s of pause = 4.48 s)
        let p = wave(json!({"pause": 2000}));
        let mut l = Layer::new("wave", vec![0], &p, &json!({}), &canvas(), &mut Rng::new(1)).unwrap();
        let lit_at: Vec<bool> = (0..30).map(|k| lit(&mut l, k as f64 * 0.1 + 0.05)).collect();
        assert_eq!(lit_at.iter().filter(|&&x| x).count(), 10, "lit 1 s every 3 s: {lit_at:?}");
        assert!(lit(&mut l, 3.05) == lit(&mut l, 0.05));
    }

    #[test]
    fn wave_spread_over_the_region() {
        // 4 rows (3..6) vertically: a wave at 100% covers the region, 1/4 of a period per row
        let canvas = Canvas::new(&HashMap::from([((3, 0), 0), ((4, 0), 1), ((6, 0), 2)]), 3);
        let p = wave(json!({"angle": 180}));
        let l = Layer::new("wave", vec![0, 1, 2], &p, &json!({}), &canvas, &mut Rng::new(1)).unwrap();
        let Kind::Scroll { phase, .. } = &l.kind else { unreachable!() };
        assert!(((phase[1] - phase[0]) - 0.25).abs() < 1e-9 && ((phase[2] - phase[1]) - 0.5).abs() < 1e-9, "{phase:?}");
    }

    #[test]
    fn tidal_black_is_transparent() {
        let mut l = layer("tidal", json!({"speed": 15, "color": 0x00FF00, "color2": 0x0000FF}), json!({}));
        let covered = (0..200).filter(|k| {
            let mut frame = vec![(9, 9, 9); 2];
            l.paint(*k as f64 * 0.02, &mut frame, &mut Rng::new(1));
            frame[0] != (9, 9, 9)
        }).count();
        assert!(covered > 0 && covered < 200, "the tidal must cover only part of the time: {covered}/200");
    }

    #[test]
    fn ripple_on_click_no_rings_before_the_click() {
        let p = json!({"speed": 10, "width": 300, "playbackStart": "onPress", "playbackEnd": "after", "cycles": 3,
                       "colorStops": [{"Stop": 0, "Color": 0xFF0000}, {"Stop": 100, "Color": 0xFF0000}]});
        let mut l = layer("ripple", p, json!({}));
        assert!(!lit(&mut l, 30.0));
        l.press(31.0);
        match &l.kind {
            Kind::Ripple { rings, .. } => assert_eq!(rings.len(), 3, "one ring per cycle"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn ripple_table_like_razer() {
        // entries read on the Chroma Studio preview (width 400 -> 10 entries, entries 2..7)
        let (r, g, b) = (rgb(0xFF0000), rgb(0x00FF00), rgb(0x0000FF));
        let t = ripple_table(&[(0.0, r), (100.0, b)], 10);
        assert_eq!(&t[2..8], &[(204, 0, 51), (178, 0, 76), (153, 0, 102), (127, 0, 127), (102, 0, 153), (76, 0, 178)]);
        let t = ripple_table(&[(0.0, r), (50.0, g), (100.0, b)], 10);
        assert_eq!(&t[2..7], &[(127, 127, 0), (63, 191, 0), (0, 255, 0), (0, 255, 0), (0, 204, 51)]);
        let t = ripple_table(&[(30.0, r), (70.0, b)], 10);
        assert_eq!(&t[2..8], &[(255, 0, 0), (255, 0, 0), (170, 0, 85), (85, 0, 170), (0, 0, 255), (0, 0, 255)]);
        // width 100 -> 7 entries: the stop at 50% falls on entry round(3.5) = 4
        let t = ripple_table(&[(0.0, r), (50.0, g), (100.0, b)], 7);
        assert_eq!(&t[2..5], &[(85, 170, 0), (0, 255, 0), (0, 255, 0)]);
    }

    #[test]
    fn ripple_period_like_razer() {
        // measured on the preview, in 40 ms ticks: (speed, width, ticks)
        for (speed, width, ticks) in [(1, 100, 552), (5, 300, 132), (7, 200, 86), (10, 400, 72), (13, 100, 44), (35, 400, 22), (45, 300, 16), (50, 300, 28)] {
            let p = json!({"speed": speed, "width": width, "colorStops": [{"Stop": 0, "Color": 0xFF0000}]});
            let l = layer("ripple", p, json!({}));
            let Kind::Ripple { period, .. } = l.kind else { unreachable!() };
            assert!((period / 0.04 - ticks as f64).abs() < 1e-6, "speed {speed} width {width}: {} ticks", period / 0.04);
        }
    }
}
