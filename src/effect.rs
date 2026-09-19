//! Un effetto di Chroma Studio pronto da disegnare: il fondo (layer di `layers.rs`), l'Audio
//! Meter che quando c'e' musica comanda da solo, e i lampi Reactive sopra a tutto.

use crate::config::Config;
use crate::layers::{gradient, lerp, rgb, stops, Canvas, Layer, Rgb, Rng, Stops, BLACK};
use serde_json::Value;
use std::collections::HashMap;

const REACTIVE_RANDOM: [u32; 6] = [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF];
const AUTO_BOOST_MAX: f64 = 4.0; // il massimo del cursore "boost" di Chroma Studio
const AUTO_BOOST_HOLD: f64 = 5.0; // s: il picco di riferimento dell'auto boost scende a zero in questo tempo

struct Audio {
    leds: Vec<usize>,
    stops: Stops,
    boost: f64,
    auto_boost: bool,
    decay: f64,
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
    base: Vec<Layer>, // fondo, dal basso verso l'alto
    audio: Vec<Audio>,
    reactive: Vec<Reactive>,
    flashes: Vec<Flash>,
    level: f64,
    reference: f64,  // auto boost: picco recente, che il boost porta verso il massimo
    last_audio: f64, // ultimo istante con audio sopra la soglia
    silence_floor: f64,
    gain: f64,
    idle_delay: f64,
    rng: Rng,
}

impl Effect {
    /// `spec`: l'effetto come sta nel dump; `grid`: (riga, colonna) di Chroma Studio -> LED fisico.
    /// Ritorna anche i nomi dei layer non supportati, da segnalare nel log.
    pub fn new(spec: &Value, cfg: &Config, grid: &HashMap<(i64, i64), usize>, canvas: &Canvas, seed: u64) -> (Effect, Vec<String>) {
        let mut fx = Effect {
            title: spec.get("title").and_then(Value::as_str).unwrap_or("(senza nome)").to_string(),
            num_leds: cfg.device.leds,
            base: Vec::new(),
            audio: Vec::new(),
            reactive: Vec::new(),
            flashes: Vec::new(),
            level: 0.0,
            reference: 0.0,
            last_audio: -1e9,
            silence_floor: cfg.audio_silence_floor,
            gain: cfg.audio_gain,
            idle_delay: cfg.audio_idle_delay,
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
                            let auto_boost = p.get("autoBoost").and_then(Value::as_bool).unwrap_or(false);
                            fx.audio.push(Audio { leds, stops: st, boost: num("boost", 1.0), auto_boost, decay: num("decay", 1.0) });
                        }
                    }
                    "reactive" => {
                        let random = p.get("randomColor").and_then(Value::as_bool).unwrap_or(false);
                        let color = p.get("color").and_then(Value::as_u64).unwrap_or(0x00FF00) as u32;
                        let duration = p.get("duration").and_then(Value::as_f64).unwrap_or(1500.0) / 1000.0;
                        fx.reactive.push(Reactive { leds, color: if random { None } else { Some(rgb(color)) }, duration });
                    }
                    _ => match Layer::new(name, leds, p, lp, canvas, &mut fx.rng) {
                        Some(l) => fx.base.push(l),
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
        !self.audio.is_empty()
    }

    pub fn needs_clicks(&self) -> bool {
        !self.reactive.is_empty() || self.base.iter().any(|l| l.on_press)
    }

    pub fn clear_flashes(&mut self) {
        self.flashes.clear();
    }

    pub fn on_click(&mut self, now: f64) {
        for l in self.base.iter_mut().filter(|l| l.on_press) {
            l.press(now);
        }
        for r in &self.reactive {
            let color = r.color.unwrap_or_else(|| rgb(REACTIVE_RANDOM[(self.rng.f64() * 6.0) as usize % 6]));
            self.flashes.push(Flash { leds: r.leds.clone(), color, t0: now, duration: r.duration });
        }
    }

    /// True se l'audio tace da abbastanza: tocca al fondo e ai lampi.
    pub fn idle(&self, now: f64) -> bool {
        self.audio.is_empty() || now - self.last_audio > self.idle_delay
    }

    /// `peak`: picco dell'uscita audio (0..1), letto dal chiamante solo se `has_audio()`.
    pub fn render(&mut self, now: f64, dt: f64, peak: f64) -> Vec<Rgb> {
        let floor = self.silence_floor;
        // auto boost: il riferimento segue i picchi e poi scende piano, il boost porta il
        // riferimento verso il fondo scala (stima: Razer non documenta come lo calcola)
        self.reference = (self.reference - dt / AUTO_BOOST_HOLD).max(peak);
        for a in &self.audio {
            let boost = if a.auto_boost { (1.0 / self.reference.max(1.0 / AUTO_BOOST_MAX)).min(AUTO_BOOST_MAX) } else { a.boost };
            let p = (peak * boost * self.gain).min(1.0);
            self.level = p.max(self.level - dt / a.decay.max(0.05));
            if self.level > floor {
                self.last_audio = now;
            }
        }
        let mut frame = vec![BLACK; self.num_leds];
        if !self.idle(now) {
            // l'audio comanda da solo: base spenta, niente lampi
            self.flashes.clear();
            for a in &self.audio {
                if self.level > floor {
                    let c = lerp(BLACK, gradient(&a.stops, self.level), ((self.level - floor) / 0.06).min(1.0)); // accensione morbida
                    for &i in &a.leds {
                        frame[i] = c;
                    }
                }
            }
            return frame;
        }
        for l in self.base.iter_mut() {
            l.paint(now, &mut frame, &mut self.rng);
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

    #[test]
    fn auto_boost_porta_un_suono_basso_verso_il_fondo_scala() {
        let (mut fixed, mut auto) = (meter(json!({})), meter(json!({"autoBoost": true})));
        let mut t = 0.0;
        for _ in 0..120 {
            t += 1.0 / 60.0;
            fixed.render(t, 1.0 / 60.0, 0.25);
            auto.render(t, 1.0 / 60.0, 0.25);
        }
        assert!((fixed.level - 0.25).abs() < 1e-9);
        assert!(auto.level > 0.95, "auto boost: livello {}", auto.level);
    }

    #[test]
    fn decay_di_default_un_secondo() {
        let mut fx = meter(json!({}));
        fx.render(0.0, 0.0, 1.0);
        fx.render(0.5, 0.5, 0.0);
        assert!((fx.level - 0.5).abs() < 1e-9, "livello {}", fx.level);
    }
}
