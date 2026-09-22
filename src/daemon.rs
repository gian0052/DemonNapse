//! Il demone: capisce il profilo onboard attivo dagli eventi HID del mouse e disegna l'effetto
//! Chroma abbinato. Quando Synapse si apre gli lascia il mouse, quando si chiude rilegge gli
//! effetti dai suoi log e riprende.

use crate::audio::PeakMeter;
use crate::config::Config;
use crate::layers::{Canvas, Rgb};
use crate::profiles::Active;
use crate::{extract, profiles, razerhid, synapse};
use hidapi::{HidApi, HidDevice};
use std::net::UdpSocket;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const VK_BUTTONS: [i32; 5] = [0x01, 0x02, 0x04, 0x05, 0x06]; // sx, dx, centrale, laterali
const WALL_ADDR: &str = "127.0.0.1:47311"; // lo sfondo (wallpaper.py) ascolta qui
const CONTROL_RETRIES: u32 = 5;

/// Legge lo slot attivo dal mouse; gli slot non configurati (es. la Hybrid Storage) vengono
/// saltati passando al successivo configurato.
struct ProfileTracker {
    slots: Vec<u8>,
    idx: usize,
    last_poll: f64,
}

impl ProfileTracker {
    fn new(ctl: &HidDevice, slots: Vec<u8>, now: f64) -> ProfileTracker {
        let mut t = ProfileTracker { slots, idx: 0, last_poll: 0.0 };
        t.refresh(ctl, now);
        t
    }

    /// Ritorna true se il profilo attivo e' cambiato.
    fn refresh(&mut self, ctl: &HidDevice, now: f64) -> bool {
        self.last_poll = now;
        let Some(mut slot) = razerhid::get_active_profile(ctl) else { return false };
        if !self.slots.contains(&slot) {
            let next = self.slots.iter().copied().find(|&s| s > slot).unwrap_or(self.slots[0]);
            info!("slot {slot} not configured -> jumping to slot {next}");
            razerhid::set_active_profile(ctl, next);
            slot = next;
        }
        let new = self.slots.iter().position(|&s| s == slot).unwrap_or(0);
        let changed = new != self.idx;
        self.idx = new;
        changed
    }

    /// Qualunque evento "05 xx" (DPI, rotella, cambio profilo) -> rilegge lo slot.
    fn feed(&mut self, ctl: &HidDevice, report: &[u8], now: f64) -> bool {
        report.len() >= 2 && report[0] == 0x05 && self.refresh(ctl, now)
    }
}

/// Effetti e profili dai log di Synapse, scritti in `dir`.
fn extract_all(dir: &Path) -> Result<(), String> {
    let cfg = Config::load(dir)?;
    extract::effects(dir)?;
    extract::profiles(dir, cfg.pid()).map(drop)
}

/// Synapse e' aperto: gli lascio il mouse e i servizi, e aspetto.
fn wait_for_synapse_to_close(dir: &Path) {
    info!("Synapse opened: pausing");
    synapse::services(true);
    while synapse::running() {
        sleep(Duration::from_secs(1));
    }
    info!("Synapse closed: re-reading effects and resuming");
    if let Err(e) = extract_all(dir) {
        info!("reading effects failed, keeping the previous ones: {e}");
    }
    sleep(Duration::from_secs(2)); // lascio finire i processi figli di Synapse
    synapse::kill_razer_apps();
    synapse::services(false);
}

