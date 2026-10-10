//! The daemon: works out the active on-board profile from the mouse's HID events and draws the
//! matching Chroma effect. When Synapse opens it leaves the mouse to it; when Synapse closes it
//! re-reads the effects from Synapse's logs and resumes.

use crate::audio::PeakMeter;
use crate::config::Config;
use crate::layers::{Canvas, Rgb, BLACK};
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

const VK_BUTTONS: [i32; 5] = [0x01, 0x02, 0x04, 0x05, 0x06]; // left, right, middle, side buttons
const WALL_ADDR: &str = "127.0.0.1:47311"; // the wallpaper (wallpaper.py) listens here
const CONTROL_RETRIES: u32 = 5;

/// The "lights on/off" gesture: `clicks` slot changes within `window` seconds. The profile button
/// is handled by the firmware and the PC only gets the slot change, so those are what is counted.
struct Burst {
    clicks: usize,
    window: f64,
    times: Vec<f64>,
    home: u8, // the slot the burst started from, to be put back
}

impl Burst {
    fn new(clicks: usize, window: f64) -> Burst {
        Burst { clicks, window, times: Vec::new(), home: 0 }
    }

    /// A slot change that started from `from`: true if it completes the gesture.
    fn change(&mut self, from: u8, now: f64) -> bool {
        if self.clicks == 0 {
            return false;
        }
        self.times.retain(|&t| now - t <= self.window);
        if self.times.is_empty() {
            self.home = from;
        }
        self.times.push(now);
        if self.times.len() < self.clicks {
            return false;
        }
        self.times.clear();
        true
    }
}

/// Reads the active slot from the mouse; slots that are not configured (e.g. Hybrid Storage) are
/// skipped by moving on to the next configured one.
struct ProfileTracker {
    slots: Vec<u8>,
    idx: usize,
    last_poll: f64,
    raw: u8, // last slot read from or set on the mouse
    burst: Burst,
    hold_until: f64, // after the gesture the burst's extra clicks are undone
    toggled: bool,   // the gesture was made: the main loop picks it up
}

impl ProfileTracker {
    fn new(ctl: &HidDevice, slots: Vec<u8>, burst: Burst, now: f64) -> ProfileTracker {
        let mut t = ProfileTracker { slots, idx: 0, last_poll: 0.0, raw: 0, burst, hold_until: 0.0, toggled: false };
        t.refresh(ctl, now);
        t
    }

    /// Returns true if the active profile changed.
    fn refresh(&mut self, ctl: &HidDevice, now: f64) -> bool {
        self.last_poll = now;
        let Some(mut slot) = razerhid::get_active_profile(ctl) else { return false };
        if self.raw != 0 && slot != self.raw {
            if now < self.hold_until {
                razerhid::set_active_profile(ctl, self.burst.home);
                slot = self.burst.home;
            } else if self.burst.change(self.raw, now) {
                razerhid::set_active_profile(ctl, self.burst.home);
                slot = self.burst.home;
                self.hold_until = now + self.burst.window;
                self.toggled = true;
            }
        }
        if !self.slots.contains(&slot) {
            let next = self.slots.iter().copied().find(|&s| s > slot).unwrap_or(self.slots[0]);
            info!("slot {slot} not configured -> jumping to slot {next}");
            razerhid::set_active_profile(ctl, next);
            slot = next;
        }
        self.raw = slot;
        let new = self.slots.iter().position(|&s| s == slot).unwrap_or(0);
        let changed = new != self.idx;
        self.idx = new;
        changed
    }

    /// Any "05 xx" event (DPI, scroll wheel, profile switch) -> re-reads the slot.
    fn feed(&mut self, ctl: &HidDevice, report: &[u8], now: f64) -> bool {
        report.len() >= 2 && report[0] == 0x05 && self.refresh(ctl, now)
    }
}

/// Effects, profiles and LED map from Synapse's logs, written to `dir`. The map does not block the rest:
/// it can also be written by hand in config.json.
fn extract_all(dir: &Path) -> Result<(), String> {
    let pid = Config::load(dir)?.mouse()?;
    extract::effects(dir)?;
    extract::profiles(dir, pid)?;
    if let Err(e) = extract::device(dir, pid) {
        info!("{e}");
    }
    Ok(())
}

/// Synapse is open: I leave it the mouse and the services, and wait.
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
    sleep(Duration::from_secs(2)); // I let Synapse's child processes finish
    synapse::kill_razer_apps();
    synapse::services(false);
}

