//! `demonnapse check [PID]`: tests step by step whether a Razer mouse works with DemonNapse and writes
//! a report to attach to an issue. It is for people with a model other than the Basilisk V3 35K, so it
//! works without config.json, says what the mouse is from the tables in `mice`, asks the user about
//! every LED and saves the report next to the executable.
//! Reads only, plus switching the active slot (like the button, then back as it was) and a few
//! colours on the LEDs: nothing is written to the mouse's memory.
//! A keyboard (a device that declares a grid of several rows) gets its own last step, all in the same
//! run: the keys in three colours, one row at a time, the first and last column, a bar crossing it.

use crate::config::{self, Config};
use crate::layers::Rgb;
use crate::mice::{self, Known};
use crate::{extract, razerhid, synapse};
use hidapi::{HidApi, HidDevice};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Console::{AllocConsole, FreeConsole};
use windows::Win32::System::Threading::CreateMutexW;

const BUTTON_SECONDS: u64 = 15;
const BLACK: Rgb = (0, 0, 0);

/// Everything printed, also kept as the report. Once the file has a name it is rewritten after every
/// line, so closing the window halfway (the X kills the program) loses nothing.
struct Report {
    text: String,
    path: Option<PathBuf>,
}

impl Report {
    fn line(&mut self, s: String) {
        println!("{s}");
        self.note(s);
    }

    /// A line of the report that is not printed again (the user typed it).
    fn note(&mut self, s: String) {
        self.text += &s;
        self.text.push('\n');
        if let Some(p) = &self.path {
            let _ = std::fs::write(p, &self.text);
        }
    }
}

macro_rules! say {
    ($r:expr, $($t:tt)*) => { $r.line(format!($($t)*)) };
}

/// An instruction for the person at the keyboard: marked so it stands out from the report lines.
fn tell(s: &str) {
    println!(">> {s}");
}

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

/// The status byte of a Razer response, in words.
fn status(s: u8) -> String {
    match s {
        0x02 => "OK, the mouse accepted it".into(),
        0x05 => "NO, the mouse answered \"not supported\" (0x05)".into(),
        0x03 => "NO, the mouse answered \"failure\" (0x03)".into(),
        0x01 => "busy (0x01), no clear answer".into(),
        0x04 => "timeout (0x04), no clear answer".into(),
        s => format!("unknown answer 0x{s:02X}"),
    }
}

/// One line per read command of `razerhid::READS`: the command, what it reads, the mouse's answer and
/// the first bytes of the data. Which ones answer tells the firmware family of a model never seen.
pub fn fingerprint(h: &HidDevice) -> Vec<String> {
    let line = |&(class, id, size, args, what): &(u8, u8, u8, &[u8], &str)| {
        let answer = match razerhid::transact(h, &razerhid::build(class, id, size, args)) {
            Ok(r) if r[0] == 0x02 => format!("OK  {}", hex(&r[8..8 + (r[5] as usize).clamp(1, 80)])), // r[5] = size of the answer
            Ok(r) if r[0] == 0x05 => "not supported".into(),
            Ok(r) if r[0] == 0x03 => "failure".into(),
            Ok(r) => format!("status 0x{:02X}", r[0]),
            Err(e) => format!("not sent ({e})"),
        };
        let sent = if args.is_empty() { String::new() } else { format!(" {}", hex(args)) };
        format!("{class:02X}/{id:02X}{sent} ({what}): {answer}")
    };
    razerhid::READS.iter().map(line).collect()
}

fn show(h: &HidDevice, frame: &[Rgb], seconds: f64) -> bool {
    let start = Instant::now();
    let mut ok = true;
    while start.elapsed().as_secs_f64() < seconds {
        ok &= razerhid::custom_frame(h, frame).is_ok();
        sleep(Duration::from_millis(100));
    }
    ok
}

