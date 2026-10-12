//! Razer mice by PID: LED count and zones of the ones with per-LED lighting, and the ones without it.
//! Facts from the device tables of OpenRGB (`Controllers/RazerController/RazerDevices.cpp`, commit
//! feab05b6) and OpenRazer (`daemon/openrazer_daemon/hardware/mouse.py`, commit 477f4d6d), the same
//! data as docs/razer-mice.md. `check` uses it to tell a new user what their mouse is before Synapse's
//! logs do; the daemon never reads it.

pub struct Mouse {
    pub pid: u16,
    pub name: &'static str,
    pub leds: usize,
    /// Zone and LED count in OpenRGB's order, which is not proven to be the LED index order.
    pub zones: &'static str,
    /// Colours set with the extended matrix command 0x0F/0x03, the one DemonNapse sends.
    pub extended: bool,
}

/// What the tables say about a PID.
pub enum Known {
    Leds(&'static Mouse),
    NoLeds(&'static str),
    Unknown,
}

pub fn find(pid: u16) -> Known {
    if let Some(m) = WITH_LEDS.iter().find(|m| m.pid == pid) {
        return Known::Leds(m);
    }
    WITHOUT_LEDS.iter().find(|(p, _)| *p == pid).map_or(Known::Unknown, |(_, n)| Known::NoLeds(n))
}

pub fn name(pid: u16) -> Option<&'static str> {
    match find(pid) {
        Known::Leds(m) => Some(m.name),
        Known::NoLeds(n) => Some(n),
        Known::Unknown => None,
    }
}

/// "wired", "wireless" or "Bluetooth" when the name says it.
pub fn connection(name: &str) -> Option<&'static str> {
    [("(Wired)", "wired"), ("(Wireless)", "wireless"), ("(Bluetooth)", "Bluetooth")].iter().find(|(tag, _)| name.ends_with(tag)).map(|(_, c)| *c)
}

/// The model without the connection, e.g. "Razer Naga Pro" for "Razer Naga Pro (Wireless)".
fn model(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name)
}

/// The other PIDs of the same model (wired, wireless, Bluetooth), which share the LED layout.
pub fn siblings(pid: u16) -> Vec<u16> {
    let Known::Leds(m) = find(pid) else { return Vec::new() };
    WITH_LEDS.iter().filter(|o| o.pid != pid && model(o.name) == model(m.name) && o.leds == m.leds).map(|o| o.pid).collect()
}

