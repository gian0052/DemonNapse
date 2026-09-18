//! Rendering dei layer di Chroma Studio, con i parametri come li scrive Synapse nei log.
//!
//! Ogni layer dipinge sui propri LED sopra quello che c'e' gia' (alpha < 1 = lascia vedere i
//! layer sotto). Audio Meter e Reactive stanno in `effect.rs` perche' dipendono dal meter e
//! dai click.
//!
//! La forma di ogni effetto segue il modo in cui Chroma Studio lo descrive al suo motore
//! (gradiente, durata, larghezza, velocita', densita'). Le unita' di "speed" invece non sono
//! documentate: le costanti qui sotto sono stime.

use serde_json::Value;
use std::collections::HashMap;
use std::f64::consts::TAU;

pub type Rgb = (u8, u8, u8);
pub type Stops = Vec<(f64, Rgb)>;

pub const BLACK: Rgb = (0, 0, 0);
const DEV_W: f64 = 200.0; // tela del dispositivo in Chroma Studio (px)
const DEV_H: f64 = 320.0;

const WAVE_HZ: f64 = 1.0 / 35.0; // cicli al secondo per unita' di "speed" (wave e tidal)
const WHEEL_DEG_S: f64 = 2.0; // gradi al secondo per unita' di "speed"
const RIPPLE_PX_S: f64 = 30.0; // px al secondo per unita' di "speed"
const RIPPLE_EVERY: (f64, f64) = (0.8, 2.5); // s tra due onde quando playbackStart = "random"
const STAR_MAX_DENSITY: f64 = 10.0; // "density" 10 = tutti i LED della regione accesi insieme
const STAR_REGEN: f64 = 3.0; // nuove stelle al secondo per LED spento, a density massima

pub fn rgb(c: u32) -> Rgb {
    (((c >> 16) & 255) as u8, ((c >> 8) & 255) as u8, (c & 255) as u8)
}

pub fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let ch = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t) as i64 as u8;
    (ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
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

/// Generatore xorshift: basta e avanza per stelle e onde a caso, senza dipendenze.
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
    pub fn color(&mut self) -> Rgb {
        let wheel = [(0.0, (255, 0, 0)), (33.0, (0, 255, 0)), (66.0, (0, 0, 255)), (100.0, (255, 0, 0))];
        gradient(&wheel, self.f64())
    }
}

/// Posizione sulla tela di ogni LED fisico, ricavata dalla griglia di Chroma Studio
/// ({(riga, colonna): indice LED}); la griglia copre tutta la tela.
pub struct Canvas {
    pub pos: Vec<(f64, f64)>,
}

impl Canvas {
    pub fn new(grid: &HashMap<(i64, i64), usize>, leds: usize) -> Self {
        let rows = grid.keys().map(|k| k.0).max().unwrap_or(1).max(1) as f64;
        let cols = grid.keys().map(|k| k.1).max().unwrap_or(1).max(1) as f64;
        let mut pos = vec![(DEV_W / 2.0, DEV_H / 2.0); leds];
        for (&(r, c), &i) in grid {
            if i < leds {
                pos[i] = (c as f64 / cols * DEV_W, r as f64 / rows * DEV_H);
            }
        }
        Canvas { pos }
    }

