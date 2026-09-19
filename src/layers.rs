//! Rendering dei layer di Chroma Studio, con i parametri come li scrive Synapse nei log.
//!
//! Ogni layer dipinge sui propri LED sopra quello che c'e' gia' (alpha < 1 = lascia vedere i
//! layer sotto). Audio Meter e Reactive stanno in `effect.rs` perche' dipendono dal meter e
//! dai click.
//!
//! La forma di ogni effetto segue il modo in cui Chroma Studio lo descrive al suo motore
//! (gradiente, durata, larghezza, velocita', densita'). Le velocita' di wave, tidal e wheel sono
//! misurate sull'anteprima di Chroma Studio (frame del motore di Razer, 2026-09-19); quelle di
//! ripple e starlight restano stime.

use serde_json::Value;
use std::collections::HashMap;
use std::f64::consts::TAU;

pub type Rgb = (u8, u8, u8);
pub type Stops = Vec<(f64, Rgb)>;

pub const BLACK: Rgb = (0, 0, 0);
const DEV_W: f64 = 200.0; // tela del dispositivo in Chroma Studio (px)
const DEV_H: f64 = 320.0;
const CELL: f64 = 40.0; // lato di una cella della griglia dei LED sulla tela dell'editor (px)

// Misurati: il wave avanza a scatti di 40 ms e con larghezza 100% un LED ripete il gradiente ogni
// floor(625 / speed) scatti (speed 10 -> 2,48 s, 15 -> 1,64 s, 25 -> 1,00 s, 50 -> 0,48 s); il
// periodo cresce in proporzione alla larghezza e non dipende da angolo e split.
const WAVE_TICK: f64 = 0.04;
const WAVE_TICKS: f64 = 625.0;
const TIDAL_PERIOD: f64 = 50.0; // s * speed: tidal a speed 15 -> 3,33 s (misurato)
// wheel: "speed" = gradi al secondo (speed 60 -> un giro in 6 s, 120 -> 3 s, misurato)
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
/// ({(riga, colonna): indice LED}). Il motore di Razer lavora sulla griglia dei LED, con celle
/// da 40 px sulla tela dell'editor (`centerPointX` = px / 40): la tela del mouse, 200x320 px,
/// e' 5x8 celle, e ogni LED sta al centro della sua cella.
pub struct Canvas {
    pub pos: Vec<(f64, f64)>,
}

impl Canvas {
    pub fn new(grid: &HashMap<(i64, i64), usize>, leds: usize) -> Self {
        let mut pos = vec![(DEV_W / 2.0, DEV_H / 2.0); leds];
        for (&(r, c), &i) in grid {
            if i < leds {
                pos[i] = ((c as f64 + 0.5) * CELL, (r as f64 + 0.5) * CELL);
            }
        }
        Canvas { pos }
    }

    /// Distanza con segno del LED dalla retta che passa per `center` perpendicolare a `angle`,
    /// in frazioni dell'estensione `size` lungo `angle` (-0.5..0.5 dentro il riquadro). Gradi come
    /// su una bussola: 0 = verso l'alto, 90 = verso destra (il wave di default di Chroma Studio).
    fn along(&self, i: usize, angle: f64, center: (f64, f64), size: (f64, f64)) -> f64 {
        let a = angle.to_radians();
        let (dx, dy) = (a.sin(), -a.cos());
        let (x, y) = self.pos[i];
        let span = (size.0 * dx).abs() + (size.1 * dy).abs();
        ((x - center.0) * dx + (y - center.1) * dy) / span
    }

