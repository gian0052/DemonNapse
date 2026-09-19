//! Layer Ambient di Chroma Studio: i LED prendono i colori dello schermo principale.
//!
//! Misurato sull'anteprima di Chroma Studio con finestre di colori noti (2026-09-19):
//! - la regione diventa una griglia di celle (il riquadro delle righe e colonne dei suoi LED) e
//!   ogni cella legge UN pixel, l'angolo in alto a sinistra della sua parte di zona:
//!   x = floor(L + i * (R - L) / colonne), y = floor(T + j * (B - T) / righe);
//! - la zona (`screen`) e' in 0..65535 dello schermo principale, troncata a intero, e diventa
//!   pixel con v * dimensione / 65535 intero; full = tutto, top/bottom = 10% alto/basso,
//!   left/right = 10% laterale, custom = `screenRect` (disegnato dall'utente);
//! - `blur` (1..9) sfoca la griglia, riga e colonna separate, con i bordi ripetuti all'infinito;
//!   il nucleo e' in `TAIL` (misurato: non e' una gaussiana esatta);
//! - nessuna media nel tempo: il colore segue lo schermo a ogni scatto da 40 ms del motore;
//! - il layer copre quelli sotto, nero compreso;
//! - senza `blur`, o con `custom` senza `screenRect`, il motore non disegna niente (trasparente);
//!   blur 0 = nessuna sfocatura; oltre 9 (l'editor non lo permette) uso il nucleo di 9.

use crate::layers::{Canvas, Rgb, BLACK};
use serde_json::Value;

const FULL: f64 = 65535.0;
pub const TICK: f64 = 0.04; // il motore di Razer aggiorna ogni 40 ms

// Per ogni blur 1..9, TAIL[d] = peso che una cella riceve dal bordo quando sta a d celle da esso
// (somma del nucleo da d in poi), in 255esimi: misurato illuminando la sola riga 0.
const TAIL: [[f64; 7]; 9] = [
    [180.0, 74.0, 21.0, 5.0, 1.0, 0.0, 0.0],
    [154.0, 100.0, 57.0, 31.0, 16.0, 8.0, 3.0],
    [144.0, 109.0, 77.0, 52.0, 34.0, 22.0, 14.0],
    [141.0, 113.0, 88.0, 67.0, 50.0, 36.0, 26.0],
    [137.0, 116.0, 95.0, 77.0, 61.0, 48.0, 37.0],
    [136.0, 118.0, 101.0, 85.0, 70.0, 58.0, 47.0],
    [134.0, 119.0, 104.0, 90.0, 77.0, 65.0, 55.0],
    [133.0, 120.0, 107.0, 94.0, 82.0, 72.0, 62.0],
    [133.0, 121.0, 109.0, 98.0, 87.0, 77.0, 68.0],
];

/// Somma della coda del nucleo da `d` in poi (0..1). Oltre le 7 celle misurate (griglie piu'
/// grandi di quella del Basilisk) la coda continua a scendere con lo stesso rapporto.
fn tail(blur: usize, d: usize) -> f64 {
    let t = &TAIL[blur - 1];
    if d < t.len() {
        return t[d] / 255.0;
    }
    let ratio = if t[5] > 0.0 { t[6] / t[5] } else { 0.0 };
    t[6] / 255.0 * ratio.powi((d - 6) as i32)
}

/// Pesi di una griglia di `n` celle su un asse: w[t * n + j] = quanto la cella j pesa sulla t.
fn weights(n: usize, blur: usize) -> Vec<f64> {
    if blur == 0 {
        return (0..n * n).map(|k| if k / n == k % n { 1.0 } else { 0.0 }).collect();
    }
    let k = |d: usize| if d == 0 { 2.0 * tail(blur, 0) - 1.0 } else { tail(blur, d) - tail(blur, d + 1) };
    let mut w = vec![0.0; n * n];
    for t in 0..n {
        for j in 0..n {
            w[t * n + j] = if n == 1 {
                1.0
            } else if j == 0 {
                tail(blur, t) // il bordo ripetuto: tutto quello che sta da questa parte
            } else if j == n - 1 {
                tail(blur, n - 1 - t)
            } else {
                k(t.abs_diff(j))
            };
        }
        // la tabella e' arrotondata a 1/255: rimetto la somma a 1, come misurato su uno
        // schermo tutto di un colore (resta quel colore)
        let sum: f64 = w[t * n..(t + 1) * n].iter().sum();
        w[t * n..(t + 1) * n].iter_mut().for_each(|v| *v /= sum);
    }
    w
}

