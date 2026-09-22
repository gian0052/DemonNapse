//! config.json: mouse da usare, griglia di Chroma Studio, abbinamenti forzati profilo -> effetto
//! (`effect_overrides`), slot da saltare (`skip_slots`), frame al secondo (`fps`) e guadagno
//! dell'Audio Meter.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
pub struct Device {
    // PID USB in esadecimale, es. "0x00CB"; se manca il mouse viene cercato tra i dispositivi collegati
    #[serde(default)]
    pub pid: String,
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
    /// Il mouse trovato da solo quando `device.pid` manca: (PID, come e' stato scelto) o perche' no.
    #[serde(skip)]
    pub detected: Option<Result<(u16, String), String>>,
}

/// Quale dispositivo Razer collegato e' il mouse: quello che Synapse chiama "mouse" o, se Synapse non
/// ne conosce il tipo, l'unico collegato. Mai una tastiera o altro, e mai a caso tra piu' candidati.
pub fn choose_mouse(connected: &[(u16, String)], categories: &HashMap<u16, String>) -> Result<(u16, String), String> {
    let list = |devs: &[&(u16, String)]| devs.iter().map(|(p, n)| format!("0x{p:04X} {n}")).collect::<Vec<_>>().join(", ");
    let mice: Vec<&(u16, String)> = connected.iter().filter(|(p, _)| categories.get(p).is_some_and(|c| c == "mouse")).collect();
    let unknown: Vec<&(u16, String)> = connected.iter().filter(|(p, _)| !categories.contains_key(p)).collect();
    match (mice.as_slice(), unknown.as_slice()) {
        ([(pid, name)], _) => Ok((*pid, format!("0x{pid:04X} {name}, a mouse for Synapse"))),
        ([], [(pid, name)]) => Ok((*pid, format!("0x{pid:04X} {name}, the only Razer device of unknown type connected"))),
        ([], []) if connected.is_empty() => Err("no Razer mouse connected".into()),
        ([], []) => Err(format!("no Razer mouse connected, only {}", list(&connected.iter().collect::<Vec<_>>()))),
        ([], several) => Err(format!("several Razer devices of unknown type connected ({}): set device.pid in config.json", list(several))),
        (several, _) => Err(format!("several Razer mice connected ({}): set device.pid in config.json", list(several))),
    }
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
        if cfg.configured_pid().is_none() {
            let connected = hidapi::HidApi::new().map(|api| crate::razerhid::connected(&api)).unwrap_or_default();
            cfg.detected = Some(choose_mouse(&connected, &crate::extract::categories()));
        }
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

    fn configured_pid(&self) -> Option<u16> {
        u16::from_str_radix(self.device.pid.trim_start_matches("0x").trim_start_matches("0X"), 16).ok().filter(|&p| p != 0)
    }

    /// Il PID del mouse: quello di config.json o quello trovato da solo, altrimenti l'errore.
    pub fn mouse(&self) -> Result<u16, String> {
        match (self.configured_pid(), &self.detected) {
            (Some(pid), _) => Ok(pid),
            (None, Some(Ok((pid, _)))) => Ok(*pid),
            (None, Some(Err(e))) => Err(e.clone()),
            (None, None) => Err("no mouse PID".into()),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cats(list: &[(u16, &str)]) -> HashMap<u16, String> {
        list.iter().map(|(p, c)| (*p, c.to_string())).collect()
    }

    #[test]
    fn picks_the_mouse_never_the_keyboard() {
        let mouse = (0x00CB, "Razer Basilisk V3 35K".to_string());
        let keyboard = (0x028F, "Razer BlackWidow".to_string());
        let (both, known) = (vec![mouse.clone(), keyboard.clone()], cats(&[(0x00CB, "mouse"), (0x028F, "keyboard")]));
        assert_eq!(choose_mouse(&both, &known).unwrap().0, 0x00CB, "mouse e tastiera: il mouse");
        assert_eq!(choose_mouse(&[mouse.clone()], &HashMap::new()).unwrap().0, 0x00CB, "unico collegato, tipo sconosciuto");
        assert!(choose_mouse(&[keyboard.clone()], &known).is_err(), "solo una tastiera: nessun mouse");
        assert_eq!(choose_mouse(&both, &cats(&[(0x028F, "keyboard")])).unwrap().0, 0x00CB, "la tastiera nota esclusa, resta il mouse sconosciuto");
        assert!(choose_mouse(&both, &HashMap::new()).is_err(), "due sconosciuti: non tiro a indovinare");
        let second = (0x0099, "Razer Basilisk V3".to_string());
        assert!(choose_mouse(&[mouse, second], &cats(&[(0x00CB, "mouse"), (0x0099, "mouse")])).is_err(), "due mouse: si sceglie in config.json");
        assert!(choose_mouse(&[], &known).is_err());
    }
}
