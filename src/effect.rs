//! Un effetto di Chroma Studio pronto da disegnare: i layer di `layers.rs` e gli Audio Meter
//! impilati come in Chroma Studio, e i lampi Reactive sopra a tutto.

use crate::config::Config;
use crate::layers::{gradient, lerp, rgb, stops, Canvas, Layer, Rgb, Rng, Stops, BLACK};
use serde_json::Value;
use std::collections::HashMap;

const REACTIVE_RANDOM: [u32; 6] = [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF];

// Audio Meter, misurato sull'anteprima di Chroma Studio con toni a volume noto (2026-09-19):
// tutti i LED della regione hanno lo stesso colore, quello del gradiente alla posizione
// livello = min(picco * boost, 1) (il picco e' quello di IAudioMeterInformation, lineare);
// il livello sale subito e scende in linea retta di 1/decay al secondo; a livello 0 il layer e'
// trasparente, sopra 0 il colore e' pieno (nessuna soglia: 0,001 accende gia').
// Auto boost (il cursore del boost e' ignorato), calcolato a scatti di 40 ms come il motore su
// `env`, il picco dello scatto seguito per 7/8 della distanza in salita e 1/2 in discesa: se
// `env` dello scatto prima supera 0,1 il boost e' min(4, 1 / massimo di `env`), altrimenti 1 e
// il massimo si azzera; 0,2 s di silenzio assoluto azzerano tutto.
const AUTO_TICK: f64 = 0.04;
const AUTO_ATTACK: f64 = 0.875;
const AUTO_RELEASE: f64 = 0.5;
const AUTO_GATE: f64 = 0.1;
const AUTO_MAX: f64 = 4.0;
const AUTO_SILENCE: f64 = 0.2;

