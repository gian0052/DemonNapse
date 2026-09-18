//! Log nel formato del demone Python (`HH:MM:SS messaggio`): su stderr, oppure su file quando
//! il demone gira senza console.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::Win32::System::SystemInformation::GetLocalTime;

static FILE: Mutex<Option<File>> = Mutex::new(None);
pub static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Da qui in poi il log va in coda a questo file invece che su stderr.
pub fn to_file(path: &Path) {
    if let Ok(f) = File::options().create(true).append(true).open(path) {
        *FILE.lock().unwrap() = Some(f);
    }
}

pub fn write(args: std::fmt::Arguments) {
    let t = unsafe { GetLocalTime() };
    let line = format!("{:02}:{:02}:{:02} {args}\n", t.wHour, t.wMinute, t.wSecond);
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

#[allow(dead_code)]
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
}
