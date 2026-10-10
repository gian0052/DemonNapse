//! Chroma Studio's Ambient layer: the LEDs take the colours of the primary screen.
//!
//! Measured on the Chroma Studio preview with windows of known colours (2026-09-19):
//! - the region becomes a grid of cells (the bounding box of its LEDs' rows and columns) and
//!   each cell reads ONE pixel, the top-left corner of its part of the screen area:
//!   x = floor(L + i * (R - L) / columns), y = floor(T + j * (B - T) / rows);
//! - the screen area (`screen`) is in 0..65535 of the primary screen, truncated to an integer, and
//!   becomes pixels with the integer division v * size / 65535; full = everything, top/bottom = top/bottom 10%,
//!   left/right = 10% on that side, custom = `screenRect` (drawn by the user);
//! - `blur` (1..9) blurs the grid, rows and columns separately, with the edges repeated to infinity;
//!   the kernel is in `TAIL` (measured: it is not an exact Gaussian);
//! - no averaging over time: the colour follows the screen at every 40 ms tick of the engine;
//! - the layer covers the ones below, black included;
//! - without `blur`, or with `custom` without `screenRect`, the engine draws nothing (transparent);
//!   blur 0 = no blur; above 9 (the editor does not allow it) I use the kernel of 9.

use crate::layers::{Canvas, Rgb, BLACK};
use serde_json::Value;

const FULL: f64 = 65535.0;
pub const TICK: f64 = 0.04; // Razer's engine updates every 40 ms

// For each blur 1..9, TAIL[d] = weight a cell receives from the edge when it is d cells away from it
// (sum of the kernel from d onwards), in 255ths: measured by lighting row 0 only.
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

/// Sum of the kernel's tail from `d` onwards (0..1). Beyond the 7 measured cells (grids larger
/// than the Basilisk's) the tail keeps falling with the same ratio.
fn tail(blur: usize, d: usize) -> f64 {
    let t = &TAIL[blur - 1];
    if d < t.len() {
        return t[d] / 255.0;
    }
    let ratio = if t[5] > 0.0 { t[6] / t[5] } else { 0.0 };
    t[6] / 255.0 * ratio.powi((d - 6) as i32)
}

/// Weights of a grid of `n` cells along one axis: w[t * n + j] = how much cell j weighs on cell t.
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
                tail(blur, t) // the repeated edge: everything that lies on this side
            } else if j == n - 1 {
                tail(blur, n - 1 - t)
            } else {
                k(t.abs_diff(j))
            };
        }
        // the table is rounded to 1/255: I bring the sum back to 1, as measured on a
        // screen of a single colour (it stays that colour)
        let sum: f64 = w[t * n..(t + 1) * n].iter().sum();
        w[t * n..(t + 1) * n].iter_mut().for_each(|v| *v /= sum);
    }
    w
}

pub struct Ambient {
    leds: Vec<(usize, usize, usize)>, // (LED, row, column) in the region's grid
    rows: usize,
    cols: usize,
    rect: Option<[u64; 4]>, // Left, Top, Right, Bottom in 0..65535; None = the engine does not draw
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

    /// The pixels to read on a `w` x `h` screen: columns and rows of the grid. None = the
    /// layer does not draw (parameters the engine does not accept).
    pub fn points(&self, w: i32, h: i32) -> Option<(Vec<i32>, Vec<i32>)> {
        let [l, t, r, b] = self.rect?;
        let px = |v: u64, dim: i32| (v * dim as u64 / FULL as u64) as f64;
        let (l, r, t, b) = (px(l, w), px(r, w), px(t, h), px(b, h));
        let xs = (0..self.cols).map(|i| (l + i as f64 * (r - l) / self.cols as f64) as i32).collect();
        let ys = (0..self.rows).map(|j| (t + j as f64 * (b - t) / self.rows as f64) as i32).collect();
        Some((xs, ys))
    }

    /// `samples`: the pixels read, row by row (rows x columns).
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

    // the LED grid of the Basilisk V3 35K (row, column) as in config.json
    const BASILISK: [(i64, i64); 11] = [(0, 2), (5, 2), (1, 0), (2, 0), (3, 0), (4, 0), (6, 0), (6, 4), (4, 4), (2, 4), (0, 4)];

    fn ambient(p: Value) -> Ambient {
        let grid: HashMap<(i64, i64), usize> = BASILISK.iter().enumerate().map(|(i, &rc)| (rc, i)).collect();
        Ambient::new((0..11).collect(), &p, &Canvas::new(&grid, 11))
    }

    #[test]
    fn reads_the_same_pixels_as_razer() {
        // measured to the pixel on a 2560x1440 screen with 1-pixel-wide lines
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
    fn a_lit_pixel_blurs_like_on_razer() {
        // row 3 white, blur 1: measured 4 15 52 106 52 15 4 on rows 0..6
        let mut a = ambient(json!({"screen": "full", "blur": 1}));
        let mut s = vec![BLACK; 35];
        for i in 0..5 {
            s[3 * 5 + i] = (255, 255, 255);
        }
        a.update(&s);
        let mut frame = vec![BLACK; 11];
        a.paint(&mut frame);
        let rows: Vec<i32> = [0, 2, 3, 4, 5, 1, 6].iter().map(|&i| frame[i].0 as i32).collect(); // LEDs of rows 0..6
        let razer = [4, 15, 52, 106, 52, 15, 4];
        assert!(rows.iter().zip(razer).all(|(a, b)| (a - b).abs() <= 1), "{rows:?}");
        // a screen of a single colour stays that colour
        a.update(&vec![(255, 0, 0); 35]);
        a.paint(&mut frame);
        assert!(frame.iter().all(|c| c.0 >= 253 && c.1 == 0));
    }

    #[test]
    fn without_blur_or_screen_area_the_engine_draws_nothing() {
        // measured: no colour, the layer below shows through
        assert!(ambient(json!({"screen": "full"})).points(2560, 1440).is_none());
        assert!(ambient(json!({"screen": "custom", "blur": 5})).points(2560, 1440).is_none());
        let mut frame = vec![(0, 0, 255); 11];
        ambient(json!({"screen": "custom", "blur": 5})).paint(&mut frame);
        assert!(frame.iter().all(|&c| c == (0, 0, 255)));
    }
}