/// A "session" between one opening of Synapse and the next. Returns Ok when Synapse appears.
/// `dark`: lights turned off with the profile button gesture; it stays so even after a pause for Synapse.
fn session(dir: &Path, clock: Instant, meter: &mut PeakMeter, wall: Option<&UdpSocket>, last_error: &mut String, dark: &mut bool) -> Result<(), String> {
    // first install: the extracted files are not there yet, I read them from Synapse's logs right away
    if ["chroma_effects_dump.json", "synapse_profiles.json", "synapse_device.json"].iter().any(|f| !dir.join(f).exists()) {
        extract_all(dir)?;
        info!("effects, profiles and LED map read from Synapse's logs");
    }
    let cfg = Config::load(dir)?;
    cfg.check_device()?;
    let grid = cfg.grid();
    let canvas = Canvas::new(&grid, cfg.device.leds);
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
    let mut active: Vec<Active> = profiles::load(dir, &cfg, &grid, &canvas, seed)?;

    let mut api = HidApi::new().map_err(|e| e.to_string())?;
    let pid = cfg.mouse()?;
    if let Some(Ok((_, how))) = &cfg.detected {
        info!("mouse: {how}");
    }
    let (ctl, _) = razerhid::open_control(&mut api, pid, CONTROL_RETRIES)?;
    // a force-closed Synapse leaves the mouse in driver mode: the profile button no longer switches slot
    if let Some(mode @ 1..) = razerhid::get_device_mode(&ctl) {
        let ok = razerhid::set_device_mode(&ctl, 0);
        info!("mouse in mode {mode} (driver): set back to normal mode{}", if ok { "" } else { " - FAILED" });
    }
    let events = razerhid::open_events(&api, pid);
    let now = || clock.elapsed().as_secs_f64();
    let burst = Burst::new(cfg.lights_toggle_clicks, cfg.lights_toggle_seconds);
    let mut tracker = ProfileTracker::new(&ctl, active.iter().map(|a| a.slot).collect(), burst, now());
    info!("active: slot {} -> profile '{}'{}", active[tracker.idx].slot, active[tracker.idx].name, if *dark { ", lights off" } else { "" });
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
                return Ok(()); // Synapse has appeared: I let go of the mouse
            }
        }

        let mut changed = false;
        for h in &events {
            // a 1 ms timeout and not a non-blocking read: with timeout 0 hidapi's Windows backend
            // takes ERROR_IO_INCOMPLETE for an error, abandons the read in progress and loses the events
            while let Ok(n @ 1..) = h.read_timeout(&mut buf, 1) {
                debug!("event {}", buf[..n.min(6)].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "));
                changed |= tracker.feed(&ctl, &buf[..n], t);
            }
        }
        if t - tracker.last_poll > 30.0 {
            changed |= tracker.refresh(&ctl, t); // safety net in case an event is lost
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
        if std::mem::take(&mut tracker.toggled) {
            *dark = !*dark;
            current.effect.clear_flashes();
            last_frame = None;
            info!("lights {} (profile button pressed {} times)", if *dark { "off" } else { "on" }, cfg.lights_toggle_clicks);
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

        let frame = if *dark {
            vec![BLACK; cfg.device.leds]
        } else {
            let peak = if fx.has_audio() { meter.peak() } else { 0.0 };
            fx.render(t, dt, peak)
        };
        // resend only if it changes; keepalive every second (the firmware may redraw its own effect)
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
    // a single instance: two daemons on the same mouse get in each other's way
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
    let mut dark = false;
    loop {
        match session(dir, clock, &mut meter, wall.as_ref(), &mut last_error, &mut dark) {
            Ok(()) => wait_for_synapse_to_close(dir),
            Err(e) => {
                // mouse unplugged, missing files...: I retry until it comes back, without filling the log
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_quick_clicks_toggle_the_lights() {
        let mut b = Burst::new(5, 1.5);
        let from = [2, 3, 4, 2, 3];
        let got: Vec<bool> = from.iter().enumerate().map(|(i, &s)| b.change(s, 10.0 + i as f64 * 0.3)).collect();
        assert_eq!(got, [false, false, false, false, true]);
        assert_eq!(b.home, 2, "back to the slot the burst started from");

        let mut slow = Burst::new(5, 1.5);
        assert!(!(0..10).any(|i| slow.change(2, i as f64 * 0.5)), "one click every half second is not enough");

        let mut off = Burst::new(0, 1.5);
        assert!(!(0..10).any(|i| off.change(2, i as f64 * 0.01)), "0 = gesture disabled");
    }
}
