//! `demonnapse check [PID]`: tests step by step whether a Razer mouse works with DemonNapse and prints
//! a report to paste into an issue. It is for people with a model other than the Basilisk V3 35K.
//! Reads only, plus switching the active slot (like the button, then back as it was) and a few
//! colours on the LEDs: nothing is written to the mouse's memory.

use crate::config::Config;
use crate::layers::Rgb;
use crate::{extract, razerhid, synapse};
use hidapi::{HidApi, HidDevice};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

const BUTTON_SECONDS: u64 = 15;

fn yes(ok: bool) -> &'static str {
    if ok {
        "OK"
    } else {
        "NO"
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// Raw response to a read command, if the mouse answers "ok" (status 0x02).
fn query(h: &HidDevice, class: u8, id: u8, size: u8) -> Option<[u8; 90]> {
    razerhid::transact(h, &razerhid::build(class, id, size, &[])).ok().filter(|r| r[0] == 0x02)
}

fn show(h: &HidDevice, leds: usize, color: impl Fn(usize) -> Rgb, seconds: f64) -> bool {
    let frame: Vec<Rgb> = (0..leds).map(color).collect();
    let start = Instant::now();
    let mut ok = true;
    while start.elapsed().as_secs_f64() < seconds {
        ok &= razerhid::custom_frame(h, &frame).is_ok();
        sleep(Duration::from_millis(100));
    }
    ok
}

pub fn run(dir: &Path, args: &[String]) -> Result<(), String> {
    // the same lock as the daemon: two programs on the same mouse get each other's responses mixed up
    let _mutex = unsafe { CreateMutexW(None, true, w!("Local\\demonnapse")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Err("the daemon is running: stop it first (schtasks /End /TN DemonNapse), then run check again".into());
    }
    if synapse::running() {
        return Err("Synapse is open: close it completely (tray -> Exit), then run check again".into());
    }

    println!("DemonNapse check {}", env!("CARGO_PKG_VERSION"));
    println!("\n[1] Razer devices connected");
    let mut api = HidApi::new().map_err(|e| e.to_string())?;
    let seen = razerhid::connected(&api);
    let categories = extract::categories();
    for (pid, name) in &seen {
        println!("  PID 0x{pid:04X}  {name}  ({})", categories.get(pid).map_or("type unknown to Synapse", String::as_str));
    }
    let cfg = Config::load(dir).ok();
    let pid = match args.first() {
        Some(a) => u16::from_str_radix(a.trim_start_matches("0x").trim_start_matches("0X"), 16).map_err(|_| format!("'{a}' is not a PID like 0x00CB"))?,
        None => cfg.as_ref().ok_or("config.json not found")?.mouse().map_err(|e| format!("{e} (or run check 0x00XX with the PID above)"))?,
    };
    let name = seen.iter().find(|(p, _)| *p == pid).map_or("", |(_, n)| n.as_str());
    let how = match (args.first(), cfg.as_ref().and_then(|c| c.detected.as_ref())) {
        (Some(_), _) => "given on the command line",
        (None, Some(Ok(_))) => "detected",
        _ => "from config.json",
    };
    println!("  checking PID 0x{pid:04X} {name} ({how})");

    println!("\n[2] Control interface");
    let (ctl, resp) = razerhid::open_control(&mut api, pid, 2).map_err(|e| format!("{e}: this device does not answer the Razer protocol the way DemonNapse speaks it"))?;
    println!("  transaction id: 0x{:02X}", razerhid::transaction_id());
    println!("  firmware: {}.{}", resp[8], resp[9]);
    println!("  device mode: {:?} (0 = normal, 3 = driver)", razerhid::get_device_mode(&ctl));

    println!("\n[3] Synapse's logs");
    let tmp = std::env::temp_dir().join("demonnapse-check");
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let map = extract::device(&tmp, pid);
    match &map {
        Ok(m) => println!("  LED map: OK, {} LEDs, {} grid cells", m.leds, m.chroma_grid.len()),
        Err(e) => println!("  LED map: NO ({e})"),
    }
    match extract::profiles(&tmp, pid) {
        Ok(ps) => println!("  profiles: OK, {}", ps.iter().map(|p| format!("slot {} '{}'", p.slot, p.name)).collect::<Vec<_>>().join(", ")),
        Err(e) => println!("  profiles: NO ({e})"),
    }
    match extract::effects(&tmp) {
        Ok(fx) => println!("  Chroma Studio effects: OK, {}", fx.len()),
        Err(e) => println!("  Chroma Studio effects: NO ({e})"),
    }

    println!("\n[4] On-board profiles");
    let count = query(&ctl, 0x05, 0x80, 0x01);
    let slots = query(&ctl, 0x05, 0x81, 0x0A);
    let active = razerhid::get_active_profile(&ctl);
    println!("  number of profiles (05/80): {}", count.map_or("NO".into(), |r| hex(&r[8..10])));
    println!("  slots (05/81): {}", slots.map_or("NO".into(), |r| hex(&r[8..18])));
    println!("  active slot (05/84): {}", active.map_or("NO".into(), |s| s.to_string()));
    let mut switch_ok = None;
    if let (Some(from), Some(r)) = (active, slots) {
        // 05/81 = [capacity, slots...]: I try switching to another slot and back
        // preferably not slot 1 (Hybrid Storage on the 35K): the daemon jumps towards the normal slots
        let list: Vec<u8> = r[9..9 + (r[8] as usize).min(8)].iter().copied().filter(|&s| s != 0 && s != from).collect();
        if let Some(to) = list.iter().copied().find(|&s| s != 1).or(list.first().copied()) {
            let go = |slot: u8| razerhid::set_active_profile(&ctl, slot) && { sleep(Duration::from_millis(300)); razerhid::get_active_profile(&ctl) == Some(slot) };
            // a second attempt: if someone presses the button in the meantime the first one fails
            let ok = (0..2).any(|_| go(to) && go(from));
            if !ok {
                razerhid::set_active_profile(&ctl, from);
            }
            switch_ok = Some(ok);
            println!("  switch slot {from} -> {to} -> {from} (05/04): {}", yes(ok));
        }
    }

    println!("\n[5] Profile button: press the profile button under the mouse now ({BUTTON_SECONDS} s)");
    let events = razerhid::open_events(&api, pid);
    let mut buf = [0u8; 64];
    let (mut n_events, mut slots_seen) = (0, vec![active]);
    let start = Instant::now();
    let mut last_poll = Instant::now();
    while start.elapsed().as_secs() < BUTTON_SECONDS {
        for h in &events {
            while let Ok(n @ 1..) = h.read_timeout(&mut buf, 1) {
                if n >= 2 && buf[0] == 0x05 {
                    n_events += 1;
                    println!("  event {}", hex(&buf[..n.min(6)]));
                }
            }
        }
        if last_poll.elapsed().as_millis() >= 250 {
            last_poll = Instant::now();
            let s = razerhid::get_active_profile(&ctl);
            if s.is_some() && slots_seen.last() != Some(&s) {
                println!("  active slot -> {}", s.unwrap_or(0));
                slots_seen.push(s);
            }
        }
        sleep(Duration::from_millis(5));
    }
    let button_ok = slots_seen.len() > 1;
    println!("  event interfaces: {}, \"05 xx\" events: {n_events}, slot changed: {}", events.len(), yes(button_ok));
    if let (Some(from), true) = (active, button_ok) {
        razerhid::set_active_profile(&ctl, from); // I put the starting slot back
    }

    println!("\n[6] LEDs: watch the mouse (red, green, blue, then one LED at a time in white)");
    let leds = map.as_ref().map_or(0, |m| m.leds);
    let mut led_ok = None;
    if leds > 0 {
        let mut ok = show(&ctl, leds, |_| (255, 0, 0), 1.0);
        ok &= show(&ctl, leds, |_| (0, 255, 0), 1.0);
        ok &= show(&ctl, leds, |_| (0, 0, 255), 1.0);
        for n in 0..leds {
            ok &= show(&ctl, leds, |i| if i == n { (255, 255, 255) } else { (0, 0, 0) }, 0.5);
        }
        ok &= show(&ctl, leds, |_| (0, 0, 0), 0.2);
        led_ok = Some(ok);
        println!("  per-LED frames (0F/03) sent: {}. Did every LED light up in turn? Say so in the report.", yes(ok));
    } else {
        println!("  skipped: no LED count (no LED map in Synapse's logs)");
    }

    println!("\n== Summary (copy everything from \"DemonNapse check\" into a GitHub issue) ==");
    println!("device: PID 0x{pid:04X} {name}, firmware {}.{}, transaction id 0x{:02X}", resp[8], resp[9], razerhid::transaction_id());
    println!("LED map from Synapse: {}", yes(map.is_ok()));
    println!("read active slot: {}", yes(active.is_some()));
    println!("switch slot: {}", switch_ok.map_or("not tried (one slot only?)", yes));
    println!("profile button seen: {}", yes(button_ok));
    println!("per-LED frames: {}", led_ok.map_or("not tried", yes));
    println!("Restart the daemon afterwards: schtasks /Run /TN DemonNapse");
    Ok(())
}