pub const WITH_LEDS: &[Mouse] = &[
    Mouse { pid: 0x0024, name: "Razer Mamba 2012 (Wired)", leds: 15, zones: "Scroll Wheel 1", extended: false },
    Mouse { pid: 0x0025, name: "Razer Mamba 2012 (Wireless)", leds: 15, zones: "Scroll Wheel 1", extended: false },
    Mouse { pid: 0x003E, name: "Razer Naga Epic Chroma", leds: 2, zones: "Scroll Wheel 1, Numpad 1", extended: false },
    Mouse { pid: 0x0043, name: "Razer DeathAdder Chroma", leds: 2, zones: "Logo 1, Scroll Wheel 1", extended: false },
    Mouse { pid: 0x0044, name: "Razer Mamba 2015 (Wired)", leds: 15, zones: "Chroma Zone 15", extended: false },
    Mouse { pid: 0x0045, name: "Razer Mamba (Wireless)", leds: 15, zones: "Chroma Zone 15", extended: false },
    Mouse { pid: 0x0046, name: "Razer Mamba Tournament Edition", leds: 16, zones: "Left LED Strip 7, Right LED Strip 7, Logo 1, Scroll Wheel 1", extended: false },
    Mouse { pid: 0x004C, name: "Razer Diamondback Chroma", leds: 21, zones: "LED Strip 19, Logo 1, Scroll Wheel 1", extended: false },
    Mouse { pid: 0x0050, name: "Razer Naga Hex V2", leds: 3, zones: "Logo 1, Scroll Wheel 1, Numpad 1", extended: false },
    Mouse { pid: 0x0053, name: "Razer Naga Chroma", leds: 3, zones: "Scroll Wheel 1, Logo 1, Numpad 1", extended: false },
    Mouse { pid: 0x0059, name: "Razer Lancehead 2017 (Wired)", leds: 16, zones: "Right LED Strip 7, Left LED Strip 7, Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x005A, name: "Razer Lancehead 2017 (Wireless)", leds: 16, zones: "Right LED Strip 7, Left LED Strip 7, Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x005C, name: "Razer DeathAdder Elite", leds: 2, zones: "Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x0060, name: "Razer Lancehead Tournament Edition", leds: 16, zones: "Scroll Wheel 1, Logo 1, Right LED Strip 7, Left LED Strip 7", extended: true },
    Mouse { pid: 0x0064, name: "Razer Basilisk", leds: 2, zones: "Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x0065, name: "Razer Basilisk Essential", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x0067, name: "Razer Naga Trinity", leds: 3, zones: "Scroll Wheel 1, Logo 1, Numpad 1", extended: true },
    Mouse { pid: 0x0069, name: "Razer Mamba Hyperflux (Wired)", leds: 2, zones: "Scroll Wheel 1, Logo 1", extended: true },
    Mouse { pid: 0x006A, name: "Razer Abyssus Elite DVa Edition", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x006B, name: "Razer Abyssus Essential", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x006C, name: "Razer Mamba Elite", leds: 20, zones: "Scroll Wheel 1, Logo 1, Left LED Strip 9, Right LED Strip 9", extended: true },
    Mouse { pid: 0x006E, name: "Razer DeathAdder Essential", leds: 2, zones: "Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x006F, name: "Razer Lancehead 2019 (Wireless)", leds: 16, zones: "Right LED Strip 7, Left LED Strip 7, Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x0070, name: "Razer Lancehead 2019 (Wired)", leds: 16, zones: "Right LED Strip 7, Left LED Strip 7, Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x0071, name: "Razer DeathAdder Essential (White Edition)", leds: 2, zones: "Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x0072, name: "Razer Mamba 2018 (Wireless)", leds: 2, zones: "Scroll Wheel Zone 1, Logo Zone 1", extended: true },
    Mouse { pid: 0x0073, name: "Razer Mamba 2018 (Wired)", leds: 2, zones: "Scroll Wheel Zone 1, Logo Zone 1", extended: true },
    Mouse { pid: 0x0078, name: "Razer Viper", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x007A, name: "Razer Viper Ultimate (Wired)", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x007B, name: "Razer Viper Ultimate (Wireless)", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x007C, name: "Razer DeathAdder V2 (Wired)", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x007D, name: "Razer DeathAdder V2 (Wireless)", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x0084, name: "Razer DeathAdder V2", leds: 2, zones: "Scroll Wheel 1, Logo 1", extended: true },
    Mouse { pid: 0x0085, name: "Razer Basilisk V2", leds: 2, zones: "Scroll Wheel 1, Logo 1", extended: true },
    Mouse { pid: 0x0086, name: "Razer Basilisk Ultimate", leds: 14, zones: "Scroll Wheel 1, Logo 1, Left LED Strip 8, Right LED Strip 4", extended: true },
    Mouse { pid: 0x0088, name: "Razer Basilisk Ultimate (Wireless)", leds: 14, zones: "Scroll Wheel 1, Logo 1, Left LED Strip 8, Right LED Strip 4", extended: true },
    Mouse { pid: 0x008A, name: "Razer Viper Mini", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x008C, name: "Razer DeathAdder V2 Mini", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x008D, name: "Razer Naga Left Handed", leds: 3, zones: "Logo 1, Scroll Wheel 1, Numpad 1", extended: true },
    Mouse { pid: 0x008F, name: "Razer Naga Pro (Wired)", leds: 3, zones: "Scroll Wheel 1, Logo 1, Numpad 1", extended: true },
    Mouse { pid: 0x0090, name: "Razer Naga Pro (Wireless)", leds: 3, zones: "Scroll Wheel 1, Logo 1, Numpad 1", extended: true },
    Mouse { pid: 0x0091, name: "Razer Viper 8kHz", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x0093, name: "Razer Naga Classic", leds: 3, zones: "Logo 1, Scroll Wheel 1, Numpad 1", extended: true },
    Mouse { pid: 0x0098, name: "Razer DeathAdder Essential V2", leds: 2, zones: "Logo 1, Scroll Wheel 1", extended: true },
    Mouse { pid: 0x0099, name: "Razer Basilisk V3", leds: 11, zones: "Logo 1, Scroll Wheel 1, LED Strip 9", extended: true },
    Mouse { pid: 0x00A3, name: "Razer Cobra", leds: 1, zones: "Logo 1", extended: true },
    Mouse { pid: 0x00A7, name: "Razer Naga Pro V2 (Wired)", leds: 2, zones: "Logo 1, Numpad 1", extended: true },
    Mouse { pid: 0x00A8, name: "Razer Naga Pro V2 (Wireless)", leds: 2, zones: "Logo 1, Numpad 1", extended: true },
    Mouse { pid: 0x00AA, name: "Razer Basilisk V3 Pro (Wired)", leds: 13, zones: "Scroll Wheel 1, Logo 1, LED Strip 11", extended: true },
    Mouse { pid: 0x00AB, name: "Razer Basilisk V3 Pro (Wireless)", leds: 13, zones: "Scroll Wheel 1, Logo 1, LED Strip 11", extended: true },
    Mouse { pid: 0x00AC, name: "Razer Basilisk V3 Pro (Bluetooth)", leds: 13, zones: "Scroll Wheel 1, Logo 1, LED Strip 11", extended: true },
    Mouse { pid: 0x00AF, name: "Razer Cobra Pro (Wired)", leds: 11, zones: "Logo 1, Scroll Wheel 1, Underglow 9", extended: true },
    Mouse { pid: 0x00B0, name: "Razer Cobra Pro (Wireless)", leds: 11, zones: "Logo 1, Scroll Wheel 1, Underglow 9", extended: true },
    Mouse { pid: 0x00B9, name: "Razer Basilisk V3 X HyperSpeed", leds: 1, zones: "Scroll Wheel 1", extended: true },
    Mouse { pid: 0x00CB, name: "Razer Basilisk V3 35K", leds: 11, zones: "Logo 1, Scroll Wheel 1, LED Strip 9", extended: true },
    Mouse { pid: 0x00CC, name: "Razer Basilisk V3 Pro 35K (Wired)", leds: 13, zones: "Logo 1, Scroll Wheel 1, LED Strip 11", extended: true },
    Mouse { pid: 0x00CD, name: "Razer Basilisk V3 Pro 35K (Wireless)", leds: 13, zones: "Logo 1, Scroll Wheel 1, LED Strip 11", extended: true },
    Mouse { pid: 0x00D6, name: "Razer Basilisk V3 Pro 35K Phantom Green Edition (Wired)", leds: 13, zones: "Scroll Wheel 1, LED Strip 11", extended: true },
    Mouse { pid: 0x00D7, name: "Razer Basilisk V3 Pro 35K Phantom Green Edition (Wireless)", leds: 13, zones: "Scroll Wheel 1, LED Strip 11", extended: true },
    Mouse { pid: 0x00D8, name: "Razer Basilisk V3 Pro 35K Phantom Green Edition (Bluetooth)", leds: 13, zones: "Scroll Wheel 1, LED Strip 11", extended: true },
    Mouse { pid: 0x00E7, name: "Razer Naga Pro V3 (Wired)", leds: 3, zones: "Logo 1, Scroll Wheel 1, Numpad 1", extended: true },
    Mouse { pid: 0x00E8, name: "Razer Naga Pro V3 (Wireless)", leds: 3, zones: "Logo 1, Scroll Wheel 1, Numpad 1", extended: true },
    Mouse { pid: 0x0F1D, name: "Razer Mouse Bungee V3 Chroma", leds: 8, zones: "LED Strip 8", extended: true },
    Mouse { pid: 0x0096, name: "Razer Naga X", leds: 2, zones: "", extended: true },
    Mouse { pid: 0x00A1, name: "Razer DeathAdder V2 Lite", leds: 1, zones: "", extended: true },
];

pub const WITHOUT_LEDS: &[(u16, &str)] = &[
    (0x0013, "Razer Orochi 2011"),
    (0x0015, "Razer Naga"),
    (0x0016, "Razer DeathAdder 3.5G"),
    (0x001F, "Razer Naga Epic"),
    (0x0020, "Razer Abyssus 1800"),
    (0x0029, "Razer DeathAdder 3.5G Black"),
    (0x002E, "Razer Naga 2012"),
    (0x002F, "Razer Imperator"),
    (0x0032, "Razer Ouroboros"),
    (0x0034, "Razer Taipan"),
    (0x0036, "Razer Naga Hex Red"),
    (0x0037, "Razer DeathAdder 2013"),
    (0x0038, "Razer DeathAdder 1800"),
    (0x0039, "Razer Orochi 2013"),
    (0x003F, "Razer Naga Epic Chroma (Wireless)"),
    (0x0040, "Razer Naga 2014"),
    (0x0041, "Razer Naga Hex"),
    (0x0042, "Razer Abyssus"),
    (0x0048, "Razer Orochi (Wired)"),
    (0x004F, "Razer DeathAdder 2000"),
    (0x0054, "Razer DeathAdder 3500"),
    (0x005B, "Razer Abyssus V2"),
    (0x005E, "Razer Abyssus 2000"),
    (0x0062, "Razer Atheris (Wireless)"),
    (0x0077, "Razer Pro Click (Wireless)"),
    (0x0080, "Razer Pro Click (Wired)"),
    (0x0083, "Razer Basilisk X HyperSpeed"),
    (0x0094, "Razer Orochi V2 (Wireless)"),
    (0x0095, "Razer Orochi V2 (Bluetooth)"),
    (0x009A, "Razer Pro Click Mini (Wireless)"),
    (0x009C, "Razer DeathAdder V2 X HyperSpeed"),
    (0x009E, "Razer Viper Mini SE (Wired)"),
    (0x009F, "Razer Viper Mini SE (Wireless)"),
    (0x00A5, "Razer Viper V2 Pro (Wired)"),
    (0x00A6, "Razer Viper V2 Pro (Wireless)"),
    (0x00B2, "Razer DeathAdder V3"),
    (0x00B3, "Razer Hyper Polling Wireless Dongle"),
    (0x00B4, "Razer Naga V2 HyperSpeed (Wireless)"),
    (0x00B6, "Razer DeathAdder V3 Pro (Wired)"),
    (0x00B7, "Razer DeathAdder V3 Pro (Wireless)"),
    (0x00B8, "Razer Viper V3 HyperSpeed"),
    (0x00BE, "Razer DeathAdder V4 Pro (Wired)"),
    (0x00BF, "Razer DeathAdder V4 Pro (Wireless)"),
    (0x00C0, "Razer Viper V3 Pro (Wired)"),
    (0x00C1, "Razer Viper V3 Pro (Wireless)"),
    (0x00C2, "Razer DeathAdder V3 Pro (Wired)"),
    (0x00C3, "Razer DeathAdder V3 Pro (Wireless)"),
    (0x00C4, "Razer DeathAdder V3 HyperSpeed (Wired)"),
    (0x00C5, "Razer DeathAdder V3 HyperSpeed (Wireless)"),
    (0x00C7, "Razer Pro Click V2 Vertical Edition (Wired)"),
    (0x00C8, "Razer Pro Click V2 Vertical Edition (Wireless)"),
    (0x00D0, "Razer Pro Click V2 (Wired)"),
    (0x00D1, "Razer Pro Click V2 (Wireless)"),
    (0x00D3, "Razer Basilisk Mobile (Wired)"),
    (0x00D4, "Razer Basilisk Mobile (Wireless)"),
    (0x00DA, "Razer Cobra HyperSpeed"),
    (0x00DB, "Razer Cobra HyperSpeed (Wireless)"),
    (0x00DE, "Razer Viper V3 Pro SE (Wired)"),
    (0x00DF, "Razer Viper V3 Pro SE (Wireless)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_mice() {
        let Known::Leds(m) = find(0x00CB) else { panic!("the 35K is in the table") };
        assert_eq!((m.name, m.leds, m.extended), ("Razer Basilisk V3 35K", 11, true));
        assert!(matches!(find(0x00B4), Known::NoLeds("Razer Naga V2 HyperSpeed (Wireless)")));
        assert!(matches!(find(0xFFFF), Known::Unknown));
        assert_eq!(connection("Razer Naga Pro (Wireless)"), Some("wireless"));
        assert_eq!(connection("Razer Basilisk V3 35K"), None);
    }

    #[test]
    fn wired_and_wireless_are_siblings() {
        assert_eq!(siblings(0x00AB), vec![0x00AA, 0x00AC], "Basilisk V3 Pro wireless: wired and Bluetooth");
        assert_eq!(siblings(0x0090), vec![0x008F], "Naga Pro wireless: wired");
        assert!(siblings(0x00CB).is_empty(), "the 35K has one PID");
    }

    #[test]
    fn every_mouse_fits_one_frame() {
        // one 0x0F/0x03 report holds 25 LEDs: no mouse needs more than one
        assert!(WITH_LEDS.iter().all(|m| m.leds >= 1 && m.leds <= crate::razerhid::MAX_FRAME_LEDS));
        let mut pids: Vec<u16> = WITH_LEDS.iter().map(|m| m.pid).chain(WITHOUT_LEDS.iter().map(|(p, _)| *p)).collect();
        let n = pids.len();
        pids.sort();
        pids.dedup();
        assert_eq!(pids.len(), n, "a PID is listed twice");
    }
}
