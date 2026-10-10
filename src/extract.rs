//! Legge dai log di Synapse l'ultima definizione di ogni effetto Chroma Studio, i profili
//! del mouse con il loro slot onboard e la mappa dei LED del mouse. Scrive chroma_effects_dump.json,
//! synapse_profiles.json e synapse_device.json.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const TITLE_KEY: &str = "\"title\":\"";
const PROFILE_KEY: &str = "{\"activeProfile\":";
const DEVICE_KEY: &str = "\"config\":{\"Version\":";
const CATEGORY_KEY: &str = "\"category\":\"";

fn logs_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join(r"Razer\RazerAppEngine\User Data\Logs")
}

/// Il testo di tutti i log `<prefisso>*.log`, dal piu' vecchio al piu' recente: cosi' l'ultima occorrenza vince.
fn read_logs(prefix: &str) -> String {
    let prefix = prefix.to_lowercase();
    let mut files: Vec<_> = std::fs::read_dir(logs_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.starts_with(&prefix) && name.ends_with(".log")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort();
    files.iter().filter_map(|(_, p)| std::fs::read(p).ok()).map(|b| String::from_utf8_lossy(&b).into_owned()).collect()
}

/// Il primo valore JSON all'inizio di `txt`, ignorando quello che segue.
fn decode_prefix(txt: &str) -> Option<Value> {
    serde_json::Deserializer::from_str(txt).into_iter::<Value>().next()?.ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, serde_json::ser::PrettyFormatter::with_indent(b" "));
    value.serialize(&mut ser).map_err(|e| e.to_string())?;
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// {titolo: effetto}, scritto anche in `dir`/chroma_effects_dump.json.
pub fn effects(dir: &Path) -> Result<Map<String, Value>, String> {
    let txt = read_logs("chroma-studio");
    let mut effects = Map::new();
    let mut from = 0;
    while let Some(i) = txt[from..].find(TITLE_KEY) {
        let at = from + i;
        from = at + TITLE_KEY.len();
        let Some(title) = txt[from..].find('"').map(|n| &txt[from..from + n]).filter(|t| !t.is_empty()) else {
            continue;
        };
        let mut start = at;
        for _ in 0..6 {
            // risale all'oggetto JSON che contiene il titolo
            let Some(s) = txt[..start].rfind('{') else { break };
            start = s;
            // un effetto e' l'oggetto con i layer e con proprio quel titolo: cosi' i titoli dei layer
            // (STATIC, WAVE...) non diventano effetti e un effetto rinominato viene letto lo stesso
            if let Some(obj) = decode_prefix(&txt[start..]).filter(|o| o.get("layers").is_some() && o["title"] == title) {
                effects.insert(title.to_string(), obj);
                break;
            }
        }
    }
    if effects.is_empty() {
        return Err("no Chroma effect in Synapse's logs".into()); // non sovrascrivo un dump buono con uno vuoto
    }
    write_json(&dir.join("chroma_effects_dump.json"), &effects)?;
    Ok(effects)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub name: String,
    pub slot: u8,
}

/// Profili Synapse del mouse con il loro slot onboard, dai log del modulo mouse (`pid` in decimale
/// nel nome del file). Scritti anche in `dir`/synapse_profiles.json.
pub fn profiles(dir: &Path, pid: u16) -> Result<Vec<Profile>, String> {
    let txt = read_logs(&format!("products_{pid}_mw"));
    let mut last: Vec<(String, Profile)> = Vec::new(); // per guid, nell'ordine in cui compaiono
    let mut from = 0;
    while let Some(i) = txt[from..].find(PROFILE_KEY) {
        let at = from + i;
        from = at + PROFILE_KEY.len();
        let Some(obj) = decode_prefix(&txt[at..]) else { continue };
        let p = &obj["activeProfile"];
        let (Some(name), Some(slot), Some(guid)) = (p["name"].as_str(), p["obmSlotId"][0].as_u64(), p["guid"].as_str()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let profile = Profile { name: name.to_string(), slot: slot as u8 };
        match last.iter_mut().find(|(g, _)| g == guid) {
            Some(entry) => entry.1 = profile,
            None => last.push((guid.to_string(), profile)),
        }
    }
    // se due profili risultano sullo stesso slot vince l'ultimo visto
    let by_slot: BTreeMap<u8, Profile> = last.into_iter().map(|(_, p)| (p.slot, p)).collect();
    let profiles: Vec<Profile> = by_slot.into_values().collect();
    if profiles.is_empty() {
        return Err("no mouse profile in Synapse's logs".into());
    }
    write_json(&dir.join("synapse_profiles.json"), &profiles)?;
    Ok(profiles)
}

/// Il mouse come lo vede Chroma Studio: PID, numero di LED e griglia "riga,colonna" -> indice LED fisico.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct DeviceMap {
    #[serde(default)]
    pub pid: u16, // il mouse a cui appartiene la mappa
    pub leds: usize,
    pub chroma_grid: BTreeMap<String, usize>,
}

/// L'ultima scheda del dispositivo `pid` in `txt`. Synapse la scrive come
/// `"config":{"Version":2,"VID":5426,"PID":203,...,"DeviceMaxRow":1,"DeviceMaxCol":11,...,
/// "LedMatrix":[{"MatrixPos":[riga,colonna],"DevicePos":[riga,colonna]},...]}`.
fn device_from(txt: &str, pid: u16) -> Option<DeviceMap> {
    let mut found = None;
    let mut from = 0;
    while let Some(i) = txt[from..].find(DEVICE_KEY) {
        let at = from + i + "\"config\":".len();
        from = at;
        let Some(cfg) = decode_prefix(&txt[at..]) else { continue };
        if cfg["VID"].as_u64() != Some(crate::razerhid::VID as u64) || cfg["PID"].as_u64() != Some(pid as u64) {
            continue;
        }
        let (rows, cols) = (cfg["DeviceMaxRow"].as_u64().unwrap_or(0) as usize, cfg["DeviceMaxCol"].as_u64().unwrap_or(0) as usize);
        let leds = rows * cols;
        let mut grid = BTreeMap::new();
        for cell in cfg["LedMatrix"].as_array().into_iter().flatten() {
            let pos = |key: &str| Some((cell[key][0].as_u64()? as usize, cell[key][1].as_u64()? as usize));
            let (Some((r, c)), Some((dr, dc))) = (pos("MatrixPos"), pos("DevicePos")) else { continue };
            let led = dr * cols + dc;
            if led < leds {
                grid.insert(format!("{r},{c}"), led);
            }
        }
        if !grid.is_empty() {
            found = Some(DeviceMap { pid, leds, chroma_grid: grid });
        }
    }
    found
}

/// La mappa dei LED del mouse `pid` dai log di Chroma Studio e del motore luci, scritta anche in
/// `dir`/synapse_device.json.
pub fn device(dir: &Path, pid: u16) -> Result<DeviceMap, String> {
    let map = ["chroma-studio", "lighting-engine"]
        .iter()
        .find_map(|prefix| device_from(&read_logs(prefix), pid))
        .ok_or_else(|| format!("no LED map for the mouse (PID 0x{pid:04X}) in Synapse's logs"))?;
    write_json(&dir.join("synapse_device.json"), &map)?;
    Ok(map)
}

/// Il tipo di ogni dispositivo come lo scrive Synapse nella sua scheda
/// (`{..."productId":203,...,"category":"mouse",...}`), per PID; l'ultima occorrenza vince.
fn categories_from(txt: &str) -> HashMap<u16, String> {
    let mut found = HashMap::new();
    let mut from = 0;
    while let Some(i) = txt[from..].find(CATEGORY_KEY) {
        let at = from + i;
        from = at + CATEGORY_KEY.len();
        let Some(category) = txt[from..].find('"').map(|n| &txt[from..from + n]) else { break };
        // il productId sta nello stesso oggetto, prima della categoria
        let start = txt[..at].rfind('{').unwrap_or(0);
        let Some(p) = txt[start..at].rfind("\"productId\":") else { continue };
        let digits: String = txt[start + p + "\"productId\":".len()..at].chars().take_while(char::is_ascii_digit).collect();
        if let Ok(pid) = digits.parse::<u16>() {
            found.insert(pid, category.to_string());
        }
    }
    found
}

/// Il tipo di ogni dispositivo Razer che Synapse conosce ("mouse", "keyboard"...), per PID.
pub fn categories() -> HashMap<u16, String> {
    let mut all = categories_from(&read_logs("lighting-engine"));
    all.extend(categories_from(&read_logs("chroma-studio")));
    all
}

/// Riassunto di un effetto, una riga per regione.
pub fn summary(title: &str, effect: &Value) -> String {
    let mut out = format!("\n== {title}");
    for layer in effect["layers"].as_array().into_iter().flatten() {
        let vis = if layer["visible"].as_bool().unwrap_or(false) { "ON " } else { "off" };
        for reg in layer["regions"].as_array().into_iter().flatten() {
            let n: usize = reg["devices"].as_array().into_iter().flatten().map(|d| d["leds"].as_array().map_or(0, Vec::len)).sum();
            let params = reg.get("params").cloned().unwrap_or(json!({}));
            out += &format!("\n  [{vis}] {:<16} {n:>2} LED  {params}", layer["title"].as_str().unwrap_or(""));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_map_from_the_log() {
        // come la scrive Synapse per il Basilisk V3 35K (ridotta a 3 LED), preceduta da un altro dispositivo
        let other = r#""config":{"Version":2,"VID":5426,"PID":99,"DeviceMaxRow":1,"DeviceMaxCol":2,"LedMatrix":[{"MatrixPos":[0,0],"DevicePos":[0,0]}]}"#;
        let mouse = r#""config":{"Version":2,"VID":5426,"PID":203,"EID":0,"MatrixMaxRow":7,"MatrixMaxCol":5,"DeviceMaxRow":1,"DeviceMaxCol":3,"LedMatrix":[{"MatrixPos":[0,2],"DevicePos":[0,1]},{"MatrixPos":[5,2],"DevicePos":[0,0]},{"MatrixPos":[6,4],"DevicePos":[0,2]},{"MatrixPos":[6,0],"DevicePos":[0,7]}]}"#;
        let map = device_from(&format!("x {mouse} y {other} z"), 203).unwrap();
        assert_eq!(map.leds, 3);
        let want: BTreeMap<String, usize> = [("0,2", 1), ("5,2", 0), ("6,4", 2)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
        assert_eq!(map.chroma_grid, want, "il LED 7 non esiste su un mouse da 3 LED: scartato");
        assert!(device_from(mouse, 0x00CC).is_none());
    }

    #[test]
    fn categories_from_the_log() {
        let txt = r#"{"positionRank":4,"productId":203,"instanceId":0,"category":"mouse","productName":"Razer Basilisk V3 35K"} {"productId":655,"layout":1,"category":"keyboard"} {"category":"headset"}"#;
        let c = categories_from(txt);
        assert_eq!(c.get(&203).map(String::as_str), Some("mouse"));
        assert_eq!(c.get(&655).map(String::as_str), Some("keyboard"));
        assert_eq!(c.len(), 2, "una categoria senza productId non conta");
    }
}
