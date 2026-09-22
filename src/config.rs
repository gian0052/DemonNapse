//! config.json: mouse da usare, griglia di Chroma Studio, abbinamenti forzati profilo -> effetto
//! (`effect_overrides`), slot da saltare (`skip_slots`), frame al secondo (`fps`) e guadagno
//! dell'Audio Meter.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
pub struct Device {
    pub pid: String, // PID USB in esadecimale, es. "0x00CB"
    // se mancano si usano quelli letti dai log di Synapse (synapse_device.json)
    #[serde(default)]
    pub leds: usize,
    #[serde(default)]
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
        let mut cfg: Config = serde_json::from_str(&txt).map_err(|e| format!("{}: {e}", path.display()))?;
        if cfg.device.chroma_grid.is_empty() {
            // mappa dei LED non scritta a mano: quella di Chroma Studio, se e' gia' stata estratta
            let read = std::fs::read_to_string(dir.join("synapse_device.json")).ok();
            if let Some(map) = read.and_then(|t| serde_json::from_str::<crate::extract::DeviceMap>(&t).ok()) {
                cfg.device.leds = map.leds;
                cfg.device.chroma_grid = map.chroma_grid.into_iter().collect();
            }
        }
        Ok(cfg)
    }

    /// Errore se non c'e' una mappa dei LED ne' in config.json ne' dai log di Synapse.
    pub fn check_device(&self) -> Result<(), String> {
        if self.device.leds == 0 || self.grid().is_empty() {
            return Err("no LED map for the mouse: open Synapse once and close it, or set device.leds and device.chroma_grid in config.json".into());
        }
        Ok(())
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
