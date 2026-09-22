//! Legge dai log di Synapse l'ultima definizione di ogni effetto Chroma Studio e i profili
//! del mouse con il loro slot onboard. Scrive chroma_effects_dump.json e synapse_profiles.json.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const TITLE_KEY: &str = "\"title\":\"";
const PROFILE_KEY: &str = "{\"activeProfile\":";

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

/// Il primo valore JSON all'inizio di `txt`, ignorando quello che segue (il `raw_decode` di Python).
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
        return Err("nessun effetto Chroma nei log di Synapse".into()); // non sovrascrivo un dump buono con uno vuoto
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
        return Err("nessun profilo del mouse nei log di Synapse".into());
    }
    write_json(&dir.join("synapse_profiles.json"), &profiles)?;
    Ok(profiles)
}

/// Riassunto di un effetto, una riga per regione (come `extract_effects.py`).
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