/// Stato dell'auto boost di un Audio Meter.
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

    /// Il boost da usare adesso; `peak` e' il picco letto in questo frame.
    fn feed(&mut self, now: f64, peak: f64) -> f64 {
        self.tick_peak = self.tick_peak.max(peak);
        let next = *self.next_tick.get_or_insert(now);
        if now >= next {
            // dopo una pausa lunga conto uno scatto solo, ma il silenzio vale per intero
            let steps = ((now - next) / AUTO_TICK).floor() + 1.0;
            self.next_tick = Some(next + steps * AUTO_TICK);
            let p = std::mem::take(&mut self.tick_peak);
            self.quiet = if p == 0.0 { self.quiet + steps * AUTO_TICK } else { 0.0 };
            let open = self.gate; // conta l'env dello scatto prima
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

/// Un elemento della pila dei layer.
enum Item {
    Layer(Layer),
    Audio(Audio),
}

/// I layer nell'ordine di Chroma Studio, con quelli dentro i gruppi (`type: "group"`) al posto
/// del gruppo. Un gruppo nascosto nasconde tutti i suoi layer.
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
    color: Option<Rgb>, // None = colore a caso a ogni click
    duration: f64,
}

struct Flash {
    leds: Vec<usize>,
    color: Rgb,
    t0: f64,
    duration: f64,
}

pub struct Effect {
    pub title: String,
    num_leds: usize,
    stack: Vec<Item>, // dal basso verso l'alto
    reactive: Vec<Reactive>,
    flashes: Vec<Flash>,
    gain: f64,
    rng: Rng,
}

impl Effect {
    /// `spec`: l'effetto come sta nel dump; `grid`: (riga, colonna) di Chroma Studio -> LED fisico.
    /// Ritorna anche i nomi dei layer non supportati, da segnalare nel log.
    pub fn new(spec: &Value, cfg: &Config, grid: &HashMap<(i64, i64), usize>, canvas: &Canvas, seed: u64) -> (Effect, Vec<String>) {
        let mut fx = Effect {
            title: spec.get("title").and_then(Value::as_str).unwrap_or("(senza nome)").to_string(),
            num_leds: cfg.device.leds,
            stack: Vec::new(),
            reactive: Vec::new(),
            flashes: Vec::new(),
            gain: cfg.audio_gain,
            rng: Rng::new(seed),
        };
        let mut unsupported = Vec::new();
        let empty = Value::Object(Default::default());
        let mut layers = Vec::new();
        flatten(spec.get("layers").and_then(Value::as_array).map_or(&[], Vec::as_slice), &mut layers);
        // in Chroma Studio il primo layer della lista sta sopra: applico dal fondo
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
                            // autoBoost: true/false dall'editor, 0/1 dagli effetti rapidi
                            let auto = p.get("autoBoost").and_then(|v| v.as_bool().or(v.as_f64().map(|n| n != 0.0))).unwrap_or(false);
                            let auto = auto.then(AutoBoost::new);
                            fx.stack.push(Item::Audio(Audio { leds, stops: st, boost: num("boost", 1.0), auto, decay: num("decay", 1.0), level: 0.0 }));
                        }
                    }
                    "reactive" => {
                        let random = p.get("randomColor").and_then(Value::as_bool).unwrap_or(false);
                        let color = p.get("color").and_then(Value::as_u64).unwrap_or(0x00FF00) as u32;
                        let duration = p.get("duration").and_then(Value::as_f64).unwrap_or(1500.0) / 1000.0;
                        fx.reactive.push(Reactive { leds, color: if random { None } else { Some(rgb(color)) }, duration });
                    }
                    _ => match Layer::new(name, leds, p, lp, canvas, &mut fx.rng) {
                        Some(l) => fx.stack.push(Item::Layer(l)),
                        None => {
                            unsupported.push(name.to_string());
                            break; // un avviso per layer, non uno per regione
                        }
                    },
                }
            }
        }
        (fx, unsupported)
    }

    /// Un effetto spento, per i profili senza effetto abbinato.
    pub fn off(cfg: &Config, canvas: &Canvas) -> Effect {
        Effect::new(&serde_json::json!({"title": "(spento)", "layers": []}), cfg, &HashMap::new(), canvas, 1).0
    }

    pub fn has_audio(&self) -> bool {
        self.stack.iter().any(|i| matches!(i, Item::Audio(_)))
    }

    pub fn needs_clicks(&self) -> bool {
        !self.reactive.is_empty() || self.stack.iter().any(|i| matches!(i, Item::Layer(l) if l.on_press))
    }

    pub fn clear_flashes(&mut self) {
        self.flashes.clear();
    }

    pub fn on_click(&mut self, now: f64) {
        for item in self.stack.iter_mut() {
            if let Item::Layer(l) = item {
                if l.on_press {
                    l.press(now);
                }
            }
        }
        for r in &self.reactive {
            let color = r.color.unwrap_or_else(|| rgb(REACTIVE_RANDOM[(self.rng.f64() * 6.0) as usize % 6]));
            self.flashes.push(Flash { leds: r.leds.clone(), color, t0: now, duration: r.duration });
        }
    }

    /// `peak`: picco dell'uscita audio (0..1), letto dal chiamante solo se `has_audio()`.
    pub fn render(&mut self, now: f64, dt: f64, peak: f64) -> Vec<Rgb> {
        let mut frame = vec![BLACK; self.num_leds];
        let peak = peak * self.gain;
        for item in self.stack.iter_mut() {
            match item {
                Item::Layer(l) => l.paint(now, &mut frame, &mut self.rng),
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
        self.flashes.retain(|f| now - f.t0 < f.duration);
        for f in &self.flashes {
            let k = 1.0 - (now - f.t0) / f.duration;
            for &i in &f.leds {
                frame[i] = lerp(frame[i], f.color, k);
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
    fn i_layer_dentro_un_gruppo_si_vedono() {
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

    /// Suona `secs` secondi a picco `peak` a 60 frame/s e ritorna il livello alla fine.
    fn play(fx: &mut Effect, t: &mut f64, peak: f64, secs: f64) -> f64 {
        for _ in 0..(secs * 60.0).round() as usize {
            *t += 1.0 / 60.0;
            fx.render(*t, 1.0 / 60.0, peak);
        }
        level(fx)
    }

    #[test]
    fn livello_uguale_al_picco_per_il_boost() {
        let mut t = 0.0;
        assert!((play(&mut meter(json!({})), &mut t, 0.3, 0.5) - 0.3).abs() < 1e-9);
        assert!((play(&mut meter(json!({"boost": 2})), &mut t, 0.3, 0.5) - 0.6).abs() < 1e-9);
        assert!((play(&mut meter(json!({"boost": 4})), &mut t, 0.001, 0.5) - 0.004).abs() < 1e-9); // nessuna soglia
        assert_eq!(play(&mut meter(json!({"boost": 4})), &mut t, 0.3, 0.5), 1.0);
    }

    #[test]
    fn a_livello_zero_si_vede_il_layer_sotto() {
        let blue = json!({"name": "static", "visible": true, "regions": region(json!({"color": 0x0000FF}))});
        let stops = json!([{"Stop": 0, "Color": 0x00FF00}, {"Stop": 100, "Color": 0xFF0000}]);
        let audio = json!({"name": "audio", "visible": true, "regions": region(json!({"boost": 1, "decay": 0.5, "colorStops": stops}))});
        let mut fx = effect(json!({"layers": [audio, blue]}));
        assert_eq!(fx.render(0.0, 0.0, 0.5)[0], (127, 127, 0));
        assert_eq!(fx.render(0.1, 0.1, 0.0)[0], (76, 178, 0)); // 0,5 - 0,1 / 0,5 = 0,3
        assert_eq!(fx.render(0.4, 0.3, 0.0)[0], (0, 0, 255)); // arrivato a 0: trasparente
    }

    #[test]
    fn decay_di_default_un_secondo() {
        let mut fx = meter(json!({}));
        fx.render(0.0, 0.0, 1.0);
        fx.render(0.5, 0.5, 0.0);
        assert!((level(&fx) - 0.5).abs() < 1e-9, "livello {}", level(&fx));
    }

    #[test]
    fn auto_boost_come_misurato_su_chroma_studio() {
        let mut fx = meter(json!({"autoBoost": true, "boost": 4, "decay": 0.1}));
        let mut t = 0.0;
        // sotto 0,1 niente boost (e il cursore del boost e' ignorato)
        assert!((play(&mut fx, &mut t, 0.08, 1.0) - 0.08).abs() < 1e-9);
        // appena sopra 0,1: boost 4
        assert!((play(&mut fx, &mut t, 0.102, 1.0) - 0.408).abs() < 1e-9);
        // dopo 0,5 il riferimento resta 0,5 finche' il suono non scende sotto 0,1: 0,2 -> 0,4
        assert_eq!(play(&mut fx, &mut t, 0.5, 1.0), 1.0);
        assert!((play(&mut fx, &mut t, 0.2, 25.0) - 0.4).abs() < 1e-9);
        // 0,15 s di silenzio non azzerano il riferimento, 0,3 s si'
        play(&mut fx, &mut t, 0.5, 1.0);
        play(&mut fx, &mut t, 0.0, 0.15);
        assert!((play(&mut fx, &mut t, 0.2, 1.0) - 0.4).abs() < 1e-9);
        play(&mut fx, &mut t, 0.5, 1.0);
        play(&mut fx, &mut t, 0.0, 0.3);
        assert!((play(&mut fx, &mut t, 0.2, 1.0) - 0.8).abs() < 1e-9);
        // scendere sotto 0,1 azzera il riferimento anche senza silenzio
        play(&mut fx, &mut t, 0.5, 1.0);
        play(&mut fx, &mut t, 0.08, 1.0);
        assert!((play(&mut fx, &mut t, 0.2, 1.0) - 0.8).abs() < 1e-9);
        // un picco a 1,0 per uno scatto porta il riferimento a 0,2 + 0,8 * 7/8 = 0,9
        play(&mut fx, &mut t, 0.0, 1.0);
        play(&mut fx, &mut t, 0.2, 1.0);
        play(&mut fx, &mut t, 1.0, 1.0 / 60.0);
        assert!((play(&mut fx, &mut t, 0.2, 1.0) - 0.2 / 0.9).abs() < 1e-9);
    }
}