/// Lines typed by the user, read on their own thread so the LEDs keep being refreshed meanwhile;
/// `None` once there is no keyboard (input redirected or closed).
fn keyboard() -> Receiver<Option<String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || loop {
        let mut line = String::new();
        let got = std::io::stdin().read_line(&mut line);
        let msg = match got {
            Ok(1..) => Some(line.trim().to_string()),
            _ => None,
        };
        let end = msg.is_none();
        if tx.send(msg).is_err() || end {
            break;
        }
    });
    rx
}

/// What the user said about one LED.
#[derive(Debug, PartialEq)]
enum Seen {
    Lit(String), // where, if they wrote it
    NotLit,
    Stop,
}

fn understand(answer: &str) -> Seen {
    match answer.to_lowercase().as_str() {
        "" | "y" | "yes" | "s" | "si" | "sì" => Seen::Lit(String::new()),
        "n" | "no" => Seen::NotLit,
        "q" | "quit" | "stop" => Seen::Stop,
        _ => Seen::Lit(answer.to_string()),
    }
}

/// Keeps `frame` on the mouse until the user answers; `None` when there is no keyboard to answer with.
fn hold(h: &HidDevice, frame: &[Rgb], keys: &Receiver<Option<String>>) -> Option<String> {
    loop {
        let _ = razerhid::custom_frame(h, frame);
        match keys.recv_timeout(Duration::from_millis(100)) {
            Ok(answer) => return answer,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

fn ask(prompt: &str) {
    print!("{prompt} ");
    let _ = std::io::stdout().flush();
}

type Grid = Vec<Vec<Rgb>>;

/// Sends a grid, one report per row, then asks the device to show it.
fn send_grid(h: &HidDevice, grid: &Grid) {
    for (row, colors) in grid.iter().enumerate() {
        let _ = razerhid::custom_row(h, row as u8, colors);
    }
    let _ = razerhid::show_custom(h);
}

fn show_grid(h: &HidDevice, grid: &Grid, seconds: f64) {
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        send_grid(h, grid);
        sleep(Duration::from_millis(100));
    }
}

/// Keeps `grid` on the keyboard until the user answers; `None` when there is nothing to answer with.
fn hold_grid(h: &HidDevice, grid: &Grid, keys: &Receiver<Option<String>>) -> Option<String> {
    loop {
        send_grid(h, grid);
        match keys.recv_timeout(Duration::from_millis(100)) {
            Ok(answer) => return answer,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// An answer as it goes in the report: Enter alone means yes.
fn answered(answer: Option<String>) -> String {
    match answer.as_deref() {
        None => "not asked (nothing to answer with)".into(),
        Some("") => "yes".into(),
        Some(a) => a.into(),
    }
}

/// A grid all black except the cells `lit` says.
fn grid_where(rows: usize, cols: usize, lit: impl Fn(usize, usize) -> bool) -> Grid {
    (0..rows).map(|row| (0..cols).map(|col| if lit(row, col) { (255, 255, 255) } else { BLACK }).collect()).collect()
}

/// The LED part of check on a keyboard, all in one run: the whole grid in three colours, then one row at
/// a time, the first and last column and a bar crossing it. With the answers, one report says how the
/// rows and columns the keyboard declares lie under its keys. Colours only, nothing is saved.
fn keyboard_steps(r: &mut Report, h: &HidDevice, keys: &Receiver<Option<String>>, rows: usize, cols: usize, device: &str) {
    say!(r, "\n[6] Profile button: skipped, this is a keyboard");
    say!(r, "\n[7] Keys: look at the keyboard");
    say!(r, "  grid told by the keyboard (0F/80): {rows} rows x {cols} columns");
    let cols = cols.min(razerhid::MAX_FRAME_LEDS);
    let full = |c: Rgb| vec![vec![c; cols]; rows];
    let said = |res: Result<u8, String>| res.map_or_else(|e| format!("NO, not sent ({e})"), status);
    let row_answer = said(razerhid::custom_row_status(h, 0, &vec![(255, 0, 0); cols]));
    let show_answer = said(razerhid::show_custom_status(h));
    say!(r, "  one row of colours (0F/03): {row_answer}");
    say!(r, "  show the colours (0F/02, effect 8, nothing saved): {show_answer}");

    tell("Look at the keyboard: all the keys turn red, then green, then blue.");
    for c in [(255, 0, 0), (0, 255, 0), (0, 0, 255)] {
        show_grid(h, &full(c), 1.0);
    }
    while keys.try_recv().is_ok() {} // an Enter typed during the colours does not count
    ask(">> Did every key go red, green and blue? Enter = yes, n = no, or write what you saw:");
    let all = answered(hold_grid(h, &full((0, 0, 255)), keys));
    r.note(format!("  all keys red, green, blue: {all}"));

    tell("Now one row of keys at a time lights up white. For each one type which row it is, counting from");
    tell("the top (1 = the row of Esc), and press Enter. n = nothing lit. If only some keys are lit, write which.");
    let mut seen = 0;
    for row in 0..rows {
        ask(&format!("  row {} of {rows}:", row + 1));
        let a = hold_grid(h, &grid_where(rows, cols, |y, _| y == row), keys);
        if a.is_none() {
            println!();
        }
        seen += usize::from(a.as_deref().is_some_and(|a| !matches!(a.to_lowercase().as_str(), "n" | "no")));
        r.note(format!("  row {} of {rows}: {}", row + 1, answered(a)));
    }
    tell("Now one column at a time. Write the keys that are lit, from the top.");
    ask("  first column:");
    let first = answered(hold_grid(h, &grid_where(rows, cols, |_, x| x == 0), keys));
    r.note(format!("  first column: {first}"));
    ask(&format!("  last column ({cols}):"));
    let last = answered(hold_grid(h, &grid_where(rows, cols, |_, x| x == cols - 1), keys));
    r.note(format!("  last column ({cols}): {last}"));
    tell("Last one: a white bar crosses the keyboard twice.");
    for _ in 0..2 {
        for col in 0..cols {
            show_grid(h, &grid_where(rows, cols, |_, x| x == col), 0.15);
        }
    }
    while keys.try_recv().is_ok() {}
    ask(">> Did the bar go from left to right without jumps? Enter = yes, n = no, or write what you saw:");
    let bar = answered(hold_grid(h, &full(BLACK), keys));
    r.note(format!("  bar from left to right: {bar}"));

    say!(r, "\n== Summary ==");
    say!(r, "device: {device}");
    say!(r, "a keyboard: {rows} rows x {cols} columns. DemonNapse does not play effects on keyboards yet: this report is what it needs to start");
    say!(r, "one row of colours: {row_answer}");
    say!(r, "show the colours: {show_answer}");
    say!(r, "all keys red, green, blue: {all}");
    say!(r, "rows seen lit: {seen} of {rows}");
    say!(r, "first column: {first}; last column: {last}; bar from left to right: {bar}");
    tell("The keys stay dark until Synapse is opened again.");
}

/// The LED count from the walk without a known count: the last LED seen lit, stopping after two dark ones.
fn count_from(answers: &[Seen]) -> usize {
    answers.iter().rposition(|s| matches!(s, Seen::Lit(_))).map_or(0, |i| i + 1)
}

/// The note about slot 1, which the daemon skips by default because it is Hybrid Storage on the 35K.
fn slot1_note(profiles: &[extract::Profile], skip: &[u8]) -> String {
    match (profiles.iter().find(|p| p.slot == 1), skip.contains(&1)) {
        (Some(p), true) => format!(
            "WARNING: slot 1 holds the profile '{}', but the daemon skips slot 1 (Hybrid Storage on the Basilisk V3 35K): set \"skip_slots\": [] in config.json",
            p.name
        ),
        (Some(p), false) => format!("slot 1 holds the profile '{}' and is not skipped", p.name),
        (None, _) => "no Synapse profile on slot 1 (on the Basilisk V3 35K it is Hybrid Storage, skipped)".into(),
    }
}

/// What the tables know about a PID, in one line.
fn describe(pid: u16) -> String {
    match mice::find(pid) {
        Known::Leds(m) if m.extended => format!("{}, {} LEDs ({})", m.name, m.leds, if m.zones.is_empty() { "zones unknown" } else { m.zones }),
        Known::Leds(m) => format!("{}, {} LEDs, older colour command that DemonNapse does not send yet", m.name, m.leds),
        Known::NoLeds(n) => format!("{n}, no per-LED lighting according to OpenRazer"),
        Known::Unknown => "not in the tables of OpenRGB and OpenRazer (docs/razer-mice.md)".into(),
    }
}

/// The device to check: the PID given, the one in config.json, the one Synapse calls a mouse, the only
/// connected device the tables know as a mouse, otherwise (no mouse) the only keyboard for Synapse.
fn pick(args: &[String], cfg: Option<&Config>, seen: &[(u16, String)], categories: &HashMap<u16, String>) -> Result<(u16, &'static str), String> {
    if let Some(a) = args.first() {
        let pid = u16::from_str_radix(a.trim_start_matches("0x").trim_start_matches("0X"), 16).map_err(|_| format!("'{a}' is not a PID like 0x00CB"))?;
        return Ok((pid, "given on the command line"));
    }
    if let Some(pid) = cfg.and_then(|c| c.mouse().ok()) {
        return Ok((pid, "from config.json or found by the daemon's rule"));
    }
    let synapse = config::choose_mouse(seen, categories);
    if let Ok((pid, _)) = synapse {
        return Ok((pid, "the one Synapse calls a mouse"));
    }
    let known: Vec<u16> = seen.iter().map(|(p, _)| *p).filter(|&p| !matches!(mice::find(p), Known::Unknown)).collect();
    let keyboards: Vec<u16> = seen.iter().map(|(p, _)| *p).filter(|p| categories.get(p).is_some_and(|c| c == "keyboard")).collect();
    match (&known[..], &keyboards[..]) {
        (&[pid], _) => Ok((pid, "the only connected Razer mouse in the tables")),
        (&[], &[pid]) => Ok((pid, "no mouse: the only Razer keyboard for Synapse")),
        _ => Err(format!("{} (run check 0x00XX with the PID of the mouse, from the list above)", synapse.unwrap_err())),
    }
}

/// Where the report goes: next to the executable, else next to config.json, else the temporary folder.
/// The first folder that accepts the file wins.
fn save(dir: &Path, pid: u16, text: &str) -> Result<PathBuf, String> {
    let name = format!("demonnapse-check-{pid:04X}.txt");
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let places = exe_dir.into_iter().chain([dir.to_path_buf(), std::env::temp_dir()]);
    let mut last = String::new();
    for place in places {
        let path = place.join(&name);
        match std::fs::write(&path, text) {
            Ok(()) => return Ok(path),
            Err(e) => last = format!("{}: {e}", path.display()),
        }
    }
    Err(last)
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
    // A terminal does not wait for a program without a window of its own: it would read the keyboard
    // together with check. Check asks questions, so it gets its own console window.
    println!("check opens in its own window: follow the lines marked >> there");
    unsafe {
        let _ = FreeConsole();
        let _ = AllocConsole();
    }
    let mut r = Report { text: String::new(), path: None };
    let keys = keyboard();
    let result = steps(dir, args, &mut r, &keys);
    if let Err(e) = &result {
        say!(r, "\nstopped: {e}");
    }
    println!();
    let saved = match r.path.clone() {
        Some(p) => Ok(p),
        None => save(dir, 0, &r.text), // stopped before the mouse was picked
    };
    match saved {
        Ok(path) => {
            tell(&format!("Done. Report saved as {}", path.display()));
            tell("Attach it to a device report: https://github.com/gian0052/DemonNapse/issues/new?template=device_report.yml");
        }
        Err(e) => tell(&format!("The report could not be saved ({e}): copy the text above.")),
    }
    // seen on the Basilisk V3 35K: after check the LEDs are off, the profile button brings back the
    // effect saved in the mouse, opening Synapse brings back the Chroma Studio effects
    tell("If the LEDs are off now: press the profile button and the mouse's own lighting comes back,");
    tell("or open Synapse again and your Chroma effects come back. Nothing saved was changed.");
    tell("If you use the daemon, start it again: schtasks /Run /TN DemonNapse");
    ask(">> Press Enter to close this window.");
    let _ = keys.recv();
    result
}

fn steps(dir: &Path, args: &[String], r: &mut Report, keys: &Receiver<Option<String>>) -> Result<(), String> {
    say!(r, "DemonNapse check {}", env!("CARGO_PKG_VERSION"));
    tell("About two minutes. Do what the lines marked >> say; the rest is the report.");
    say!(r, "\n[1] Razer devices connected");
    let mut api = HidApi::new().map_err(|e| e.to_string())?;
    let seen = razerhid::connected(&api);
    let categories = extract::categories();
    for (pid, name) in &seen {
        let synapse = categories.get(pid).map_or("type unknown to Synapse", String::as_str);
        say!(r, "  PID 0x{pid:04X}  {name}  ({synapse}; {})", mice::name(*pid).unwrap_or("not in the tables"));
    }
    let cfg = Config::load(dir).ok();
    say!(r, "  config.json: {}", if cfg.is_some() { "found" } else { "not found, not needed by check" });
    let (pid, how) = pick(args, cfg.as_ref(), &seen, &categories)?;
    r.path = save(dir, pid, &r.text).ok(); // from here on every line goes to the file at once
    let name = seen.iter().find(|(p, _)| *p == pid).map_or("", |(_, n)| n.as_str());
    say!(r, "  checking PID 0x{pid:04X} {name} ({how})");
    say!(r, "  model: {}", describe(pid));
    let connection = mice::name(pid).and_then(mice::connection);
    say!(r, "  connection: {}", connection.unwrap_or("not told by the PID (say it in the issue: cable, dongle or Bluetooth)"));
    if let Known::NoLeds(_) = mice::find(pid) {
        say!(r, "  this mouse has no LEDs DemonNapse can drive one by one: the effects cannot be played on it, the rest is checked anyway");
    }

    say!(r, "\n[2] Control interface");
    let (ctl, resp) = razerhid::open_control(&mut api, pid, 2).map_err(|e| format!("{e}: this device does not answer the Razer protocol the way DemonNapse speaks it"))?;
    say!(r, "  transaction id: 0x{:02X}", razerhid::transaction_id());
    say!(r, "  firmware: {}.{}", resp[8], resp[9]);
    say!(r, "  device mode: {:?} (0 = normal, 3 = driver)", razerhid::get_device_mode(&ctl));

    say!(r, "\n[3] Synapse's logs");
    let tmp = std::env::temp_dir().join("demonnapse-check");
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    // a wireless mouse may be logged under its wired PID: same model, same LEDs
    let mut map = extract::device(&tmp, pid).map(|m| (m, pid));
    if map.is_err() {
        if let Some(found) = mice::siblings(pid).into_iter().find_map(|o| extract::device(&tmp, o).ok().map(|m| (m, o))) {
            map = Ok(found);
        }
    }
    match &map {
        Ok((m, from)) => {
            let rows = m.chroma_grid.iter().map(|(cell, led)| format!("{cell}->{led}")).collect::<Vec<_>>().join(" ");
            let borrowed = if *from == pid { String::new() } else { format!(", logged under PID 0x{from:04X} (same model)") };
            say!(r, "  LED map: OK, {} LEDs, {} grid cells{borrowed}", m.leds, m.chroma_grid.len());
            say!(r, "  grid (Chroma Studio cell -> LED): {rows}");
            if let Known::Leds(t) = mice::find(pid) {
                if t.leds != m.leds {
                    say!(r, "  note: the tables say {} LEDs, Synapse {}", t.leds, m.leds);
                }
            }
        }
        Err(e) => say!(r, "  LED map: NO ({e}). Open Chroma Studio once with this mouse connected, close Synapse and run check again"),
    }
    let profiles = extract::profiles(&tmp, pid).unwrap_or_default();
    if profiles.is_empty() {
        say!(r, "  profiles: NO (no profile of this mouse in Synapse's logs)");
    } else {
        say!(r, "  profiles: OK, {}", profiles.iter().map(|p| format!("slot {} '{}'", p.slot, p.name)).collect::<Vec<_>>().join(", "));
    }
    match extract::effects(&tmp) {
        Ok(fx) => say!(r, "  Chroma Studio effects: OK, {}", fx.len()),
        Err(e) => say!(r, "  Chroma Studio effects: NO ({e})"),
    }

    say!(r, "\n[4] On-board profiles");
    let count = query(&ctl, 0x05, 0x80, 0x01);
    let slots = query(&ctl, 0x05, 0x81, 0x0A);
    let active = razerhid::get_active_profile(&ctl);
    say!(r, "  number of profiles (05/80): {}", count.map_or("NO".into(), |r| hex(&r[8..10])));
    say!(r, "  slots (05/81): {}", slots.map_or("NO".into(), |r| hex(&r[8..18])));
    say!(r, "  active slot (05/84): {}", active.map_or("NO".into(), |s| s.to_string()));
    let skip = cfg.as_ref().map_or(vec![1], |c| c.skip_slots.clone());
    if !profiles.is_empty() {
        say!(r, "  {}", slot1_note(&profiles, &skip));
    }
    let mut switch_ok = None;
    if let (Some(from), Some(resp)) = (active, slots) {
        // 05/81 = [capacity, slots...]: I try switching to another slot and back
        // preferably not slot 1 (Hybrid Storage on the 35K): the daemon jumps towards the normal slots
        let list: Vec<u8> = resp[9..9 + (resp[8] as usize).min(8)].iter().copied().filter(|&s| s != 0 && s != from).collect();
        if let Some(to) = list.iter().copied().find(|&s| s != 1).or(list.first().copied()) {
            let go = |slot: u8| razerhid::set_active_profile(&ctl, slot) && { sleep(Duration::from_millis(300)); razerhid::get_active_profile(&ctl) == Some(slot) };
            // a second attempt: if someone presses the button in the meantime the first one fails
            let ok = (0..2).any(|_| go(to) && go(from));
            if !ok {
                razerhid::set_active_profile(&ctl, from);
            }
            switch_ok = Some(ok);
            say!(r, "  switch slot {from} -> {to} -> {from} (05/04): {}", yes(ok));
        }
    }

    say!(r, "\n[5] Read commands (nothing is changed on the mouse)");
    for line in fingerprint(&ctl) {
        say!(r, "  {line}");
    }

    // the device's own answer comes before the tables of other projects
    let told = razerhid::zones(&ctl);
    if let Some((rows, cols)) = told.as_deref().and_then(razerhid::grid) {
        // a keyboard: no profile button under it, and a grid of keys instead of a row of LEDs
        let device = format!("PID 0x{pid:04X} {name}, firmware {}.{}, transaction id 0x{:02X}", resp[8], resp[9], razerhid::transaction_id());
        keyboard_steps(r, &ctl, keys, rows, cols, &device);
        return Ok(());
    }

    say!(r, "\n[6] Profile button");
    while keys.try_recv().is_ok() {} // an Enter typed earlier does not count
    tell(&format!("Press Enter, then press the profile button under the mouse a few times, for {BUTTON_SECONDS} seconds."));
    let _ = keys.recv(); // starts on Enter; without a keyboard it starts at once
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
                    say!(r, "  event {}", hex(&buf[..n.min(6)]));
                }
            }
        }
        if last_poll.elapsed().as_millis() >= 250 {
            last_poll = Instant::now();
            let s = razerhid::get_active_profile(&ctl);
            if s.is_some() && slots_seen.last() != Some(&s) {
                say!(r, "  active slot -> {}", s.unwrap_or(0));
                slots_seen.push(s);
            }
        }
        sleep(Duration::from_millis(5));
    }
    let button_ok = slots_seen.len() > 1;
    say!(r, "  event interfaces: {}, \"05 xx\" events: {n_events}, slot changed: {}", events.len(), yes(button_ok));
    if let (Some(from), true) = (active, button_ok) {
        razerhid::set_active_profile(&ctl, from); // I put the starting slot back
    }

    say!(r, "\n[7] LEDs: look at the mouse");
    let table = match mice::find(pid) {
        Known::Leds(m) => Some(m.leds),
        _ => None,
    };
    let told_leds = told.as_deref().and_then(razerhid::single_row);
    let shape = |&(id, rows, cols): &(u8, usize, usize)| match rows {
        1 => format!("{} {cols}", razerhid::zone_name(id)),
        _ => format!("{} {rows} rows x {cols} columns", razerhid::zone_name(id)),
    };
    match &told {
        Some(z) => say!(r, "  zones told by the device (0F/80), in LED order: {}", z.iter().map(shape).collect::<Vec<_>>().join(", ")),
        None => say!(r, "  zones told by the device (0F/80): NO"),
    }
    let (leds, source) = match (&map, told_leds, table) {
        (Ok((m, _)), _, _) => (m.leds, "Synapse"),
        (Err(_), Some(n), _) => (n, "the mouse"),
        (Err(_), None, Some(n)) => (n, "the tables"),
        (Err(_), None, None) => (razerhid::MAX_FRAME_LEDS, "unknown: trying up to 25"),
    };
    if let Some(n) = told_leds.filter(|&n| n != leds) {
        say!(r, "  note: the mouse says {n} LEDs, {source} {leds}");
    }
    let leds = leds.min(razerhid::MAX_FRAME_LEDS);
    say!(r, "  LED count: {leds} (from {source})");
    if let Known::Leds(m) = mice::find(pid) {
        if !m.zones.is_empty() {
            say!(r, "  zones in the tables: {} (the order is a guess, confirmed only on the Basilisk V3 35K)", m.zones);
        }
    }
    let answer = razerhid::custom_frame_status(&ctl, &vec![(255, 0, 0); leds]).map_or_else(|e| format!("NO, not sent ({e})"), status);
    say!(r, "  per-LED frame (0F/03): {answer}");
    tell("Look at the mouse: it turns red, then green, then blue.");
    show(&ctl, &vec![(255, 0, 0); leds], 1.0);
    show(&ctl, &vec![(0, 255, 0); leds], 1.0);
    show(&ctl, &vec![(0, 0, 255); leds], 1.0);
    while keys.try_recv().is_ok() {} // an Enter typed during the colours does not count
    tell("Now one LED at a time lights up white. For each one, type where it is on the mouse");
    tell("(logo, wheel, left side 1...) and press Enter. Enter alone = lit. n = nothing lit. q = stop.");
    let mut answers = Vec::new();
    let mut interactive = true;
    for n in 0..leds {
        let frame: Vec<Rgb> = (0..leds).map(|i| if i == n { (255, 255, 255) } else { BLACK }).collect();
        let s = if interactive {
            ask(&format!("  LED {} of {leds}:", n + 1));
            match hold(&ctl, &frame, keys) {
                Some(a) => understand(&a),
                None => {
                    interactive = false; // no keyboard: the walk goes on by itself, half a second per LED
                    println!();
                    show(&ctl, &frame, 0.5);
                    Seen::Lit("not asked".into())
                }
            }
        } else {
            show(&ctl, &frame, 0.5);
            Seen::Lit("not asked".into())
        };
        let text = match &s {
            Seen::Lit(w) if w.is_empty() => "lit".to_string(),
            Seen::Lit(w) => format!("lit ({w})"),
            Seen::NotLit => "NOT lit".to_string(),
            Seen::Stop => "stopped here".to_string(),
        };
        r.note(format!("  LED {} of {leds}: {text}", n + 1));
        if s == Seen::Stop {
            break;
        }
        answers.push(s);
        // without a known count, two dark LEDs in a row mean the mouse has no more
        if table.is_none() && told_leds.is_none() && map.is_err() && answers.len() >= 2 && answers[answers.len() - 2..].iter().all(|a| *a == Seen::NotLit) {
            break;
        }
    }
    show(&ctl, &vec![BLACK; leds], 0.2);
    let lit = answers.iter().filter(|a| matches!(a, Seen::Lit(_))).count();
    let dark = answers.iter().filter(|a| **a == Seen::NotLit).count();
    if table.is_none() && told_leds.is_none() && map.is_err() && interactive {
        say!(r, "  LED count found by hand: {}", count_from(&answers));
    }

    say!(r, "\n== Summary ==");
    say!(r, "device: PID 0x{pid:04X} {name}, firmware {}.{}, transaction id 0x{:02X}", resp[8], resp[9], razerhid::transaction_id());
    say!(r, "model: {}", describe(pid));
    say!(r, "connection: {}", connection.unwrap_or("unknown"));
    say!(r, "LED map from Synapse: {}", yes(map.is_ok()));
    say!(r, "profiles in Synapse's logs: {}", profiles.len());
    say!(r, "read active slot: {}", yes(active.is_some()));
    say!(r, "switch slot: {}", switch_ok.map_or("not tried (one slot only?)", yes));
    say!(r, "profile button seen: {}", yes(button_ok));
    say!(r, "per-LED frame: {answer}");
    if interactive {
        say!(r, "LEDs lit one at a time: {lit} of {}, not lit: {dark}", answers.len());
    } else {
        say!(r, "LEDs lit one at a time: not asked (no keyboard), say in the issue what you saw");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::Profile;

    #[test]
    fn answers() {
        assert_eq!(understand(""), Seen::Lit(String::new()));
        assert_eq!(understand("N"), Seen::NotLit);
        assert_eq!(understand("q"), Seen::Stop);
        assert_eq!(understand("left strip"), Seen::Lit("left strip".into()));
    }

    #[test]
    fn led_count_by_hand() {
        let l = || Seen::Lit(String::new());
        assert_eq!(count_from(&[l(), l(), l(), Seen::NotLit, Seen::NotLit]), 3);
        assert_eq!(count_from(&[l(), Seen::NotLit, l(), Seen::NotLit, Seen::NotLit]), 3, "a dark LED in the middle does not end the count");
        assert_eq!(count_from(&[Seen::NotLit, Seen::NotLit]), 0);
    }

    #[test]
    fn slot_one() {
        let p = |slot, name: &str| Profile { name: name.into(), slot };
        assert!(slot1_note(&[p(1, "Gaming"), p(2, "Work")], &[1]).starts_with("WARNING"), "a real profile on slot 1 would be skipped");
        assert!(!slot1_note(&[p(1, "Gaming")], &[]).starts_with("WARNING"));
        assert!(slot1_note(&[p(2, "Work"), p(3, "Gaming")], &[1]).starts_with("no Synapse profile on slot 1"));
    }

    #[test]
    fn picks_from_the_tables_when_synapse_does_not_know() {
        let seen = vec![(0x0090, "Razer Naga Pro".to_string()), (0x028F, "some keyboard".to_string())];
        let none = HashMap::new();
        assert_eq!(pick(&[], None, &seen, &none).unwrap().0, 0x0090);
        assert_eq!(pick(&["0x00CB".into()], None, &seen, &none).unwrap().0, 0x00CB, "the PID given wins");
        assert!(pick(&["zz".into()], None, &seen, &none).is_err());
        // a keyboard alone is checked without typing its PID; next to a mouse the mouse still wins
        let keyboard: HashMap<u16, String> = [(0x0287, "keyboard".to_string())].into();
        let alone = vec![(0x0287, "Razer BlackWidow V4".to_string())];
        assert_eq!(pick(&[], None, &alone, &keyboard).unwrap().0, 0x0287);
        let both = vec![(0x0090, "Razer Naga Pro".to_string()), (0x0287, "Razer BlackWidow V4".to_string())];
        assert_eq!(pick(&[], None, &both, &keyboard).unwrap().0, 0x0090);
    }
}
