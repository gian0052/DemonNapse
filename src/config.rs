//! config.json: mouse da usare, griglia di Chroma Studio e guadagno dell'Audio Meter.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
pub struct Device {
    pub pid: String, // PID USB in esadecimale, es. "0x00CB"
    pub leds: usize,
    pub chroma_grid: HashMap<String, usize>, // "riga,colonna" di Chroma Studio -> indice LED fisico
}

#[derive(Deserialize)]
pub struct Config {
    pub device: Device,
    #[serde(default)]
    pub effect_overrides: HashMap<String, String>,
    #[serde(default = "default_skip")]
    pub skip_slots: Vec<u8>,
    #[serde(default = "default_fps")]
    pub fps: f64,
    #[serde(default = "default_gain")]
    pub audio_gain: f64,
}

fn default_skip() -> Vec<u8> {
    vec![1] // slot 1 = Hybrid Storage
}
fn default_fps() -> f64 {
    60.0
}
fn default_gain() -> f64 {
    1.0
}

impl Config {
    pub fn load(dir: &Path) -> Result<Config, String> {
        let path = dir.join("config.json");
        let txt = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&txt).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn pid(&self) -> u16 {
        u16::from_str_radix(self.device.pid.trim_start_matches("0x").trim_start_matches("0X"), 16).unwrap_or(0)
    }

    /// (riga, colonna) -> indice LED fisico
    pub fn grid(&self) -> HashMap<(i64, i64), usize> {
        self.device
            .chroma_grid
            .iter()
            .filter_map(|(k, &v)| {
                let (r, c) = k.split_once(',')?;
                Some(((r.trim().parse().ok()?, c.trim().parse().ok()?), v))
            })
            .collect()
    }
}
