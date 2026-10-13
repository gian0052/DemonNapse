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

/// The arguments of 0F/03: 0, zone (0 = the device's own), row, first column, last column, then the colours.
fn row_report(row: u8, colors: &[Rgb]) -> [u8; 90] {
    let colors = &colors[..colors.len().min(MAX_FRAME_LEDS)];
    let mut args = vec![0, 0, row, 0, colors.len().saturating_sub(1) as u8];
    args.extend(colors.iter().flat_map(|c| [c.0, c.1, c.2]));
    build(0x0F, 0x03, args.len() as u8, &args)
}

fn frame_report(colors: &[Rgb]) -> [u8; 90] {
    row_report(0, colors) // a mouse is one row
}

/// One row of a grid (a keyboard). Send only, like `custom_frame`.
pub fn custom_row(h: &HidDevice, row: u8, colors: &[Rgb]) -> Result<(), String> {
    send(h, &row_report(row, colors))
}

/// Like `custom_row`, but reads the device's answer (its status byte).
pub fn custom_row_status(h: &HidDevice, row: u8, colors: &[Rgb]) -> Result<u8, String> {
    transact(h, &row_report(row, colors)).map(|r| r[0])
}

/// Asks the device to show the rows it was sent: effect 8 (custom) with profile 0, which in the firmware
/// read so far means "show it now, save nothing". OpenRazer sends the same after the rows of a keyboard;
/// a mouse shows its one row without it.
fn show_custom_report() -> [u8; 90] {
    build(0x0F, 0x02, 0x0C, &[0x00, 0x00, 0x08])
}

pub fn show_custom(h: &HidDevice) -> Result<(), String> {
    send(h, &show_custom_report())
}

pub fn show_custom_status(h: &HidDevice) -> Result<u8, String> {
    transact(h, &show_custom_report()).map(|r| r[0])
}

/// Colour per single LED (effect 0x0F/0x03). Send only: the mouse does not answer every frame.
pub fn custom_frame(h: &HidDevice, colors: &[Rgb]) -> Result<(), String> {
    send(h, &frame_report(colors))
}

/// Like `custom_frame`, but reads the mouse's answer: its status byte (0x02 = done, 0x05 = not supported).
pub fn custom_frame_status(h: &HidDevice, colors: &[Rgb]) -> Result<u8, String> {
    transact(h, &frame_report(colors)).map(|r| r[0])
}

/// Read commands with a known format (class, id, data size, arguments, what they read): `check` sends
/// them and reports which ones the mouse answers, to tell which firmware family a new model belongs
/// to. None changes anything on the mouse. Formats from OpenRazer (00/85, 04/85, 07/80, 0F/84), from
/// the firmware of the Basilisk V3, V3 Pro, V3 Pro 35K and Cobra Pro (05/88, 05/8A) and from the 35K itself (0F/82).
/// In class 0x0F the first argument is the on-board profile (0 = what is shown now), the second the
/// zone (0x01 scroll wheel, 0x04 logo, 0x0A strip).
pub const READS: &[(u8, u8, u8, &[u8], &str)] = &[
    (0x00, 0x85, 0x01, &[], "polling rate"),
    (0x04, 0x85, 0x07, &[0x00], "DPI"),
    (0x07, 0x80, 0x02, &[], "battery, wireless mice only"),
    (0x05, 0x8A, 0x01, &[], "largest number of profiles"),
    // data size = 5 + bytes wanted (16 here), arguments = profile, offset high, offset low: the firmware
    // subtracts 5 from the size to get the length, a smaller size would ask for a huge one
    (0x05, 0x88, 0x15, &[0x01, 0x00, 0x00], "first 16 bytes of the data block of profile 1"),
    (0x05, 0x88, 0x15, &[0x02, 0x00, 0x00], "first 16 bytes of the data block of profile 2"),
    // five bytes per zone, the first is the zone id; on the Basilisk V3 Pro, V3 Pro 35K and Cobra Pro the
    // firmware answers 01 19 03 01 01, 04 19 03 01 01, 0A 19 03 01 0B (the last two bytes are rows and columns)
    (0x0F, 0x80, 0x0F, &[], "lighting zones of the mouse"),
    (0x0F, 0x82, 0x0C, &[0x00, 0x01], "light shown now, scroll wheel"),
    (0x0F, 0x82, 0x0C, &[0x00, 0x04], "light shown now, logo"),
    (0x0F, 0x82, 0x0C, &[0x00, 0x0A], "light shown now, strip"),
    (0x0F, 0x82, 0x0C, &[0x01, 0x01], "light saved in profile 1, scroll wheel"),
    (0x0F, 0x82, 0x0C, &[0x01, 0x04], "light saved in profile 1, logo"),
    (0x0F, 0x82, 0x0C, &[0x01, 0x0A], "light saved in profile 1, strip"),
    (0x0F, 0x82, 0x0C, &[0x02, 0x04], "light saved in profile 2, logo"),
    (0x0F, 0x84, 0x03, &[0x00, 0x00], "brightness shown now"),
    (0x0F, 0x84, 0x03, &[0x00, 0x04], "brightness shown now, logo"),
    (0x0F, 0x84, 0x03, &[0x01, 0x00], "brightness saved in profile 1"),
];