    /// Posizione 0..1 del LED lungo la direzione `angle` attraverso la tela. Gradi come su una
    /// bussola: 0 = verso l'alto, 90 = verso destra (il wave di default di Chroma Studio).
    fn along(&self, i: usize, angle: f64) -> f64 {
        let a = angle.to_radians();
        let (dx, dy) = (a.sin(), -a.cos());
        let (x, y) = self.pos[i];
        let span = (DEV_W * dx).abs() + (DEV_H * dy).abs();
        ((x - DEV_W / 2.0) * dx + (y - DEV_H / 2.0) * dy) / span + 0.5
    }
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

enum Kind {
    Static(Rgb),
    /// Tutta la regione dello stesso colore, un giro del gradiente ogni `period` s.
    Spectrum { stops: Stops, period: f64 },
    /// Wave, Tidal e Wheel: il gradiente scorre, `phase` e' la posizione di ogni LED della regione.
    Scroll { stops: Stops, phase: Vec<f64>, hz: f64 },
    /// Anelli che si allargano da un punto: a caso, oppure a ogni click se onPress.
    Ripple { stops: Stops, speed: f64, thick: f64, rings: Vec<(f64, f64, f64)>, next: f64, all: Vec<(f64, f64)>, at: Vec<(f64, f64)> },
    /// Stelle che nascono a caso: il gradiente e' la vita di una stella, `max` quante insieme.
    Starlight { stops: Stops, rnd: bool, life: f64, max: usize, rate: f64, stars: Vec<Option<(f64, Option<Rgb>)>>, last: Option<f64> },
    /// Un ciclo ogni `period` s: buio, respiro di color, buio, respiro di color2, buio.
    Breathing { fixed: Option<Stops>, c1: Rgb, c2: Rgb, rnd: bool, period: f64, cycle: i64, stops: Stops },
    /// Il calore tremola per conto suo su ogni LED: caldo = color, tiepido = color2, freddo = buio.
    Fire { stops: Stops },
}

pub struct Layer {
    pub leds: Vec<usize>,
    pub on_press: bool, // parte al primo click (per il ripple: un anello per click)
    running: bool,
    kind: Kind,
}

fn breathing_stops(c1: Rgb, c2: Rgb) -> Stops {
    vec![(0.0, BLACK), (12.0, BLACK), (25.0, c1), (38.0, c1), (50.0, BLACK), (62.0, BLACK), (75.0, c2), (88.0, c2), (100.0, BLACK)]
}

impl Layer {
    /// `name`: nome del layer nei log; `p`: parametri della regione; `lp`: parametri del layer.
    /// None = tipo di layer non supportato.
    pub fn new(name: &str, leds: Vec<usize>, p: &Value, lp: &Value, canvas: &Canvas, rng: &mut Rng) -> Option<Layer> {
        let on_press = p.get("playbackStart").and_then(Value::as_str) == Some("onPress");
        let mut running = !on_press;
        let kind = match name {
            "static" => Kind::Static(color(p, "color").unwrap_or(BLACK)),
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
                let (angle, split) = (num(p, "angle", 90.0), flag(p, "split"));
                let length = num(p, "width", 100.0).max(1.0) / 100.0; // lunghezze d'onda per tela
                // `counterclockwise` booleano nella regione (effetti rapidi del mouse) = onda che gira in tondo
                let spin = p.get("counterclockwise").and_then(Value::as_bool).map(|ccw| if ccw { -1.0 } else { 1.0 });
                let back = flag(lp, "counterclockwise") && spin.is_none(); // verso di marcia invertito
                let phase = leds
                    .iter()
                    .map(|&i| {
                        let pos = if let Some(dir) = spin {
                            let (x, y) = canvas.pos[i];
                            (y - DEV_H / 2.0).atan2(x - DEV_W / 2.0) / TAU * dir
                        } else {
                            let mut pos = canvas.along(i, angle);
                            if split {
                                pos = (pos - 0.5).abs() * 2.0;
                            }
                            if back { -pos } else { pos }
                        };
                        pos / length
                    })
                    .collect();
                Kind::Scroll { stops: st, phase, hz: num(p, "speed", 25.0) * WAVE_HZ }
            }
            "wheel" => {
                let (cx, cy) = (num(lp, "centerPointX", DEV_W / 2.0), num(lp, "centerPointY", DEV_H / 2.0));
                let dir = if flag(lp, "counterclockwise") { -1.0 } else { 1.0 };
                let phase = leds.iter().map(|&i| (canvas.pos[i].1 - cy).atan2(canvas.pos[i].0 - cx) / TAU).collect();
                Kind::Scroll { stops: stops(p)?, phase, hz: num(p, "speed", 60.0) * WHEEL_DEG_S / 360.0 * dir }
            }
            "ripple" => {
                running = true;
                Kind::Ripple {
                    stops: stops(p)?,
                    speed: num(p, "speed", 10.0).max(1.0) * RIPPLE_PX_S,
                    thick: num(p, "width", 300.0).max(30.0) / 3.0,
                    rings: Vec::new(),
                    next: 0.0,
                    all: canvas.pos.clone(),
                    at: leds.iter().map(|&i| canvas.pos[i]).collect(),
                }
            }
            "starlight" => {
                let dens = num(p, "density", 1.0).clamp(0.0, STAR_MAX_DENSITY) / STAR_MAX_DENSITY;
                Kind::Starlight {
                    stops: stops(p)?,
                    rnd: flag(p, "randomColor"),
                    life: num(p, "duration", 1700.0).max(100.0) / 1000.0,
                    max: if dens > 0.0 { ((leds.len() as f64 * dens).round() as usize).max(1) } else { 0 },
                    rate: dens * STAR_REGEN,
                    stars: vec![None; leds.len()],
                    last: None,
                }
            }
            "breathing" => {
                let rnd = flag(p, "randomColor");
                let c1 = color(p, "color").unwrap_or(rgb(0x00FF00));
                Kind::Breathing {
                    fixed: if rnd { None } else { stops(p) },
                    c1,
                    c2: color(p, "color2").unwrap_or(c1),
                    rnd,
                    period: num(p, "duration", 7000.0).max(200.0) / 1000.0,
                    cycle: -1,
                    stops: Vec::new(),
                }
            }
            "fire" => {
                let c1 = color(p, "color").unwrap_or(rgb(0xFF0000));
                Kind::Fire { stops: vec![(0.0, c1), (50.0, color(p, "color2").unwrap_or(c1)), (100.0, BLACK)] }
            }
            _ => return None,
        };
        Some(Layer { leds, on_press, running, kind })
    }

