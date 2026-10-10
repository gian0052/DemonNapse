//! DemonNapse - lightweight companion daemon for Razer Synapse.
//!
//!   demonnapse [-v]        the daemon (no window; log to demonnapse.log if there is no terminal)
//!   demonnapse check [PID]  tests a Razer mouse step by step and prints a report to paste into an issue
//!
//! Tools to verify the daemon piece by piece:
//!   demonnapse render "<effect>" <t0> <frames> <fps> [--layer NAME] [--all] [--clicks S,S] [--live]   frames in hexadecimal
//!   demonnapse probe                                                          firmware and active slot
//!   demonnapse profiles                                                       profile -> matched effect (JSON)
//!   demonnapse audio [seconds]                                               peak of the audio output, 10 readings per second
//!   demonnapse synapse [--kill NAME.exe]                                      Synapse open? admin? state of the Razer services
//!   demonnapse extract [--out FOLDER]                                         effects and profiles from Synapse's logs

#![windows_subsystem = "windows"]

#[macro_use]
mod log;
mod ambient;
mod audio;
mod check;
mod config;
mod daemon;
mod effect;
mod extract;
mod layers;
mod profiles;
mod razerhid;
mod screen;
mod synapse;

use config::Config;
use effect::Effect;
use layers::Canvas;
use serde_json::{json, Value};
use std::path::PathBuf;
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};

/// Folder with config.json and the extracted files: next to the executable or a little above it (the compiled
/// exe is in target/release), otherwise the current one.
fn home() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let found = exe.ancestors().skip(1).take(3).find(|d| d.join("config.json").exists());
    found.map_or_else(|| std::env::current_dir().unwrap_or_default(), PathBuf::from)
}

fn load_json(path: PathBuf) -> Result<Value, String> {
    let txt = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&txt).map_err(|e| format!("{}: {e}", path.display()))
}

fn render(args: &[String]) -> Result<(), String> {
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let only = opt("--layer");
    // --clicks: times of the clicks in seconds, comma-separated (for reactive and on-click playback)
    let clicks_arg = opt("--clicks");
    let pos: Vec<&String> = pos.into_iter().filter(|a| Some(*a) != only.as_ref() && Some(*a) != clicks_arg.as_ref()).collect();
    let [title, t0, frames, fps] = pos[..] else {
        return Err("usage: render \"<effect>\" <t0> <frames> <fps> [--layer NAME] [--all] [--live] [--clicks S,S,...]".into());
    };
    let mut clicks: Vec<f64> = clicks_arg.iter().flat_map(|s| s.split(',')).filter_map(|s| s.trim().parse().ok()).collect();
    clicks.sort_by(f64::total_cmp);
    let (t0, frames, fps): (f64, usize, f64) =
        (t0.parse().map_err(|_| "t0")?, frames.parse().map_err(|_| "frame")?, fps.parse().map_err(|_| "fps")?);

    let dir = home();
    let cfg = Config::load(&dir)?;
    let grid = cfg.grid();
    let canvas = Canvas::new(&grid, cfg.device.leds);
    let dump = load_json(dir.join("chroma_effects_dump.json"))?;
    let mut spec = dump.get(title.as_str()).cloned().ok_or(format!("effect '{title}' not found in the dump"))?;

    if let Some(name) = &only {
        let layer = spec["layers"].as_array().and_then(|ls| ls.iter().find(|l| l["name"] == name.as_str())).cloned();
        let mut layer = layer.ok_or(format!("'{title}' has no layer '{name}'"))?;
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
        eprintln!("layer '{name}' not supported, ignored");
    }
    // --live: in real time, one frame every 1/fps s (for the Ambient, which reads the screen)
    let live = args.iter().any(|a| a == "--live");
    let start = std::time::Instant::now();
    for n in 0..frames {
        if live {
            let due = std::time::Duration::from_secs_f64(n as f64 / fps);
            std::thread::sleep(due.saturating_sub(start.elapsed()));
        }
        let now = t0 + n as f64 / fps;
        while clicks.first().is_some_and(|&c| c <= now) {
            fx.on_click(clicks.remove(0));
        }
        let frame = fx.render(now, 1.0 / fps, 0.0);
        let hex: Vec<String> = frame.iter().map(|c| format!("{:02X}{:02X}{:02X}", c.0, c.1, c.2)).collect();
        println!("{}", hex.join(" "));
    }
    Ok(())
}

