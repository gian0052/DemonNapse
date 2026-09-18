//! Picco dell'uscita audio di sistema (IAudioMeterInformation): niente cattura, costo ~zero.

use std::time::{Duration, Instant};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

const REACQUIRE: Duration = Duration::from_secs(10);

pub struct PeakMeter {
    meter: Option<(IAudioMeterInformation, Instant)>,
}

impl PeakMeter {
    /// Va usato sempre dallo stesso thread che lo crea (COM viene inizializzato qui).
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
        // riaggancia ogni 10s: segue il cambio di dispositivo predefinito
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
                // dispositivo sparito, COM, ecc.
                debug!("audio: {e}");
                self.meter = None;
                0.0
            }
        }
    }
}
