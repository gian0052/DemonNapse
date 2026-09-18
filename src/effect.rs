//! Un effetto di Chroma Studio pronto da disegnare: il fondo (layer di `layers.rs`), l'Audio
//! Meter che quando c'e' musica comanda da solo, e i lampi Reactive sopra a tutto.

use crate::config::Config;
use crate::layers::{gradient, lerp, rgb, stops, Canvas, Layer, Rgb, Rng, Stops, BLACK};
use serde_json::Value;
use std::collections::HashMap;

const REACTIVE_RANDOM: [u32; 6] = [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF];

struct Audio {
    leds: Vec<usize>,
    stops: Stops,
    boost: f64,
    decay: f64,
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
            last_audio: -1e9,
            silence_floor: cfg.audio_silence_floor,
            gain: cfg.audio_gain,
            idle_delay: cfg.audio_idle_delay,
            rng: Rng::new(seed),
        };
        let mut unsupported = Vec::new();
        let empty = Value::Object(Default::default());
        let layers = spec.get("layers").and_then(Value::as_array).cloned().unwrap_or_default();
        // in Chroma Studio il primo layer della lista sta sopra: applico dal fondo
        for layer in layers.iter().rev() {
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
                            fx.audio.push(Audio { leds, stops: st, boost: num("boost", 1.0), decay: num("decay", 0.5) });
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
        for a in &self.audio {
            let p = (peak * a.boost * self.gain).min(1.0);
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
