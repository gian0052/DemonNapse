//! Minimal Razer HID protocol (report format from the OpenRazer wiki).
//!
//! On-board profiles (class 0x05), verified on the Basilisk V3 35K:
//!   get 0x80 = number of profiles, 0x81 = [capacity, slots...], 0x84 = active slot
//!   set 0x04 = switches the active slot (the mouse emits the same events as the button)

use crate::layers::Rgb;
use hidapi::{HidApi, HidDevice};
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread::sleep;
use std::time::Duration;

pub const VID: u16 = 0x1532;
/// Transaction id of the reports: 0x1F on recent devices (35K included), 0x3F or 0xFF on others
/// (OpenRazer's device table). `open_control` tries them and keeps the one the mouse answers to.
const TRANSACTION_IDS: [u8; 3] = [0x1F, 0x3F, 0xFF];
static TRANSACTION_ID: AtomicU8 = AtomicU8::new(0x1F);

pub fn transaction_id() -> u8 {
    TRANSACTION_ID.load(Ordering::Relaxed)
}

pub fn build(cmd_class: u8, cmd_id: u8, data_size: u8, args: &[u8]) -> [u8; 90] {
    let mut r = [0u8; 90];
    r[1] = transaction_id();
    r[5] = data_size;
    r[6] = cmd_class;
    r[7] = cmd_id;
    r[8..8 + args.len()].copy_from_slice(args);
    r[88] = r[2..88].iter().fold(0, |crc, b| crc ^ b);
    r
}

fn send(h: &HidDevice, report: &[u8; 90]) -> Result<(), String> {
    let mut buf = [0u8; 91]; // byte 0 = report id
    buf[1..].copy_from_slice(report);
    h.send_feature_report(&buf).map_err(|e| e.to_string())
}

pub fn transact(h: &HidDevice, report: &[u8; 90]) -> Result<[u8; 90], String> {
    send(h, report)?;
    sleep(Duration::from_millis(50));
    let mut buf = [0u8; 91];
    h.get_feature_report(&mut buf).map_err(|e| e.to_string())?;
    let mut resp = [0u8; 90];
    resp.copy_from_slice(&buf[1..]);
    Ok(resp)
}

/// Finds the collection that accepts the 90-byte feature reports and the mouse's transaction id.
/// Also returns the response to the probe (firmware version, read only).
pub fn open_control(api: &mut HidApi, pid: u16, retries: u32) -> Result<(HidDevice, [u8; 90]), String> {
    for attempt in 0..retries {
        let _ = api.refresh_devices();
        for info in api.device_list().filter(|d| d.vendor_id() == VID && d.product_id() == pid) {
            let Ok(h) = info.open_device(api) else { continue };
            // always in the same order: a mouse that accepts any id (the 35K) stays on 0x1F
            for tid in TRANSACTION_IDS {
                TRANSACTION_ID.store(tid, Ordering::Relaxed);
                if let Ok(resp) = transact(&h, &build(0x00, 0x81, 0x02, &[])) {
                    if resp[0] == 0x02 && resp[6] == 0x00 && resp[7] == 0x81 {
                        return Ok((h, resp));
                    }
                }
            }
        }
        if attempt + 1 < retries {
            sleep(Duration::from_secs(1)); // another instance is dying or the mouse is busy
        }
    }
    Err("no control interface found".into())
}

/// The connected Razer devices, one per PID, with the name they give to Windows.
pub fn connected(api: &HidApi) -> Vec<(u16, String)> {
    let mut seen: Vec<(u16, String)> = Vec::new();
    for d in api.device_list().filter(|d| d.vendor_id() == VID) {
        match seen.iter_mut().find(|(p, _)| *p == d.product_id()) {
            Some((_, name)) if name.is_empty() => *name = d.product_string().unwrap_or("").to_string(),
            Some(_) => {}
            None => seen.push((d.product_id(), d.product_string().unwrap_or("").to_string())),
        }
    }
    seen
}

/// The collections the "05 xx" events come from (DPI, scroll wheel, profile switch).
pub fn open_events(api: &HidApi, pid: u16) -> Vec<HidDevice> {
    api.device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == pid && d.usage_page() == 0x0001 && d.usage() == 0x0000)
        .filter_map(|d| d.open_device(api).ok())
        .filter(|h| h.set_blocking_mode(false).is_ok())
        .collect()
}

/// LEDs one 0x0F/0x03 report holds (80 bytes of arguments: 5 of header, 3 per LED). Every Razer mouse
/// fits (docs/razer-mice.md: 21 at most); the rest of a longer frame is dropped.
pub const MAX_FRAME_LEDS: usize = 25;

fn frame_report(colors: &[Rgb]) -> [u8; 90] {
    let colors = &colors[..colors.len().min(MAX_FRAME_LEDS)];
    let mut args = vec![0, 0, 0, 0, colors.len().saturating_sub(1) as u8];
    args.extend(colors.iter().flat_map(|c| [c.0, c.1, c.2]));
    build(0x0F, 0x03, args.len() as u8, &args)
}

/// Colour per single LED (effect 0x0F/0x03). Send only: the mouse does not answer every frame.
pub fn custom_frame(h: &HidDevice, colors: &[Rgb]) -> Result<(), String> {
    send(h, &frame_report(colors))
}

/// Like `custom_frame`, but reads the mouse's answer: its status byte (0x02 = done, 0x05 = not supported).
pub fn custom_frame_status(h: &HidDevice, colors: &[Rgb]) -> Result<u8, String> {
    transact(h, &frame_report(colors)).map(|r| r[0])
}

/// Mouse mode: 0 = normal (the buttons are handled by the mouse, on-board profiles included),
/// 3 = driver (it sends them to the software: it is the mode Synapse leaves it in if it is force-closed).
pub fn get_device_mode(h: &HidDevice) -> Option<u8> {
    let resp = transact(h, &build(0x00, 0x84, 0x02, &[])).ok()?;
    (resp[0] == 0x02).then_some(resp[8])
}

pub fn set_device_mode(h: &HidDevice, mode: u8) -> bool {
    transact(h, &build(0x00, 0x04, 0x02, &[mode, 0x00])).map(|r| r[0] == 0x02).unwrap_or(false)
}

pub fn get_active_profile(h: &HidDevice) -> Option<u8> {
    let resp = transact(h, &build(0x05, 0x84, 0x01, &[])).ok()?;
    (resp[0] == 0x02).then_some(resp[8])
}

pub fn set_active_profile(h: &HidDevice, slot: u8) -> bool {
    transact(h, &build(0x05, 0x04, 0x01, &[slot])).map(|r| r[0] == 0x02).unwrap_or(false)
}