    pub fn press(&mut self, now: f64) {
        self.running = true;
        if let Kind::Ripple { rings, .. } = &mut self.kind {
            rings.push((now, DEV_W / 2.0, DEV_H / 2.0));
        }
    }

    pub fn paint(&mut self, now: f64, frame: &mut [Rgb], rng: &mut Rng) {
        if !self.running {
            return;
        }
        let on_press = self.on_press;
        for (n, &i) in self.leds.iter().enumerate() {
            let out: Option<(Rgb, f64)> = match &mut self.kind {
                Kind::Static(c) => Some((*c, 1.0)),
                Kind::Spectrum { stops, period } => Some((gradient(stops, now.rem_euclid(*period) / *period), 1.0)),
                Kind::Scroll { stops, phase, hz } => Some((gradient(stops, (phase[n] - now * *hz).rem_euclid(1.0)), 1.0)),
                Kind::Ripple { stops, speed, thick, rings, next, all, at } => {
                    if n == 0 {
                        if !on_press && now >= *next && !all.is_empty() {
                            let (x, y) = all[((rng.f64() * all.len() as f64) as usize).min(all.len() - 1)];
                            rings.push((now, x, y));
                            *next = now + RIPPLE_EVERY.0 + rng.f64() * (RIPPLE_EVERY.1 - RIPPLE_EVERY.0);
                        }
                        let reach = DEV_W.hypot(DEV_H) + *thick;
                        rings.retain(|r| (now - r.0) * *speed < reach);
                    }
                    let (x, y) = at[n];
                    let mut best: Option<f64> = None;
                    for &(t0, rx, ry) in rings.iter() {
                        let k = ((now - t0) * *speed - (x - rx).hypot(y - ry)) / *thick; // 0 = fronte, 1 = coda
                        if (0.0..=1.0).contains(&k) && best.map_or(true, |b| k < b) {
                            best = Some(k);
                        }
                    }
                    best.map(|k| (gradient(stops, k), 1.0 - k))
                }
                Kind::Starlight { stops, rnd, life, max, rate, stars, last } => {
                    if n == 0 {
                        let dt = last.map_or(0.0, |l| (now - l).min(0.5));
                        *last = Some(now);
                        for s in 0..stars.len() {
                            if matches!(stars[s], Some((t0, _)) if now - t0 >= *life) {
                                stars[s] = None;
                            }
                            let lit = stars.iter().filter(|x| x.is_some()).count();
                            if stars[s].is_none() && lit < *max && rng.f64() < *rate * dt {
                                stars[s] = Some((now, if *rnd { Some(rng.color()) } else { None }));
                            }
                        }
                    }
                    stars[n].map(|(t0, fixed)| {
                        let k = (now - t0) / *life;
                        match fixed {
                            Some(c) => (c, 1.0 - (2.0 * k - 1.0).abs()), // colore a caso: si accende e si spegne
                            None => (gradient(stops, k), (k / 0.15).min((1.0 - k) / 0.15).min(1.0)),
                        }
                    })
                }
                Kind::Breathing { fixed, c1, c2, rnd, period, cycle, stops } => {
                    let c = (now / *period) as i64;
                    if c != *cycle {
                        *cycle = c;
                        *stops = match fixed {
                            Some(s) => s.clone(),
                            None if *rnd => breathing_stops(rng.color(), rng.color()),
                            None => breathing_stops(*c1, *c2),
                        };
                    }
                    Some((gradient(stops, (now / *period).rem_euclid(1.0)), 1.0))
                }
                Kind::Fire { stops } => {
                    let f = i as f64;
                    let heat = ((now * 7.3 + f * 1.7).sin() + (now * 12.1 + f * 2.9).sin() + (now * 3.7 + f * 5.3).sin()) / 6.0 + 0.5;
                    Some((gradient(stops, (1.0 - heat) * 0.8), 1.0)) // 0.8: la fiamma non si spegne mai del tutto
                }
            };
            if let Some((c, a)) = out {
                frame[i] = if a >= 1.0 { c } else { lerp(frame[i], c, a) };
            }
        }
    }
}
