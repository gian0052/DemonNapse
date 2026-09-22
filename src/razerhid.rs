//! Protocollo HID Razer minimale (formato dei report dal wiki di OpenRazer).
//!
//! Profili on-board (classe 0x05), verificati sul Basilisk V3 35K:
//!   get 0x80 = numero profili, 0x81 = [capienza, slot...], 0x84 = slot attivo
//!   set 0x04 = cambia slot attivo (il mouse emette gli stessi eventi del pulsante)

use crate::layers::Rgb;
use hidapi::{HidApi, HidDevice};
use std::thread::sleep;
use std::time::Duration;

pub const VID: u16 = 0x1532;
const TRANSACTION_ID: u8 = 0x1F;

pub fn build(cmd_class: u8, cmd_id: u8, data_size: u8, args: &[u8]) -> [u8; 90] {
    let mut r = [0u8; 90];
    r[1] = TRANSACTION_ID;
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

/// Trova la collection che accetta i feature report da 90 byte. Ritorna anche la risposta
/// alla sonda (versione firmware, sola lettura).
pub fn open_control(api: &mut HidApi, pid: u16, retries: u32) -> Result<(HidDevice, [u8; 90]), String> {
    let probe = build(0x00, 0x81, 0x02, &[]);
    for attempt in 0..retries {
        let _ = api.refresh_devices();
        for info in api.device_list().filter(|d| d.vendor_id() == VID && d.product_id() == pid) {
            if let Ok(h) = info.open_device(api) {
                if let Ok(resp) = transact(&h, &probe) {
                    if resp[0] == 0x02 && resp[6] == 0x00 && resp[7] == 0x81 {
                        return Ok((h, resp));
                    }
                }
            }
        }
        if attempt + 1 < retries {
            sleep(Duration::from_secs(1)); // un'altra istanza sta morendo o il mouse e' occupato
        }
    }
    Err("no control interface found".into())
}

/// Le collection da cui arrivano gli eventi "05 xx" (DPI, rotella, cambio profilo).
pub fn open_events(api: &HidApi, pid: u16) -> Vec<HidDevice> {
    api.device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == pid && d.usage_page() == 0x0001 && d.usage() == 0x0000)
        .filter_map(|d| d.open_device(api).ok())
        .filter(|h| h.set_blocking_mode(false).is_ok())
        .collect()
}

/// Colore per singolo LED (effetto 0x0F/0x03). Solo invio: il mouse non risponde a ogni frame.
pub fn custom_frame(h: &HidDevice, colors: &[Rgb]) -> Result<(), String> {
    let mut args = vec![0, 0, 0, 0, colors.len().saturating_sub(1) as u8];
    args.extend(colors.iter().flat_map(|c| [c.0, c.1, c.2]));
    send(h, &build(0x0F, 0x03, args.len() as u8, &args))
}

/// Modo del mouse: 0 = normale (i pulsanti li gestisce il mouse, profili on-board compresi),
/// 3 = driver (li manda al software: e' il modo in cui lo lascia Synapse se viene chiuso di forza).
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