/// Una "sessione" tra un'apertura di Synapse e l'altra. Ritorna Ok quando Synapse compare.
fn session(dir: &Path, clock: Instant, meter: &mut PeakMeter, wall: Option<&UdpSocket>, last_error: &mut String) -> Result<(), String> {
    // prima installazione: i file estratti non ci sono ancora, li leggo subito dai log di Synapse
    if !dir.join("chroma_effects_dump.json").exists() || !dir.join("synapse_profiles.json").exists() {
        extract_all(dir)?;
        info!("effects and profiles read from Synapse's logs");
    }
    let cfg = Config::load(dir)?;
    let grid = cfg.grid();
    let canvas = Canvas::new(&grid, cfg.device.leds);
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
    let mut active: Vec<Active> = profiles::load(dir, &cfg, &grid, &canvas, seed)?;

    let mut api = HidApi::new().map_err(|e| e.to_string())?;
    let (ctl, _) = razerhid::open_control(&mut api, cfg.pid(), CONTROL_RETRIES)?;
    // Synapse chiuso di forza lascia il mouse in modo driver: il pulsante dei profili non cambia piu' slot
    if let Some(mode @ 1..) = razerhid::get_device_mode(&ctl) {
        let ok = razerhid::set_device_mode(&ctl, 0);
        info!("mouse in mode {mode} (driver): set back to normal mode{}", if ok { "" } else { " - FAILED" });
    }
    let events = razerhid::open_events(&api, cfg.pid());
    let now = || clock.elapsed().as_secs_f64();
    let mut tracker = ProfileTracker::new(&ctl, active.iter().map(|a| a.slot).collect(), now());
    info!("active: slot {} -> profile '{}'", active[tracker.idx].slot, active[tracker.idx].name);
    last_error.clear();

    let mut pressed = [false; VK_BUTTONS.len()];
    let tick = 1.0 / cfg.fps;
    let mut last = now();
    let mut last_frame: Option<Vec<Rgb>> = None;
    let (mut last_sent, mut last_syn) = (0.0, 0.0);
    let mut buf = [0u8; 64];

    loop {
        let t = now();
        let dt = t - last;
        last = t;

        if t - last_syn > 1.5 {
            last_syn = t;
            if synapse::running() {
                return Ok(()); // Synapse e' comparso: mollo il mouse
            }
        }

        let mut changed = false;
        for h in &events {
            // timeout di 1 ms e non lettura non bloccante: con timeout 0 il backend Windows di hidapi
            // prende ERROR_IO_INCOMPLETE per un errore, abbandona la lettura in corso e perde gli eventi
            while let Ok(n @ 1..) = h.read_timeout(&mut buf, 1) {
                debug!("event {}", buf[..n.min(6)].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "));
                changed |= tracker.feed(&ctl, &buf[..n], t);
            }
        }
        if t - tracker.last_poll > 30.0 {
            changed |= tracker.refresh(&ctl, t); // rete di sicurezza se un evento si perde
        }
        if changed {
            for (k, a) in active.iter_mut().enumerate() {
                if k != tracker.idx {
                    a.effect.sleep();
                }
            }
        }
        let current = &mut active[tracker.idx];
        if changed {
            current.effect.clear_flashes();
            last_frame = None;
            info!("profile -> '{}' ({})", current.name, current.effect.title);
        }
        let fx = &mut current.effect;

        if fx.needs_clicks() {
            for (n, &vk) in VK_BUTTONS.iter().enumerate() {
                let down = unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0;
                if down && !pressed[n] {
                    fx.on_click(t);
                }
                pressed[n] = down;
            }
        }

        let peak = if fx.has_audio() { meter.peak() } else { 0.0 };
        let frame = fx.render(t, dt, peak);
        // reinvio solo se cambia; keepalive ogni secondo (il firmware puo' ridisegnare il suo effetto)
        if last_frame.as_ref() != Some(&frame) || t - last_sent > 1.0 {
            razerhid::custom_frame(&ctl, &frame).map_err(|e| format!("sending to the mouse failed: {e}"))?;
            if let Some(wall) = wall {
                let bytes: Vec<u8> = frame.iter().flat_map(|c| [c.0, c.1, c.2]).collect();
                let _ = wall.send_to(&bytes, WALL_ADDR);
            }
            last_frame = Some(frame);
            last_sent = t;
        }

        sleep(Duration::from_secs_f64((tick - (now() - t)).max(0.0)));
    }
}

pub fn run(dir: &Path) {
    // una sola istanza: due demoni sullo stesso mouse si pestano i piedi
    let mutex = unsafe { CreateMutexW(None, true, w!("Local\\demonnapse")) };
    if mutex.is_err() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        info!("DemonNapse is already running, exiting");
        return;
    }

    info!("start: cleaning up Razer processes and services");
    synapse::kill_razer_apps();
    synapse::services(false);

    let mut meter = PeakMeter::new();
    let wall = UdpSocket::bind("127.0.0.1:0").ok();
    let clock = Instant::now();
    let mut last_error = String::new();
    loop {
        match session(dir, clock, &mut meter, wall.as_ref(), &mut last_error) {
            Ok(()) => wait_for_synapse_to_close(dir),
            Err(e) => {
                // mouse scollegato, file mancanti...: riprovo finche' non torna, senza riempire il log
                if e != last_error {
                    info!("error: {e}; retrying every 5 seconds");
                    last_error = e;
                }
                sleep(Duration::from_secs(5));
                if synapse::running() {
                    wait_for_synapse_to_close(dir);
                }
            }
        }
    }
}