/// The lighting zones the device itself declares (0F/80): zone id, rows and columns, in the order of the
/// LED indices of the frame. Five bytes per zone, `id 19 03 rows columns`. Read in the firmware of the
/// Basilisk V3 Pro, V3 Pro 35K and Cobra Pro (three zones of one row) and of two keyboards (one zone,
/// id 05: 6 x 24 on the BlackWidow V4, 6 x 22 on the Huntsman V3 Pro); on the Basilisk V3 35K it
/// answers logo 1, scroll wheel 1, strip 9, the layout verified by eye. `None` when it is not answered.
pub fn zones(h: &HidDevice) -> Option<Vec<(u8, usize, usize)>> {
    let resp = transact(h, &build(0x0F, 0x80, 0x0F, &[])).ok().filter(|r| r[0] == 0x02)?;
    parse_zones(&resp[8..8 + (resp[5] as usize).min(80)])
}

fn parse_zones(data: &[u8]) -> Option<Vec<(u8, usize, usize)>> {
    let zones: Vec<(u8, usize, usize)> = data.chunks_exact(5).map(|z| (z[0], z[3] as usize, z[4] as usize)).collect();
    (!zones.is_empty() && data.len() % 5 == 0 && zones.iter().all(|z| z.1 >= 1 && z.2 >= 1)).then_some(zones)
}

/// The LEDs of zones that are all one row, the only layout one frame report can hold: a mouse.
/// `None` for a grid (a keyboard), which needs one report per row.
pub fn single_row(zones: &[(u8, usize, usize)]) -> Option<usize> {
    let total: usize = zones.iter().map(|z| z.2).sum();
    (zones.iter().all(|z| z.1 == 1) && total <= MAX_FRAME_LEDS).then_some(total)
}

/// Rows and columns of the first zone that is a grid of several rows: a keyboard.
pub fn grid(zones: &[(u8, usize, usize)]) -> Option<(usize, usize)> {
    zones.iter().find(|z| z.1 > 1).map(|z| (z.1, z.2))
}

/// The name of a lighting zone id of class 0x0F.
pub fn zone_name(id: u8) -> String {
    match id {
        0x01 => "scroll wheel".into(),
        0x04 => "logo".into(),
        0x05 => "keys".into(),
        0x0A => "strip".into(),
        id => format!("zone 0x{id:02X}"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_of_the_35k() {
        // the answer of the Basilisk V3 35K on 2026-10-12
        let data = [0x04, 0x19, 0x03, 0x01, 0x01, 0x01, 0x19, 0x03, 0x01, 0x01, 0x0A, 0x19, 0x03, 0x01, 0x09];
        let zones = parse_zones(&data).unwrap();
        assert_eq!(zones, vec![(0x04, 1, 1), (0x01, 1, 1), (0x0A, 1, 9)]);
        assert_eq!(single_row(&zones), Some(11));
        assert_eq!(parse_zones(&data[..12]), None, "a cut answer is not trusted");
        assert_eq!(parse_zones(&[0; 15]), None, "no LEDs at all");
        // what the firmware of the BlackWidow V4 answers: a grid, not one row
        let keys = parse_zones(&[0x05, 0x19, 0x03, 0x06, 0x18]).unwrap();
        assert_eq!((keys.clone(), single_row(&keys)), (vec![(0x05, 6, 24)], None));
        assert_eq!((grid(&keys), grid(&zones)), (Some((6, 24)), None));
    }

    #[test]
    fn rows_of_a_grid() {
        let r = row_report(3, &[(1, 2, 3); 24]);
        assert_eq!((r[5], r[6], r[7]), (5 + 72, 0x0F, 0x03));
        assert_eq!(r[8..16], [0, 0, 3, 0, 23, 1, 2, 3], "0, zone, row, first column, last column, colours");
        assert_eq!(frame_report(&[(9, 9, 9); 11])[8..13], [0, 0, 0, 0, 10], "a mouse is row 0");
        // profile 0: shown, not saved
        assert_eq!(show_custom_report()[8..11], [0x00, 0x00, 0x08]);
    }

    #[test]
    fn reads_only_read() {
        // an id below 0x80 sets something; in the firmware read so far every id from 0x80 up only reads.
        // Class 0xFE (factory) is never asked.
        assert!(READS.iter().all(|&(class, id, _, _, _)| id >= 0x80 && class != 0xFE));
        // 05/88 gets its length from the data size minus 5: it must stay inside the 80 bytes of arguments
        assert!(READS.iter().filter(|r| (r.0, r.1) == (0x05, 0x88)).all(|r| (6..=80).contains(&r.2)));
    }
}
