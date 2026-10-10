//! Living alongside Synapse: telling whether it is open, closing the Razer processes left hanging,
//! stopping and restarting the Razer services (as administrator only).

use std::sync::Mutex;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::ProcessStatus::EnumProcesses;
use windows::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus, StartServiceW, SC_MANAGER_CONNECT,
    SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_START, SERVICE_STATUS, SERVICE_STOP,
};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::UI::Shell::IsUserAnAdmin;

const SYNAPSE_EXES: &[&str] = &["razerappengine.exe"]; // Synapse 4 and Chroma App
const RAZER_EXES: &[&str] = &["razerappengine.exe", "razeraxon.exe"]; // to be killed if they are left hanging
pub const RAZER_SERVICES: &[&str] = &[
    "RzActionSvc",
    "RazerExperienceService",
    "Razer Game Manager Service 3",
    "RGS",
    "Razer Chroma SDK Server",
    "Razer Chroma SDK Service",
    "Razer Chroma SDK Diagnostic Service",
    "Razer Chroma Stream Server",
    "Razer Elevation Service",
];

/// (pid, name) of the processes whose executable is in `names` (lowercase).
fn processes(names: &[&str]) -> Vec<(u32, String)> {
    let mut found = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return found };
        let mut pe = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap, &mut pe);
        while more.is_ok() {
            let len = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
            let name = String::from_utf16_lossy(&pe.szExeFile[..len]);
            if names.contains(&name.to_lowercase().as_str()) {
                found.push((pe.th32ProcessID, name));
            }
            more = Process32NextW(snap, &mut pe);
        }
        let _ = CloseHandle(snap);
    }
    found
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() }.as_bool()
}

/// Is Synapse open? Called often: the full process list (a few ms) is re-read
/// only when a new pid appears, otherwise the list of pids is enough.
pub fn running() -> bool {
    static SEEN: Mutex<(Vec<u32>, Vec<u32>)> = Mutex::new((Vec::new(), Vec::new())); // pids already seen, Synapse's pids
    let mut pids = vec![0u32; 4096];
    let mut bytes = 0;
    if unsafe { EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut bytes) }.is_err() {
        return !processes(SYNAPSE_EXES).is_empty();
    }
    pids.truncate(bytes as usize / 4);
    let mut seen = SEEN.lock().unwrap();
    if pids.iter().any(|p| !seen.0.contains(p)) {
        seen.1 = processes(SYNAPSE_EXES).into_iter().map(|(pid, _)| pid).collect();
    }
    seen.1.retain(|p| pids.contains(p));
    seen.0 = pids;
    !seen.1.is_empty()
}

pub fn kill(names: &[&str]) {
    for (pid, name) in processes(names) {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                if TerminateProcess(h, 1).is_ok() {
                    info!("killed {name} (pid {pid})");
                }
                let _ = CloseHandle(h);
            }
        }
    }
}

pub fn kill_razer_apps() {
    kill(RAZER_EXES);
}

/// Starts or stops the Razer services, without waiting for them to finish (like `sc start` / `sc stop`).
/// Requires administrator privileges.
pub fn services(start: bool) {
    let action = if start { "start" } else { "stop" };
    if !is_admin() {
        info!("not administrator: Razer services left as they are ({action})");
        return;
    }
    unsafe {
        let Ok(scm) = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) else { return };
        for name in RAZER_SERVICES {
            let Ok(svc) = OpenServiceW(scm, &HSTRING::from(*name), SERVICE_START | SERVICE_STOP) else { continue };
            let _ = if start { StartServiceW(svc, None) } else { ControlService(svc, SERVICE_CONTROL_STOP, &mut SERVICE_STATUS::default()) };
            let _ = CloseServiceHandle(svc);
        }
        let _ = CloseServiceHandle(scm);
    }
    info!("Razer services: {action}");
}

/// State of every Razer service (`None` = not installed), for the verification command.
pub fn service_states() -> Vec<(&'static str, Option<u32>)> {
    let mut out = Vec::new();
    unsafe {
        let Ok(scm) = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) else { return out };
        for name in RAZER_SERVICES {
            let mut status = SERVICE_STATUS::default();
            let state = OpenServiceW(scm, &HSTRING::from(*name), SERVICE_QUERY_STATUS).ok().and_then(|svc| {
                let ok = QueryServiceStatus(svc, &mut status).is_ok();
                let _ = CloseServiceHandle(svc);
                ok.then_some(status.dwCurrentState.0)
            });
            out.push((*name, state));
        }
        let _ = CloseServiceHandle(scm);
    }
    out
}
