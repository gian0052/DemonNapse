//! Log con data e ora (`AAAA-MM-GG HH:MM:SS messaggio`): su stderr, oppure su file quando il
//! demone gira senza console.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};
use windows::Win32::System::SystemInformation::GetLocalTime;

static FILE: Mutex<Option<File>> = Mutex::new(None);
pub static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Da qui in poi il log va in coda a questo file invece che su stderr.
fn to_file(path: &Path) {
    if let Ok(f) = File::options().create(true).append(true).open(path) {
        *FILE.lock().unwrap() = Some(f);
    }
}

pub fn write(args: std::fmt::Arguments) {
    let t = unsafe { GetLocalTime() };
    let line = format!("{}-{:02}-{:02} {:02}:{:02}:{:02} {args}\n", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond);
    match FILE.lock().unwrap().as_mut() {
        Some(f) => drop(f.write_all(line.as_bytes())),
        None => drop(std::io::stderr().write_all(line.as_bytes())),
    }
}

macro_rules! info {
    ($($a:tt)*) => { crate::log::write(format_args!($($a)*)) };
}

macro_rules! debug {
    ($($a:tt)*) => {
        if crate::log::VERBOSE.load(std::sync::atomic::Ordering::Relaxed) {
            crate::log::write(format_args!($($a)*))
        }
    };
}

/// Log del demone: su stderr se c'e' (lanciato da un terminale), altrimenti in coda a `file`.
pub fn init(file: &Path, verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
    let has_stderr = unsafe { GetStdHandle(STD_ERROR_HANDLE) }.is_ok_and(|h| !h.is_invalid() && !h.0.is_null());
    if !has_stderr {
        to_file(file);
    }
}
