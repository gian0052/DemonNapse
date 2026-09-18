//! DemonNapse - lightweight companion daemon for Razer Synapse (port in Rust, in corso).
//!
//! Per ora solo gli strumenti per verificare il port pezzo per pezzo:
//!   demonnapse render "<effetto>" <t0> <frame> <fps> [--layer NOME] [--all]   frame in esadecimale
//!   demonnapse probe                                                          firmware e slot attivo
//!   demonnapse extract [--out CARTELLA]                                       effetti e profili dai log di Synapse

mod config;
mod effect;
mod extract;
mod layers;
mod razerhid;

use config::Config;
use effect::Effect;
use layers::Canvas;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Cartella con config.json e i file estratti: accanto all'eseguibile, altrimenti quella corrente.
fn home() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from));
    match exe_dir {
        Some(d) if d.join("config.json").exists() => d,
        _ => std::env::current_dir().unwrap_or_default(),
    }
}

fn load_json(path: PathBuf) -> Result<Value, String> {
    let txt = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&txt).map_err(|e| format!("{}: {e}", path.display()))
}

fn render(args: &[String]) -> Result<(), String> {
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let only = args.iter().position(|a| a == "--layer").and_then(|i| args.get(i + 1)).cloned();
    let pos: Vec<&String> = pos.into_iter().filter(|a| Some(*a) != only.as_ref()).collect();
    let [title, t0, frames, fps] = pos[..] else {
        return Err("uso: render \"<effetto>\" <t0> <frame> <fps> [--layer NOME] [--all]".into());
    };
    let (t0, frames, fps): (f64, usize, f64) =
        (t0.parse().map_err(|_| "t0")?, frames.parse().map_err(|_| "frame")?, fps.parse().map_err(|_| "fps")?);

    let dir = home();
    let cfg = Config::load(&dir)?;
    let grid = cfg.grid();
    let canvas = Canvas::new(&grid, cfg.device.leds);
    let dump = load_json(dir.join("chroma_effects_dump.json"))?;
    let mut spec = dump.get(title.as_str()).cloned().ok_or(format!("effetto '{title}' non trovato nel dump"))?;

    if let Some(name) = &only {
        let layer = spec["layers"].as_array().and_then(|ls| ls.iter().find(|l| l["name"] == name.as_str())).cloned();
        let mut layer = layer.ok_or(format!("'{title}' non ha un layer '{name}'"))?;
        let first = layer["regions"].as_array().and_then(|r| r.first()).cloned().unwrap_or(json!({}));
        layer["regions"] = json!([first]);
        spec["layers"] = json!([layer]);
    }
    if args.iter().any(|a| a == "--all") {
        let mut cells: Vec<&(i64, i64)> = grid.keys().collect();
        cells.sort();
        let everywhere = json!([{ "leds": cells.iter().map(|(r, c)| json!({"row": r, "col": c})).collect::<Vec<_>>() }]);
        for layer in spec["layers"].as_array_mut().into_iter().flatten() {
            for reg in layer["regions"].as_array_mut().into_iter().flatten() {
                reg["devices"] = everywhere.clone();
            }
        }
    }

    let (mut fx, unsupported) = Effect::new(&spec, &cfg, &grid, &canvas, 1);
    for name in unsupported {
        eprintln!("layer '{name}' non supportato, ignorato");
    }
    for n in 0..frames {
        let frame = fx.render(t0 + n as f64 / fps, 1.0 / fps, 0.0);
        let hex: Vec<String> = frame.iter().map(|c| format!("{:02X}{:02X}{:02X}", c.0, c.1, c.2)).collect();
        println!("{}", hex.join(" "));
    }
    Ok(())
}

fn probe() -> Result<(), String> {
    let cfg = Config::load(&home())?;
    let mut api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let (ctl, resp) = razerhid::open_control(&mut api, cfg.pid(), 2)?;
    println!("firmware: {}.{}", resp[8], resp[9]);
    println!("slot attivo: {:?}", razerhid::get_active_profile(&ctl));
    println!("interfacce eventi: {}", razerhid::open_events(&api, cfg.pid()).len());
    Ok(())
}

fn extract(args: &[String]) -> Result<(), String> {
    let dir = home();
    let out = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).map_or(dir.clone(), PathBuf::from);
    let cfg = Config::load(&dir)?;
    for (title, effect) in &extract::effects(&out)? {
        println!("{}", extract::summary(title, effect));
    }
    println!("
salvato: {}
", out.join("chroma_effects_dump.json").display());
    for p in extract::profiles(&out, cfg.pid())? {
        println!("slot {}: {}", p.slot, p.name);
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("render") => render(&args[1..]),
        Some("probe") => probe(),
        Some("extract") => extract(&args[1..]),
        _ => Err("comandi: render, probe, extract (il demone vero arriva nei prossimi passi del port)".into()),
    };
    if let Err(e) = result {
        eprintln!("errore: {e}");
        std::process::exit(1);
    }
}