pub struct Ambient {
    leds: Vec<(usize, usize, usize)>, // (LED, riga, colonna) nella griglia della regione
    rows: usize,
    cols: usize,
    rect: Option<[u64; 4]>, // Left, Top, Right, Bottom in 0..65535; None = il motore non disegna
    wr: Vec<f64>,
    wc: Vec<f64>,
    colors: Vec<Rgb>,
}

impl Ambient {
    pub fn new(leds: Vec<usize>, p: &Value, canvas: &Canvas) -> Ambient {
        let cells: Vec<(i64, i64)> = leds.iter().map(|&i| canvas.cell(i)).collect();
        let (r0, c0) = (cells.iter().map(|c| c.0).min().unwrap_or(0), cells.iter().map(|c| c.1).min().unwrap_or(0));
        let rows = (cells.iter().map(|c| c.0).max().unwrap_or(0) - r0 + 1) as usize;
        let cols = (cells.iter().map(|c| c.1).max().unwrap_or(0) - c0 + 1) as usize;
        let custom = &p["screenRect"];
        let side = |k: &str| custom[k].as_f64();
        let rect = match p["screen"].as_str() {
            Some("top") => Some([0.0, 0.0, FULL, 0.1 * FULL]),
            Some("bottom") => Some([0.0, 0.9 * FULL, FULL, FULL]),
            Some("left") => Some([0.0, 0.0, 0.1 * FULL, FULL]),
            Some("right") => Some([0.9 * FULL, 0.0, FULL, FULL]),
            Some("custom") => match (side("Left"), side("Top"), side("Right"), side("Bottom")) {
                (Some(l), Some(t), Some(r), Some(b)) => Some([l, t, r, b]),
                _ => None,
            },
            _ => Some([0.0, 0.0, FULL, FULL]),
        }
        .map(|r| r.map(|v| v.clamp(0.0, FULL) as u64));
        let blur = p["blur"].as_f64();
        let rect = rect.filter(|_| blur.is_some());
        let blur = blur.unwrap_or(0.0).clamp(0.0, 9.0) as usize;
        let leds = leds.iter().zip(&cells).map(|(&i, &(r, c))| (i, (r - r0) as usize, (c - c0) as usize)).collect();
        Ambient { leds, rows, cols, rect, wr: weights(rows, blur), wc: weights(cols, blur), colors: vec![BLACK; rows * cols] }
    }

    /// I pixel da leggere su uno schermo `w` x `h`: colonne e righe della griglia. None = il
    /// layer non disegna (parametri che il motore non accetta).
    pub fn points(&self, w: i32, h: i32) -> Option<(Vec<i32>, Vec<i32>)> {
        let [l, t, r, b] = self.rect?;
        let px = |v: u64, dim: i32| (v * dim as u64 / FULL as u64) as f64;
        let (l, r, t, b) = (px(l, w), px(r, w), px(t, h), px(b, h));
        let xs = (0..self.cols).map(|i| (l + i as f64 * (r - l) / self.cols as f64) as i32).collect();
        let ys = (0..self.rows).map(|j| (t + j as f64 * (b - t) / self.rows as f64) as i32).collect();
        Some((xs, ys))
    }

    /// `samples`: i pixel letti, riga per riga (righe x colonne).
    pub fn update(&mut self, samples: &[Rgb]) {
        let (nr, nc) = (self.rows, self.cols);
        for &(_, t, u) in &self.leds {
            let mut acc = [0.0f64; 3];
            for j in 0..nr {
                let wr = self.wr[t * nr + j];
                if wr == 0.0 {
                    continue;
                }
                for i in 0..nc {
                    let w = wr * self.wc[u * nc + i];
                    let s = samples[j * nc + i];
                    acc[0] += w * s.0 as f64;
                    acc[1] += w * s.1 as f64;
                    acc[2] += w * s.2 as f64;
                }
            }
            let ch = |v: f64| v.round().clamp(0.0, 255.0) as u8;
            self.colors[t * nc + u] = (ch(acc[0]), ch(acc[1]), ch(acc[2]));
        }
    }

