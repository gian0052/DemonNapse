//! A Chroma Studio effect ready to draw: the layers of `layers.rs`, the Audio Meters, the
//! Ambients and the Reactives stacked as in Chroma Studio (the first in the list is on top).

use crate::ambient::{self, Ambient};
use crate::config::Config;
use crate::layers::{gradient, lerp, rgb, stops, Canvas, Layer, Rgb, Rng, Stops, BLACK};
use serde_json::Value;
use crate::screen::Screen;
use std::collections::HashMap;

// Reactive, measured with real clicks on the Chroma Studio preview (2026-09-19): the flash starts
// with the click, at full brightness, and falls in a straight line to black in (floor(duration / 40 ms) - 1) ticks
// of 40 ms (1500 ms -> 1.44 s, 500 ms -> 0.44 s); while it lasts it covers the layers below (also while
// it fades out: nothing shows underneath), once over it is transparent again. A click during the flash makes
// it start over (also with a new colour). Random colours: each channel 0, 128 or 255, never black.
const REACTIVE_TICK: f64 = 0.04;

// Audio Meter, measured on the Chroma Studio preview with tones at a known volume (2026-09-19):
// all the LEDs of the region have the same colour, the gradient's colour at position
// level = min(peak * boost, 1) (the peak is IAudioMeterInformation's, linear);
// the level rises at once and falls in a straight line by 1/decay per second; at level 0 the layer is
// transparent, above 0 the colour is full (no threshold: 0.001 already lights it).
// Auto boost (the boost slider is ignored), computed in 40 ms ticks like the engine, on `env`,
// which is the tick's peak followed for 7/8 of the distance when rising and 1/2 when falling: if
// the previous tick's `env` exceeds 0.1 the boost is min(4, 1 / maximum of `env`), otherwise 1 and
// the maximum is reset; 0.2 s of absolute silence reset everything.
const AUTO_TICK: f64 = 0.04;
const AUTO_ATTACK: f64 = 0.875;
const AUTO_RELEASE: f64 = 0.5;
const AUTO_GATE: f64 = 0.1;
const AUTO_MAX: f64 = 4.0;
const AUTO_SILENCE: f64 = 0.2;

/// State of an Audio Meter's auto boost.
struct AutoBoost {
    next_tick: Option<f64>,
    tick_peak: f64,
    env: f64,
    gate: bool,
    top: f64,
    quiet: f64,
    gain: f64,
}

impl AutoBoost {
    fn new() -> AutoBoost {
        AutoBoost { next_tick: None, tick_peak: 0.0, env: 0.0, gate: false, top: 0.0, quiet: 0.0, gain: 1.0 }
    }

    /// The boost to use now; `peak` is the peak read in this frame.
    fn feed(&mut self, now: f64, peak: f64) -> f64 {
        self.tick_peak = self.tick_peak.max(peak);
        let next = *self.next_tick.get_or_insert(now);
        if now >= next {
            // after a long pause I count one tick only, but the silence counts in full
            let steps = ((now - next) / AUTO_TICK).floor() + 1.0;
            self.next_tick = Some(next + steps * AUTO_TICK);
            let p = std::mem::take(&mut self.tick_peak);
            self.quiet = if p == 0.0 { self.quiet + steps * AUTO_TICK } else { 0.0 };
            let open = self.gate; // what counts is the previous tick's env
            if self.quiet >= AUTO_SILENCE - 1e-9 {
                self.env = 0.0;
            } else if p > 0.0 {
                self.env += (p - self.env) * if p > self.env { AUTO_ATTACK } else { AUTO_RELEASE };
            }
            self.top = if open { self.top.max(self.env) } else { 0.0 };
            self.gain = if open && self.top > 0.0 { (1.0 / self.top).min(AUTO_MAX) } else { 1.0 };
            self.gate = self.env > AUTO_GATE;
        }
        self.gain
    }
}

struct Audio {
    leds: Vec<usize>,
    stops: Stops,
    boost: f64,
    auto: Option<AutoBoost>,
    decay: f64,
    level: f64,
}

/// One item of the layer stack.
enum Item {
    Layer(Layer),
    Audio(Audio),
    Ambient(Ambient),
    Reactive(Reactive),
}