fn probe() -> Result<(), String> {
    let cfg = Config::load(&home())?;
    let mut api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let pid = cfg.mouse()?;
    let (ctl, resp) = razerhid::open_control(&mut api, pid, 2)?;
    println!("firmware: {}.{}", resp[8], resp[9]);
    println!("active slot: {:?}", razerhid::get_active_profile(&ctl));
    println!("mode: {:?} (0 = normal, 3 = driver)", razerhid::get_device_mode(&ctl));
    println!("event interfaces: {}", razerhid::open_events(&api, pid).len());
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
saved: {}
", out.join("chroma_effects_dump.json").display());
    let pid = cfg.mouse()?;
    for p in extract::profiles(&out, pid)? {
        println!("slot {}: {}", p.slot, p.name);
    }
    let map = extract::device(&out, pid)?;
    println!("LED map: {} LEDs, {} grid cells", map.leds, map.chroma_grid.len());
    Ok(())
}

fn profiles() -> Result<(), String> {
    let dir = home();
    let cfg = Config::load(&dir)?;
    let grid = cfg.grid();
    let active = profiles::load(&dir, &cfg, &grid, &Canvas::new(&grid, cfg.device.leds), 1)?;
    let out: Vec<Value> = active.iter().map(|a| json!({"slot": a.slot, "name": a.name, "effect": a.effect.title})).collect();
    println!("{}", Value::from(out));
    Ok(())
}

fn audio(args: &[String]) -> Result<(), String> {
    let seconds: f64 = args.first().map_or(Ok(3.0), |s| s.parse()).map_err(|_| "seconds")?;
    let mut meter = audio::PeakMeter::new();
    for _ in 0..(seconds * 10.0) as usize {
        println!("{:.4}", meter.peak());
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(())
}

fn synapse(args: &[String]) -> Result<(), String> {
    if let Some(name) = args.iter().position(|a| a == "--kill").and_then(|i| args.get(i + 1)) {
        synapse::kill(&[name.to_lowercase().as_str()]); // to try the kill on a harmless process
        return Ok(());
    }
    let t = std::time::Instant::now();
    let open = (0..20).fold(false, |_, _| synapse::running());
    println!("synapse open: {open} (check takes {:.2} ms)", t.elapsed().as_secs_f64() * 50.0);
    println!("administrator: {}", synapse::is_admin());
    for (name, state) in synapse::service_states() {
        let state = match state {
            None => "not installed".to_string(),
            Some(1) => "STOPPED".to_string(),
            Some(4) => "RUNNING".to_string(),
            Some(n) => format!("state {n}"),
        };
        println!("{name}: {state}");
    }
    Ok(())
}

fn main() {
    // no window of its own: if a terminal launches me I write to it
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("-v") => {
            log::init(&home().join("demonnapse.log"), !args.is_empty());
            // with panic=abort no trace is left: the reason goes in the log
            std::panic::set_hook(Box::new(|p| info!("fatal error: {p}")));
            daemon::run(&home());
            Ok(())
        }
        Some("render") => render(&args[1..]),
        Some("probe") => probe(),
        Some("extract") => extract(&args[1..]),
        Some("profiles") => profiles(),
        Some("audio") => audio(&args[1..]),
        Some("synapse") => synapse(&args[1..]),
        Some("check") => check::run(&home(), &args[1..]),
        _ => Err("commands: (none = daemon), check, render, probe, extract, profiles, audio, synapse".into()),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