    /// Riquadro delle celle dei LED di una regione: centro e dimensioni (px). Il motore di Razer
    /// stende l'onda sulla regione, non su tutto il mouse: con larghezza 100% un'onda copre le
    /// righe della regione (misurato: regione di 3 righe -> 1/3 di periodo per riga, di 4 -> 1/4).
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

/// Centro di wheel e tidal sulla tela del mouse. Chroma Studio lo salva in px della tela
/// dell'editor (`centerPointX2/Y2`, 10000x10000) e ci mette accanto la posizione del mouse
/// (`deviceWithCenterPoint.x/y`); `centerPointX/Y` e' lo stesso punto diviso per 40.
fn center_point(lp: &Value) -> (f64, f64) {
    let dev = &lp["deviceWithCenterPoint"];
    let (Some(dx), Some(dy)) = (dev["x"].as_f64(), dev["y"].as_f64()) else {
        return (DEV_W / 2.0, DEV_H / 2.0);
    };
    let fine = (lp["centerPointX2"].as_f64(), lp["centerPointY2"].as_f64());
    let coarse = (lp["centerPointX"].as_f64().map(|v| v * CELL), lp["centerPointY"].as_f64().map(|v| v * CELL));
    match (fine, coarse) {
        ((Some(x), Some(y)), _) | (_, (Some(x), Some(y))) => (x - dx, y - dy),
        _ => (DEV_W / 2.0, DEV_H / 2.0),
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

/// Quando si ferma un effetto partito con un click (`playbackEnd`).
#[derive(Clone, Copy, PartialEq)]
enum End {
    Never,
    After(u32), // dopo N cicli (`cycles`)
    Toggle,     // al click successivo
}

enum Kind {
    Static(Option<Rgb>), // None = colore vuoto: il layer non copre niente
    /// Tutta la regione dello stesso colore, un giro del gradiente ogni `period` s.
    Spectrum { stops: Stops, period: f64 },
    /// Wave, Tidal e Wheel: il gradiente scorre, `phase` e' la posizione di ogni LED della regione.
    /// `pause`: cicli di buio dopo ogni passaggio (wave). `clear_black`: il nero lascia vedere i
    /// layer sotto (tidal, misurato).
    Scroll { stops: Stops, phase: Vec<f64>, hz: f64, pause: f64, clear_black: bool },
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
    pub on_press: bool, // parte con un click (per il ripple: anelli dal centro a ogni click)
    end: End,
    running: bool,
    start: f64, // istante del click che l'ha fatto partire: l'animazione riparte da capo
    kind: Kind,
}

fn breathing_stops(c1: Rgb, c2: Rgb) -> Stops {
    vec![(0.0, BLACK), (12.0, BLACK), (25.0, c1), (38.0, c1), (50.0, BLACK), (62.0, BLACK), (75.0, c2), (88.0, c2), (100.0, BLACK)]
}

impl Layer {
    /// `name`: nome del layer nei log; `p`: parametri della regione; `lp`: parametri del layer.
    /// None = tipo di layer non supportato.
    pub fn new(name: &str, leds: Vec<usize>, p: &Value, lp: &Value, canvas: &Canvas, rng: &mut Rng) -> Option<Layer> {
        // "onSelectedKeys": sul mouse nessun pulsante e' legato a un LED (LedInputMap), quindi
        // vale qualunque click, come "onPress"
        let on_press = matches!(p.get("playbackStart").and_then(Value::as_str), Some("onPress" | "onSelectedKeys"));
        let end = match p.get("playbackEnd").and_then(Value::as_str) {
            _ if !on_press => End::Never,
            Some("toggle") => End::Toggle,
            Some("never") => End::Never,
            _ => End::After(num(p, "cycles", 1.0).max(1.0) as u32),
        };
        let mut running = !on_press;
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
                // tidal: default del motore angle 160 e split attivo; l'onda parte dal centro scelto
                let tidal = name == "tidal";
                let angle = num(p, "angle", if tidal { 160.0 } else { 90.0 });
                let split = p.get("split").and_then(Value::as_bool).unwrap_or(tidal);
                let (middle, size) = canvas.region(&leds);
                let center = if tidal { center_point(lp) } else { middle };
                let length = num(p, "width", 100.0).max(1.0) / 100.0; // lunghezze d'onda per regione
                let speed = num(p, "speed", if tidal { 10.0 } else { 15.0 });
                let period = match (tidal, speed > 0.0) {
                    (_, false) => f64::INFINITY,
                    (true, true) => TIDAL_PERIOD / speed * length,
                    (false, true) => (WAVE_TICKS / speed).floor().max(1.0) * WAVE_TICK * length,
                };
                let hz = 1.0 / period;
                let pause = if tidal { 0.0 } else { num(p, "pause", 0.0).max(0.0) / 1000.0 * hz }; // ms -> cicli
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
                            let d = canvas.along(i, angle, center, size);
                            let pos = if split { d.abs() * 2.0 } else { d + 0.5 };
                            if back { -pos } else { pos }
                        };
                        pos / length
                    })
                    .collect();
                Kind::Scroll { stops: st, phase, hz, pause, clear_black: tidal }
            }
            "wheel" => {
                let (cx, cy) = center_point(lp);
                let dir = if flag(lp, "counterclockwise") { -1.0 } else { 1.0 };
                let phase = leds.iter().map(|&i| (canvas.pos[i].1 - cy).atan2(canvas.pos[i].0 - cx) / TAU).collect();
                Kind::Scroll { stops: stops(p)?, phase, hz: num(p, "speed", 60.0) / 360.0 * dir, pause: 0.0, clear_black: false }
            }
            "ripple" => {
                running = true;
                Kind::Ripple {
                    stops: stops(p)?,
                    speed: num(p, "speed", 10.0).max(1.0) * RIPPLE_PX_S,
                    thick: num(p, "width", 300.0).max(30.0) / 3.0,
                    rings: Vec::new(),
                    next: if on_press { f64::INFINITY } else { 0.0 }, // con un click: niente anelli finche' non si preme
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
                // con i colori a caso il motore di Razer dimezza la durata del ciclo
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
                Kind::Fire { stops: vec![(0.0, c1), (50.0, color(p, "color2").unwrap_or(c1)), (100.0, BLACK)] }
            }
            _ => return None,
        };
        Some(Layer { leds, on_press, end, running, start: 0.0, kind })
    }

    pub fn press(&mut self, now: f64) {
        if let Kind::Ripple { rings, speed, thick, next, .. } = &mut self.kind {
            match self.end {
                // toggle: il click accende o spegne l'emissione continua di anelli dal centro
                End::Toggle => *next = if next.is_finite() { f64::INFINITY } else { now },
                // un anello dal centro per ciclo, uno dietro l'altro
                End::After(n) => rings.extend((0..n).map(|k| (now + k as f64 * *thick / *speed, DEV_W / 2.0, DEV_H / 2.0))),
                End::Never => rings.push((now, DEV_W / 2.0, DEV_H / 2.0)),
            }
            return;
        }
        if self.end == End::Toggle && self.running {
            self.running = false;
            return;
        }
        self.running = true;
        self.start = now;
    }

    /// Durata di un ciclo, per fermare gli effetti partiti con un click dopo N cicli.
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
        // partito con un click: l'animazione riparte da capo e si ferma dopo N cicli
        let t = if on_press { now - self.start } else { now };
        if let (true, End::After(n), Some(c)) = (on_press, self.end, self.cycle()) {
            if t >= n as f64 * c {
                self.running = false;
                return;
            }
        }
        for (n, &i) in self.leds.iter().enumerate() {
            let out: Option<(Rgb, f64)> = match &mut self.kind {
                Kind::Static(c) => c.map(|c| (c, 1.0)),
                Kind::Spectrum { stops, period } => Some((gradient(stops, now.rem_euclid(*period) / *period), 1.0)),
                Kind::Scroll { stops, phase, hz, pause, clear_black } => {
                    // u = quanta parte del gradiente e' gia' passata sul LED; con la pausa, dopo un
                    // passaggio il LED resta spento per `pause` cicli
                    let u = (t * *hz - phase[n]).rem_euclid(1.0 + *pause);
                    let c = gradient(stops, (-u).rem_euclid(1.0));
                    (u < 1.0 && !(*clear_black && c == BLACK)).then_some((c, 1.0))
                }
                Kind::Ripple { stops, speed, thick, rings, next, all, at } => {
                    if n == 0 {
                        if !on_press && now >= *next && !all.is_empty() {
                            let (x, y) = all[((rng.f64() * all.len() as f64) as usize).min(all.len() - 1)];
                            rings.push((now, x, y));
                            *next = now + RIPPLE_EVERY.0 + rng.f64() * (RIPPLE_EVERY.1 - RIPPLE_EVERY.0);
                        } else if on_press && now >= *next {
                            // toggle acceso: anelli dal centro uno dietro l'altro
                            rings.push((now, DEV_W / 2.0, DEV_H / 2.0));
                            *next = now + *thick / *speed;
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
                    let c = (t / *period).floor() as i64;
                    if c != *cycle {
                        *cycle = c;
                        *stops = match fixed {
                            Some(s) => s.clone(),
                            None if *rnd => breathing_stops(rng.color(), rng.color()),
                            None => breathing_stops(*c1, *c2),
                        };
                    }
                    Some((gradient(stops, (t / *period).rem_euclid(1.0)), 1.0))
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
    fn led_al_centro_della_sua_cella() {
        assert_eq!(canvas().pos, vec![(100.0, 20.0), (180.0, 260.0)]);
    }

    #[test]
    fn centro_dai_px_dell_editor_meno_la_posizione_del_mouse() {
        let dev = json!({"x": 4920, "y": 4840});
        assert_eq!(center_point(&json!({"centerPointX2": 5068, "centerPointY2": 4976, "deviceWithCenterPoint": dev})), (148.0, 136.0));
        assert_eq!(center_point(&json!({"centerPointX": 127, "centerPointY": 124, "deviceWithCenterPoint": dev})), (160.0, 120.0));
        assert_eq!(center_point(&json!({"centerPointX": 127})), (DEV_W / 2.0, DEV_H / 2.0));
    }

    fn wave(extra: Value) -> Value {
        let mut p = json!({"speed": 25, "width": 100, "colorStops": [{"Stop": 0, "Color": 0xFF0000}, {"Stop": 100, "Color": 0xFF0000}]});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    }

    #[test]
    fn dopo_n_cicli_si_ferma() {
        // speed 25 = un ciclo al secondo (misurato); 2 cicli dal click a t=10
        let mut l = layer("wave", wave(json!({"playbackStart": "onPress", "playbackEnd": "after", "cycles": 2})), json!({}));
        assert!(!lit(&mut l, 5.0), "prima del click non si vede");
        l.press(10.0);
        assert!(lit(&mut l, 10.5) && lit(&mut l, 11.9), "acceso per due cicli");
        assert!(!lit(&mut l, 12.1), "spento dopo il secondo ciclo");
        l.press(20.0);
        assert!(lit(&mut l, 20.1), "un nuovo click lo fa ripartire");
    }

    #[test]
    fn toggle_acceso_e_spento_dai_click() {
        let mut l = layer("wave", wave(json!({"playbackStart": "onPress", "playbackEnd": "toggle"})), json!({}));
        l.press(1.0);
        assert!(lit(&mut l, 50.0), "resta acceso finche' non si clicca di nuovo");
        l.press(51.0);
        assert!(!lit(&mut l, 52.0));
    }

    #[test]
    fn on_selected_keys_parte_col_click() {
        let mut l = layer("wave", wave(json!({"playbackStart": "onSelectedKeys", "playbackEnd": "after", "cycles": 1})), json!({}));
        assert!(l.on_press && !lit(&mut l, 1.0));
    }

    #[test]
    fn random_non_si_ferma_mai() {
        let mut l = layer("wave", wave(json!({"playbackStart": "random", "playbackEnd": "never", "cycles": -1})), json!({}));
        assert!(lit(&mut l, 1000.0));
    }

    #[test]
    fn static_senza_colore_e_trasparente() {
        let mut l = layer("static", json!({}), json!({}));
        let mut frame = vec![(1, 2, 3); 2];
        l.paint(0.0, &mut frame, &mut Rng::new(1));
        assert_eq!(frame, vec![(1, 2, 3); 2]);
        let mut l = layer("static", json!({"color2": 0x0000FF}), json!({}));
        l.paint(0.0, &mut frame, &mut Rng::new(1));
        assert_eq!(frame, vec![(0, 0, 255); 2]);
    }

    #[test]
    fn breathing_a_caso_dura_la_meta() {
        let period = |p: Value| match layer("breathing", p, json!({})).kind {
            Kind::Breathing { period, .. } => period,
            _ => unreachable!(),
        };
        assert_eq!(period(json!({"duration": 14000})), 14.0);
        assert_eq!(period(json!({"duration": 14000, "randomColor": true})), 7.0);
        assert_eq!(period(json!({})), 14.0);
    }

    fn period(l: &Layer) -> f64 {
        match &l.kind {
            Kind::Scroll { hz, .. } => 1.0 / hz.abs(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn periodi_misurati_su_chroma_studio() {
        let w = |speed: f64, width: f64| period(&layer("wave", wave(json!({"speed": speed, "width": width})), json!({})));
        for (speed, width, measured) in [(10.0, 100.0, 2.48), (15.0, 100.0, 1.64), (30.0, 100.0, 0.80), (45.0, 100.0, 0.52), (50.0, 100.0, 0.48), (10.0, 200.0, 4.96), (10.0, 50.0, 1.24)] {
            assert!((w(speed, width) - measured).abs() < 0.005, "wave {speed}/{width}: {} invece di {measured}", w(speed, width));
        }
        let tidal = layer("tidal", json!({"speed": 15, "color": 0xFF0000, "color2": 0xFF00FF}), json!({}));
        assert!((period(&tidal) - 3.336).abs() < 0.01);
        let wheel = layer("wheel", wave(json!({"speed": 60})), json!({}));
        assert!((period(&wheel) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn pausa_spegne_il_led_dopo_un_passaggio() {
        // speed 25 = 1 s per passaggio, poi 2 s spento (misurato: 2,48 s + 2 s di pausa = 4,48 s)
        let p = wave(json!({"pause": 2000}));
        let mut l = Layer::new("wave", vec![0], &p, &json!({}), &canvas(), &mut Rng::new(1)).unwrap();
        let lit_at: Vec<bool> = (0..30).map(|k| lit(&mut l, k as f64 * 0.1 + 0.05)).collect();
        assert_eq!(lit_at.iter().filter(|&&x| x).count(), 10, "acceso 1 s ogni 3 s: {lit_at:?}");
        assert!(lit(&mut l, 3.05) == lit(&mut l, 0.05));
    }

    #[test]
    fn onda_stesa_sulla_regione() {
        // 4 righe (3..6) in verticale: un'onda al 100% copre la regione, 1/4 di periodo per riga
        let canvas = Canvas::new(&HashMap::from([((3, 0), 0), ((4, 0), 1), ((6, 0), 2)]), 3);
        let p = wave(json!({"angle": 180}));
        let l = Layer::new("wave", vec![0, 1, 2], &p, &json!({}), &canvas, &mut Rng::new(1)).unwrap();
        let Kind::Scroll { phase, .. } = &l.kind else { unreachable!() };
        assert!(((phase[1] - phase[0]) - 0.25).abs() < 1e-9 && ((phase[2] - phase[1]) - 0.5).abs() < 1e-9, "{phase:?}");
    }

    #[test]
    fn il_nero_del_tidal_e_trasparente() {
        let mut l = layer("tidal", json!({"speed": 15, "color": 0x00FF00, "color2": 0x0000FF}), json!({}));
        let covered = (0..200).filter(|k| {
            let mut frame = vec![(9, 9, 9); 2];
            l.paint(*k as f64 * 0.02, &mut frame, &mut Rng::new(1));
            frame[0] != (9, 9, 9)
        }).count();
        assert!(covered > 0 && covered < 200, "il tidal deve coprire solo a tratti: {covered}/200");
    }

    #[test]
    fn ripple_con_click_niente_anelli_prima_del_click() {
        let p = json!({"speed": 10, "width": 300, "playbackStart": "onPress", "playbackEnd": "after", "cycles": 3,
                       "colorStops": [{"Stop": 0, "Color": 0xFF0000}, {"Stop": 100, "Color": 0xFF0000}]});
        let mut l = layer("ripple", p, json!({}));
        assert!(!lit(&mut l, 30.0));
        l.press(31.0);
        match &l.kind {
            Kind::Ripple { rings, .. } => assert_eq!(rings.len(), 3, "un anello per ciclo"),
            _ => unreachable!(),
        }
    }
}