/// The layers in Chroma Studio's order, with those inside groups (`type: "group"`) in place
/// of the group. A hidden group hides all its layers.
fn flatten<'a>(layers: &'a [Value], out: &mut Vec<&'a Value>) {
    for layer in layers {
        if layer.get("type").and_then(Value::as_str) == Some("group") {
            if layer.get("visible").and_then(Value::as_bool).unwrap_or(true) {
                flatten(layer.get("layers").and_then(Value::as_array).map_or(&[], Vec::as_slice), out);
            }
        } else {
            out.push(layer);
        }
    }
}

struct Reactive {
    leds: Vec<usize>,
    color: Option<Rgb>, // None = random colour at every click
    duration: f64,      // s, already reduced to the engine's ticks
    flash: Option<(Rgb, f64)>, // flash in progress: colour and time of the click
}

pub struct Effect {
    pub title: String,
    num_leds: usize,
    stack: Vec<Item>, // from bottom to top
    gain: f64,
    rng: Rng,
    screen: Option<Screen>, // opened only if the effect has an Ambient
    next_capture: f64,
}

impl Effect {
    /// `spec`: the effect as it is in the dump; `grid`: Chroma Studio (row, column) -> physical LED.
    /// Also returns the names of the unsupported layers, to be reported in the log.
    pub fn new(spec: &Value, cfg: &Config, grid: &HashMap<(i64, i64), usize>, canvas: &Canvas, seed: u64) -> (Effect, Vec<String>) {
        let mut fx = Effect {
            title: spec.get("title").and_then(Value::as_str).unwrap_or("(unnamed)").to_string(),
            num_leds: cfg.device.leds,
            stack: Vec::new(),
            gain: cfg.audio_gain,
            rng: Rng::new(seed),
            screen: None,
            next_capture: 0.0,
        };
        let mut unsupported = Vec::new();
        let empty = Value::Object(Default::default());
        let mut layers = Vec::new();
        flatten(spec.get("layers").and_then(Value::as_array).map_or(&[], Vec::as_slice), &mut layers);
        // in Chroma Studio the first layer in the list is on top: I apply from the bottom
        for layer in layers.into_iter().rev() {
            if !layer.get("visible").and_then(Value::as_bool).unwrap_or(true) {
                continue;
            }
            let name = layer.get("name").and_then(Value::as_str).unwrap_or("");
            let lp = layer.get("params").unwrap_or(&empty);
            for reg in layer.get("regions").and_then(Value::as_array).into_iter().flatten() {
                let leds: Vec<usize> = reg
                    .get("devices")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .flat_map(|d| d.get("leds").and_then(Value::as_array).into_iter().flatten())
                    .filter_map(|l| grid.get(&(l.get("row")?.as_i64()?, l.get("col")?.as_i64()?)).copied())
                    .collect();
                let p = reg.get("params").unwrap_or(&empty);
                match name {
                    "audio" => {
                        if let Some(st) = stops(p) {
                            let num = |k: &str, d: f64| p.get(k).and_then(Value::as_f64).unwrap_or(d);
                            // autoBoost: true/false from the editor, 0/1 from the quick effects
                            let auto = p.get("autoBoost").and_then(|v| v.as_bool().or(v.as_f64().map(|n| n != 0.0))).unwrap_or(false);
                            let auto = auto.then(AutoBoost::new);
                            fx.stack.push(Item::Audio(Audio { leds, stops: st, boost: num("boost", 1.0), auto, decay: num("decay", 1.0), level: 0.0 }));
                        }
                    }
                    "ambient" => fx.stack.push(Item::Ambient(Ambient::new(leds, p, canvas))),
                    "reactive" => {
                        let random = p.get("randomColor").and_then(Value::as_bool).unwrap_or(false);
                        let color = p.get("color").and_then(Value::as_u64).unwrap_or(0x00FF00) as u32;
                        let ms = p.get("duration").and_then(Value::as_f64).unwrap_or(1500.0);
                        let duration = ((ms / 1000.0 / REACTIVE_TICK + 1e-9).floor() - 1.0).max(1.0) * REACTIVE_TICK;
                        let color = if random { None } else { Some(rgb(color)) };
                        fx.stack.push(Item::Reactive(Reactive { leds, color, duration, flash: None }));
                    }
                    _ => match Layer::new(name, leds, p, lp, canvas, &mut fx.rng) {
                        Some(l) => fx.stack.push(Item::Layer(l)),
                        None => {
                            unsupported.push(name.to_string());
                            break; // one warning per layer, not one per region
                        }
                    },
                }
            }
        }
        (fx, unsupported)
    }