    pub fn paint(&self, frame: &mut [Rgb]) {
        if self.rect.is_none() {
            return;
        }
        for &(i, t, u) in &self.leds {
            frame[i] = self.colors[t * self.cols + u];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    // la griglia dei LED del Basilisk V3 35K (riga, colonna) come in config.json
    const BASILISK: [(i64, i64); 11] = [(0, 2), (5, 2), (1, 0), (2, 0), (3, 0), (4, 0), (6, 0), (6, 4), (4, 4), (2, 4), (0, 4)];

    fn ambient(p: Value) -> Ambient {
        let grid: HashMap<(i64, i64), usize> = BASILISK.iter().enumerate().map(|(i, &rc)| (rc, i)).collect();
        Ambient::new((0..11).collect(), &p, &Canvas::new(&grid, 11))
    }

    #[test]
    fn i_pixel_letti_sono_quelli_di_razer() {
        // misurati al pixel su uno schermo 2560x1440 con linee larghe 1 pixel
        let a = ambient(json!({"screen": "full", "blur": 1}));
        assert_eq!(a.points(2560, 1440).unwrap(), (vec![0, 512, 1024, 1536, 2048], vec![0, 205, 411, 617, 822, 1028, 1234]));
        let a = ambient(json!({"screen": "right", "blur": 1}));
        assert_eq!(a.points(2560, 1440).unwrap().0, vec![2303, 2354, 2405, 2457, 2508]);
        let a = ambient(json!({"screen": "top", "blur": 1}));
        assert_eq!(a.points(2560, 1440).unwrap().1, vec![0, 20, 40, 61, 81, 102, 122]);
        let a = ambient(json!({"screen": "custom", "screenRect": {"Left": 16384, "Top": 20000, "Right": 49151, "Bottom": 45000}, "blur": 7}));
        assert_eq!(a.points(2560, 1440).unwrap(), (vec![640, 895, 1151, 1407, 1663], vec![439, 517, 595, 674, 752, 831, 909]));
    }

    #[test]
    fn un_pixel_acceso_si_sfoca_come_su_razer() {
        // riga 3 bianca, blur 1: misurato 4 15 52 106 52 15 4 sulle righe 0..6
        let mut a = ambient(json!({"screen": "full", "blur": 1}));
        let mut s = vec![BLACK; 35];
        for i in 0..5 {
            s[3 * 5 + i] = (255, 255, 255);
        }
        a.update(&s);
        let mut frame = vec![BLACK; 11];
        a.paint(&mut frame);
        let rows: Vec<i32> = [0, 2, 3, 4, 5, 1, 6].iter().map(|&i| frame[i].0 as i32).collect(); // LED delle righe 0..6
        let razer = [4, 15, 52, 106, 52, 15, 4];
        assert!(rows.iter().zip(razer).all(|(a, b)| (a - b).abs() <= 1), "{rows:?}");
        // uno schermo tutto di un colore resta di quel colore
        a.update(&vec![(255, 0, 0); 35]);
        a.paint(&mut frame);
        assert!(frame.iter().all(|c| c.0 >= 253 && c.1 == 0));
    }

    #[test]
    fn senza_blur_o_senza_zona_il_motore_non_disegna() {
        // misurato: nessun colore, si vede il layer sotto
        assert!(ambient(json!({"screen": "full"})).points(2560, 1440).is_none());
        assert!(ambient(json!({"screen": "custom", "blur": 5})).points(2560, 1440).is_none());
        let mut frame = vec![(0, 0, 255); 11];
        ambient(json!({"screen": "custom", "blur": 5})).paint(&mut frame);
        assert!(frame.iter().all(|&c| c == (0, 0, 255)));
    }
}
