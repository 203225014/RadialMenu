//! «Играет ли звук»: пиковый уровень на устройстве вывода по умолчанию.
//! Опрос идёт только пока меню на экране — в простое ни потока, ни пробуждений.

use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};

const THRESHOLD: f32 = 0.002; // пик 0..1
const HOLD: f64 = 1.5; // сек: пауза между треками не считается выключением
const POLL: f64 = 0.1; // сек между опросами, пока меню открыто
const REOPEN: f64 = 10.0; // сек: устройство вывода могли сменить

pub struct AudioMeter {
    meter: Option<IAudioMeterInformation>,
    opened: f64,
    last_poll: f64,
    last_sound: f64,
}

fn open() -> Option<IAudioMeterInformation> {
    unsafe {
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        dev.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
            .ok()
    }
}

impl AudioMeter {
    pub fn new() -> AudioMeter {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        AudioMeter {
            meter: None,
            opened: f64::NEG_INFINITY,
            last_poll: f64::NEG_INFINITY,
            last_sound: f64::NEG_INFINITY,
        }
    }

    /// Освободить COM-объекты, пока меню не нужно.
    pub fn release(&mut self) {
        self.meter = None;
        self.opened = f64::NEG_INFINITY;
    }

    /// Обновить состояние (не чаще `POLL`, если не `force`) и вернуть «играет».
    pub fn playing(&mut self, now: f64, force: bool) -> bool {
        if force || now - self.last_poll >= POLL {
            self.last_poll = now;
            if self.meter.is_none() || now - self.opened > REOPEN {
                self.meter = open();
                self.opened = now;
            }
            if let Some(m) = &self.meter {
                match unsafe { m.GetPeakValue() } {
                    Ok(peak) if peak > THRESHOLD => self.last_sound = now,
                    Ok(_) => {}
                    Err(_) => self.meter = None,
                }
            }
        }
        now - self.last_sound < HOLD
    }
}