    /// An effect that is off, for profiles with no matching effect.
    pub fn off(cfg: &Config, canvas: &Canvas) -> Effect {
        Effect::new(&serde_json::json!({"title": "(off)", "layers": []}), cfg, &HashMap::new(), canvas, 1).0
    }

    pub fn has_audio(&self) -> bool {
        self.stack.iter().any(|i| matches!(i, Item::Audio(_)))
    }

    pub fn needs_clicks(&self) -> bool {
        self.stack.iter().any(|i| matches!(i, Item::Reactive(_)) || matches!(i, Item::Layer(l) if l.on_press))
    }

    pub fn clear_flashes(&mut self) {
        for item in self.stack.iter_mut() {
            if let Item::Reactive(r) = item {
                r.flash = None;
            }
        }
    }

    /// The effect is no longer the active one: I close the screen reading (Ambient).
    pub fn sleep(&mut self) {
        self.screen = None;
        self.next_capture = 0.0;
    }

    pub fn on_click(&mut self, now: f64) {
        for item in self.stack.iter_mut() {
            match item {
                Item::Layer(l) if l.on_press => l.press(now),
                Item::Reactive(r) => r.flash = Some((r.color.unwrap_or_else(|| self.rng.any_color()), now)),
                _ => {}
            }
        }
    }

    /// Ambient: reads the screen every 40 ms like Razer's engine. Without an Ambient it opens nothing.
    fn capture(&mut self, now: f64) {
        if now < self.next_capture || !self.stack.iter().any(|i| matches!(i, Item::Ambient(a) if a.points(1, 1).is_some())) {
            return;
        }
        self.next_capture = if now - self.next_capture > ambient::TICK { now + ambient::TICK } else { self.next_capture + ambient::TICK };
        if self.screen.is_none() {
            self.screen = Screen::open();
        }
        let Some(screen) = self.screen.as_mut() else { return };
        let (w, h) = screen.size();
        let requests: Vec<_> = self.stack.iter().filter_map(|i| if let Item::Ambient(a) = i { a.points(w, h) } else { None }).collect();
        // None = screen unchanged: the colours stay as they were
        let Some(samples) = screen.capture(&requests) else { return };
        let mut it = samples.iter();
        for item in self.stack.iter_mut() {
            if let Item::Ambient(a) = item {
                if a.points(w, h).is_some() {
                    a.update(it.next().expect("one reading per Ambient"));
                }
            }
        }
    }

