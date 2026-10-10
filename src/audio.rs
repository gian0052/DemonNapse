//! Peak of the system audio output (IAudioMeterInformation): no capture, ~zero cost.

use std::time::{Duration, Instant};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

const REACQUIRE: Duration = Duration::from_secs(10);

pub struct PeakMeter {
    meter: Option<(IAudioMeterInformation, Instant)>,
}

impl PeakMeter {
    /// Must always be used from the same thread that creates it (COM is initialised here).
    pub fn new() -> PeakMeter {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        PeakMeter { meter: None }
    }

    fn acquire() -> windows::core::Result<IAudioMeterInformation> {
        unsafe {
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            devices.GetDefaultAudioEndpoint(eRender, eMultimedia)?.Activate(CLSCTX_ALL, None)
        }
    }

    pub fn peak(&mut self) -> f64 {
        // re-acquires every 10s: follows a change of the default device
        if !self.meter.as_ref().is_some_and(|(_, t)| t.elapsed() < REACQUIRE) {
            self.meter = None;
            match Self::acquire() {
                Ok(m) => self.meter = Some((m, Instant::now())),
                Err(e) => debug!("audio: {e}"),
            }
        }
        let Some((meter, _)) = &self.meter else { return 0.0 };
        match unsafe { meter.GetPeakValue() } {
            Ok(v) => v as f64,
            Err(e) => {
                // device gone, COM, etc.
                debug!("audio: {e}");
                self.meter = None;
                0.0
            }
        }
    }
}
