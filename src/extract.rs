//! Reads from Synapse's logs the latest definition of every Chroma Studio effect, the mouse's
//! profiles with their on-board slot and the mouse's LED map. Writes chroma_effects_dump.json,
//! synapse_profiles.json and synapse_device.json.

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

/// The text of all the `<prefix>*.log` logs, from the oldest to the newest: that way the last occurrence wins.
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

/// The first JSON value at the start of `txt`, ignoring what follows.
fn decode_prefix(txt: &str) -> Option<Value> {
    serde_json::Deserializer::from_str(txt).into_iter::<Value>().next()?.ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, serde_json::ser::PrettyFormatter::with_indent(b" "));
    value.serialize(&mut ser).map_err(|e| e.to_string())?;
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// {title: effect}, also written to `dir`/chroma_effects_dump.json.
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
            // walks back to the JSON object that contains the title
            let Some(s) = txt[..start].rfind('{') else { break };
            start = s;
            // an effect is the object with the layers and with exactly that title: that way the layers' titles
            // (STATIC, WAVE...) do not become effects and a renamed effect is read all the same
            if let Some(obj) = decode_prefix(&txt[start..]).filter(|o| o.get("layers").is_some() && o["title"] == title) {
                effects.insert(title.to_string(), obj);
                break;
            }
        }
    }
    if effects.is_empty() {
        return Err("no Chroma effect in Synapse's logs".into()); // I do not overwrite a good dump with an empty one
    }
    write_json(&dir.join("chroma_effects_dump.json"), &effects)?;
    Ok(effects)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub name: String,
    pub slot: u8,
}

/// The mouse's Synapse profiles with their on-board slot, from the mouse module's logs (`pid` in decimal
/// in the file name). Also written to `dir`/synapse_profiles.json.
pub fn profiles(dir: &Path, pid: u16) -> Result<Vec<Profile>, String> {
    let txt = read_logs(&format!("products_{pid}_mw"));
    let mut last: Vec<(String, Profile)> = Vec::new(); // by guid, in the order they appear
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
    // if two profiles turn out to be on the same slot the last one seen wins
    let by_slot: BTreeMap<u8, Profile> = last.into_iter().map(|(_, p)| (p.slot, p)).collect();
    let profiles: Vec<Profile> = by_slot.into_values().collect();
    if profiles.is_empty() {
        return Err("no mouse profile in Synapse's logs".into());
    }
    write_json(&dir.join("synapse_profiles.json"), &profiles)?;
    Ok(profiles)
}

/// The mouse as Chroma Studio sees it: PID, number of LEDs and grid "row,column" -> physical LED index.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct DeviceMap {
    #[serde(default)]
    pub pid: u16, // the mouse the map belongs to
    pub leds: usize,
    pub chroma_grid: BTreeMap<String, usize>,
}

/// The last record of device `pid` in `txt`. Synapse writes it as
/// `"config":{"Version":2,"VID":5426,"PID":203,...,"DeviceMaxRow":1,"DeviceMaxCol":11,...,
/// "LedMatrix":[{"MatrixPos":[row,column],"DevicePos":[row,column]},...]}`.
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

/// The LED map of mouse `pid` from the logs of Chroma Studio and of the lighting engine, also written to
/// `dir`/synapse_device.json.
pub fn device(dir: &Path, pid: u16) -> Result<DeviceMap, String> {
    let map = ["chroma-studio", "lighting-engine"]
        .iter()
        .find_map(|prefix| device_from(&read_logs(prefix), pid))
        .ok_or_else(|| format!("no LED map for the mouse (PID 0x{pid:04X}) in Synapse's logs"))?;
    write_json(&dir.join("synapse_device.json"), &map)?;
    Ok(map)
}

/// The type of every device as Synapse writes it in its record
/// (`{..."productId":203,...,"category":"mouse",...}`), by PID; the last occurrence wins.
fn categories_from(txt: &str) -> HashMap<u16, String> {
    let mut found = HashMap::new();
    let mut from = 0;
    while let Some(i) = txt[from..].find(CATEGORY_KEY) {
        let at = from + i;
        from = at + CATEGORY_KEY.len();
        let Some(category) = txt[from..].find('"').map(|n| &txt[from..from + n]) else { break };
        // the productId is in the same object, before the category
        let start = txt[..at].rfind('{').unwrap_or(0);
        let Some(p) = txt[start..at].rfind("\"productId\":") else { continue };
        let digits: String = txt[start + p + "\"productId\":".len()..at].chars().take_while(char::is_ascii_digit).collect();
        if let Ok(pid) = digits.parse::<u16>() {
            found.insert(pid, category.to_string());
        }
    }
    found
}

/// The type of every Razer device Synapse knows ("mouse", "keyboard"...), by PID.
pub fn categories() -> HashMap<u16, String> {
    let mut all = categories_from(&read_logs("lighting-engine"));
    all.extend(categories_from(&read_logs("chroma-studio")));
    all
}

/// Summary of an effect, one line per region.
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
        // as Synapse writes it for the Basilisk V3 35K (cut down to 3 LEDs), followed by another device
        let other = r#""config":{"Version":2,"VID":5426,"PID":99,"DeviceMaxRow":1,"DeviceMaxCol":2,"LedMatrix":[{"MatrixPos":[0,0],"DevicePos":[0,0]}]}"#;
        let mouse = r#""config":{"Version":2,"VID":5426,"PID":203,"EID":0,"MatrixMaxRow":7,"MatrixMaxCol":5,"DeviceMaxRow":1,"DeviceMaxCol":3,"LedMatrix":[{"MatrixPos":[0,2],"DevicePos":[0,1]},{"MatrixPos":[5,2],"DevicePos":[0,0]},{"MatrixPos":[6,4],"DevicePos":[0,2]},{"MatrixPos":[6,0],"DevicePos":[0,7]}]}"#;
        let map = device_from(&format!("x {mouse} y {other} z"), 203).unwrap();
        assert_eq!(map.leds, 3);
        let want: BTreeMap<String, usize> = [("0,2", 1), ("5,2", 0), ("6,4", 2)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
        assert_eq!(map.chroma_grid, want, "LED 7 does not exist on a 3-LED mouse: discarded");
        assert!(device_from(mouse, 0x00CC).is_none());
    }

    #[test]
    fn categories_from_the_log() {
        let txt = r#"{"positionRank":4,"productId":203,"instanceId":0,"category":"mouse","productName":"Razer Basilisk V3 35K"} {"productId":655,"layout":1,"category":"keyboard"} {"category":"headset"}"#;
        let c = categories_from(txt);
        assert_eq!(c.get(&203).map(String::as_str), Some("mouse"));
        assert_eq!(c.get(&655).map(String::as_str), Some("keyboard"));
        assert_eq!(c.len(), 2, "a category without productId does not count");
    }
}