    /// `peak`: peak of the audio output (0..1), read by the caller only if `has_audio()`.
    pub fn render(&mut self, now: f64, dt: f64, peak: f64) -> Vec<Rgb> {
        self.capture(now);
        let mut frame = vec![BLACK; self.num_leds];
        let peak = peak * self.gain;
        for item in self.stack.iter_mut() {
            match item {
                Item::Layer(l) => l.paint(now, &mut frame, &mut self.rng),
                Item::Ambient(a) => a.paint(&mut frame),
                Item::Reactive(r) => {
                    if let Some((c, t0)) = r.flash {
                        let k = 1.0 - (now - t0) / r.duration;
                        if k > 0.0 {
                            let c = lerp(BLACK, c, k.min(1.0));
                            for &i in &r.leds {
                                frame[i] = c;
                            }
                        } else {
                            r.flash = None;
                        }
                    }
                }
                Item::Audio(a) => {
                    let boost = match &mut a.auto {
                        Some(auto) => auto.feed(now, peak),
                        None => a.boost,
                    };
                    a.level = (peak * boost).min(1.0).max(a.level - dt / a.decay.max(0.05)).max(0.0);
                    if a.level > 0.0 {
                        let c = gradient(&a.stops, a.level);
                        for &i in &a.leds {
                            frame[i] = c;
                        }
                    }
                }
            }
        }
        frame

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> Config {
        serde_json::from_value(json!({"device": {"pid": "0x00CB", "leds": 2, "chroma_grid": {"0,0": 0, "1,0": 1}}})).unwrap()
    }

    fn effect(spec: Value) -> Effect {
        let cfg = cfg();
        let grid = cfg.grid();
        Effect::new(&spec, &cfg, &grid, &Canvas::new(&grid, 2), 1).0
    }

    fn region(params: Value) -> Value {
        json!([{"params": params, "devices": [{"leds": [{"row": 0, "col": 0}, {"row": 1, "col": 0}]}]}])
    }

    #[test]
    fn layers_inside_a_group_are_shown() {
        let red = json!({"name": "static", "visible": true, "regions": region(json!({"color": 0xFF0000}))});
        let mut fx = effect(json!({"layers": [{"type": "group", "visible": true, "layers": [red.clone()]}]}));
        assert_eq!(fx.render(0.0, 0.016, 0.0), vec![(255, 0, 0); 2]);
        let mut hidden = effect(json!({"layers": [{"type": "group", "visible": false, "layers": [red]}]}));
        assert_eq!(hidden.render(0.0, 0.016, 0.0), vec![BLACK; 2]);
    }

    fn meter(extra: Value) -> Effect {
        let mut p = json!({"boost": 1, "colorStops": [{"Stop": 0, "Color": 0x00FF00}, {"Stop": 100, "Color": 0xFF0000}]});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        effect(json!({"layers": [{"name": "audio", "visible": true, "regions": region(p)}]}))
    }

    fn level(fx: &Effect) -> f64 {
        fx.stack.iter().find_map(|i| if let Item::Audio(a) = i { Some(a.level) } else { None }).unwrap()
    }

    /// Plays `secs` seconds at peak `peak` at 60 frames/s and returns the level at the end.
    fn play(fx: &mut Effect, t: &mut f64, peak: f64, secs: f64) -> f64 {
        for _ in 0..(secs * 60.0).round() as usize {
            *t += 1.0 / 60.0;
            fx.render(*t, 1.0 / 60.0, peak);
        }
        level(fx)
    }

    #[test]
    fn level_equals_peak_times_boost() {
        let mut t = 0.0;
        assert!((play(&mut meter(json!({})), &mut t, 0.3, 0.5) - 0.3).abs() < 1e-9);
        assert!((play(&mut meter(json!({"boost": 2})), &mut t, 0.3, 0.5) - 0.6).abs() < 1e-9);
        assert!((play(&mut meter(json!({"boost": 4})), &mut t, 0.001, 0.5) - 0.004).abs() < 1e-9); // no threshold
        assert_eq!(play(&mut meter(json!({"boost": 4})), &mut t, 0.3, 0.5), 1.0);
    }

    #[test]
    fn at_level_zero_the_layer_below_shows() {
        let blue = json!({"name": "static", "visible": true, "regions": region(json!({"color": 0x0000FF}))});
        let stops = json!([{"Stop": 0, "Color": 0x00FF00}, {"Stop": 100, "Color": 0xFF0000}]);
        let audio = json!({"name": "audio", "visible": true, "regions": region(json!({"boost": 1, "decay": 0.5, "colorStops": stops}))});
        let mut fx = effect(json!({"layers": [audio, blue]}));
        assert_eq!(fx.render(0.0, 0.0, 0.5)[0], (127, 127, 0));
        assert_eq!(fx.render(0.1, 0.1, 0.0)[0], (76, 178, 0)); // 0.5 - 0.1 / 0.5 = 0.3
        assert_eq!(fx.render(0.4, 0.3, 0.0)[0], (0, 0, 255)); // reached 0: transparent
    }

    #[test]
    fn default_decay_is_one_second() {
        let mut fx = meter(json!({}));
        fx.render(0.0, 0.0, 1.0);
        fx.render(0.5, 0.5, 0.0);
        assert!((level(&fx) - 0.5).abs() < 1e-9, "level {}", level(&fx));
    }

    #[test]
    fn auto_boost_as_measured_on_chroma_studio() {
        let mut fx = meter(json!({"autoBoost": true, "boost": 4, "decay": 0.1}));
        let mut t = 0.0;
        // below 0.1 no boost (and the boost slider is ignored)
        assert!((play(&mut fx, &mut t, 0.08, 1.0) - 0.08).abs() < 1e-9);
        // just above 0.1: boost 4
        assert!((play(&mut fx, &mut t, 0.102, 1.0) - 0.408).abs() < 1e-9);
        // after 0.5 the reference stays 0.5 until the sound falls below 0.1: 0.2 -> 0.4
        assert_eq!(play(&mut fx, &mut t, 0.5, 1.0), 1.0);
        assert!((play(&mut fx, &mut t, 0.2, 25.0) - 0.4).abs() < 1e-9);
        // 0.15 s of silence do not reset the reference, 0.3 s do
        play(&mut fx, &mut t, 0.5, 1.0);
        play(&mut fx, &mut t, 0.0, 0.15);
        assert!((play(&mut fx, &mut t, 0.2, 1.0) - 0.4).abs() < 1e-9);
        play(&mut fx, &mut t, 0.5, 1.0);
        play(&mut fx, &mut t, 0.0, 0.3);
        assert!((play(&mut fx, &mut t, 0.2, 1.0) - 0.8).abs() < 1e-9);
        // falling below 0.1 resets the reference even without silence
        play(&mut fx, &mut t, 0.5, 1.0);
        play(&mut fx, &mut t, 0.08, 1.0);
        assert!((play(&mut fx, &mut t, 0.2, 1.0) - 0.8).abs() < 1e-9);
        // a peak at 1.0 for one tick brings the reference to 0.2 + 0.8 * 7/8 = 0.9
        play(&mut fx, &mut t, 0.0, 1.0);
        play(&mut fx, &mut t, 0.2, 1.0);
        play(&mut fx, &mut t, 1.0, 1.0 / 60.0);
        assert!((play(&mut fx, &mut t, 0.2, 1.0) - 0.2 / 0.9).abs() < 1e-9);
    }

    fn reactive(params: Value) -> Value {
        json!({"name": "reactive", "visible": true, "regions": region(params)})
    }

    #[test]
    fn reactive_flash_as_measured() {
        // 1500 ms -> 36 ticks of 40 ms: 255, 247, 240 ... 7, then off (measured with real clicks)
        let mut fx = effect(json!({"layers": [reactive(json!({"color": 0xFF0000, "duration": 1500}))]}));
        fx.on_click(1.0);
        assert_eq!(fx.render(1.0, 0.0, 0.0)[0], (255, 0, 0));
        assert_eq!(fx.render(1.04, 0.0, 0.0)[0].0, 247); // like Razer's engine
        assert_eq!(fx.render(2.44, 0.0, 0.0)[0], BLACK);
        // a click during the flash makes it start over
        fx.on_click(3.0);
        fx.on_click(3.5);
        assert_eq!(fx.render(3.5, 0.0, 0.0)[0], (255, 0, 0));
    }

    #[test]
    fn reactive_respects_the_layer_order() {
        let blue = json!({"name": "static", "visible": true, "regions": region(json!({"color": 0x0000FF}))});
        let red = reactive(json!({"color": 0xFF0000, "duration": 1500}));
        // reactive on top: while it fades out it covers the static (measured: 226,0,0 and no blue)
        let mut fx = effect(json!({"layers": [red.clone(), blue.clone()]}));
        fx.on_click(0.0);
        assert_eq!(fx.render(0.72, 0.0, 0.0)[0], (127, 0, 0));
        assert_eq!(fx.render(1.5, 0.0, 0.0)[0], (0, 0, 255), "once the flash is over the static is back");
        // reactive below a static: it does not show
        let mut fx = effect(json!({"layers": [blue, red]}));
        fx.on_click(0.0);
        assert_eq!(fx.render(0.1, 0.0, 0.0)[0], (0, 0, 255));
    }

    #[test]
    fn reactive_random_colors_never_black() {
        let mut fx = effect(json!({"layers": [reactive(json!({"randomColor": true, "duration": 500}))]}));
        for k in 0..200 {
            fx.on_click(k as f64);
            let c = fx.render(k as f64, 0.0, 0.0)[0];
            assert!(c != BLACK && [c.0, c.1, c.2].iter().all(|v| [0, 128, 255].contains(v)), "{c:?}");
        }
    }
}
