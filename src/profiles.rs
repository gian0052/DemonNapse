//! Abbina a ogni profilo Synapse il suo effetto Chroma e prepara gli effetti da disegnare.

use crate::config::Config;
use crate::effect::Effect;
use crate::extract::Profile;
use crate::layers::Canvas;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

fn tokens(name: &str) -> HashSet<String> {
    let name = name.to_lowercase();
    let name = name.strip_prefix("chroma effect").unwrap_or(&name);
    name.split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()).map(String::from).collect()
}

/// Effetto Chroma per un profilo Synapse: override esplicito, poi parole in comune nei nomi.
pub fn match_effect<'a>(profile: &str, titles: impl Iterator<Item = &'a String>, overrides: &'a HashMap<String, String>) -> Option<&'a String> {
    if let Some(title) = overrides.get(profile) {
        return Some(title);
    }
    let want = tokens(profile);
    let (mut best, mut score) = (None, 0);
    for t in titles {
        let common = want.intersection(&tokens(t)).count();
        if common > score {
            (best, score) = (Some(t), common);
        }
    }
    best
}

pub struct Active {
    pub slot: u8,
    pub name: String,
    pub effect: Effect,
}

/// I profili da seguire (in ordine di slot, senza quelli in `skip_slots`), ognuno col suo effetto.
pub fn load(dir: &Path, cfg: &Config, grid: &HashMap<(i64, i64), usize>, canvas: &Canvas, seed: u64) -> Result<Vec<Active>, String> {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"));
    let dump: Map<String, Value> = serde_json::from_str(&read("chroma_effects_dump.json")?).map_err(|e| format!("chroma_effects_dump.json: {e}"))?;
    let synapse: Vec<Profile> = serde_json::from_str(&read("synapse_profiles.json")?).map_err(|e| format!("synapse_profiles.json: {e}"))?;
    let mut active = Vec::new();
    for p in synapse.into_iter().filter(|p| !cfg.skip_slots.contains(&p.slot)) {
        let effect = match match_effect(&p.name, dump.keys(), &cfg.effect_overrides).and_then(|t| dump.get(t)) {
            Some(spec) => {
                let (fx, unsupported) = Effect::new(spec, cfg, grid, canvas, seed + p.slot as u64);
                for layer in unsupported {
                    info!("{}: layer '{layer}' not supported, ignored", fx.title);
                }
                fx
            }
            None => {
                info!("profile '{}' (slot {}): no matching Chroma effect, RGB off", p.name, p.slot);
                Effect::off(cfg, canvas)
            }
        };
        info!("slot {}  {:<18} -> {}", p.slot, p.name, effect.title);
        active.push(Active { slot: p.slot, name: p.name, effect });
    }
    if active.is_empty() {
        return Err("no Synapse profile found: open Synapse once and close it".into());
    }
    Ok(active)
}
