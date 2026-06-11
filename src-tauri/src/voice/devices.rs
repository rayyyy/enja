//! 音声入力デバイスの列挙・選択・変更監視。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) const AUDIO_INPUT_DEVICES_CHANGED_EVENT: &str = "audio-input-devices-changed";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn spawn_audio_input_device_watcher(app: tauri::AppHandle) {
    audio_input_device_watcher::spawn(app);
}

pub fn list_audio_input_devices() -> Result<Vec<AudioInputDevice>, String> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let mut name_counts = std::collections::HashMap::<String, usize>::new();
    let mut entries = Vec::new();

    for (idx, device) in host.input_devices().map_err(|e| e.to_string())?.enumerate() {
        let name = device
            .name()
            .unwrap_or_else(|_| "名称未取得のマイク".to_string());
        if name == "Enja Tap" {
            continue;
        }
        *name_counts.entry(name.clone()).or_insert(0) += 1;
        entries.push((idx, name));
    }

    let mut out = Vec::with_capacity(entries.len());
    for (idx, name) in entries {
        let is_default = default_name
            .as_deref()
            .is_some_and(|default| default == name && name_counts.get(&name) == Some(&1));
        out.push(AudioInputDevice {
            id: format!("{name}#{idx}"),
            is_default,
            name,
        });
    }
    Ok(out)
}

pub(crate) fn parse_audio_input_device_id(selected_id: &str) -> Option<&str> {
    let hash_index = selected_id.rfind('#')?;
    if hash_index == 0 {
        return None;
    }
    selected_id[hash_index + 1..]
        .parse::<usize>()
        .ok()
        .map(|_| &selected_id[..hash_index])
}

pub(crate) fn input_device_by_id(
    host: &cpal::Host,
    selected_id: Option<&str>,
) -> Result<Option<cpal::Device>, String> {
    let Some(selected_id) = selected_id else {
        return Ok(None);
    };

    let selected_name = parse_audio_input_device_id(selected_id);
    let mut same_name_match = None;
    let mut same_name_count = 0usize;

    for (idx, device) in host.input_devices().map_err(|e| e.to_string())?.enumerate() {
        let name = device.name().unwrap_or_default();
        if selected_id == format!("{name}#{idx}") {
            return Ok(Some(device));
        }
        if selected_name == Some(name.as_str()) {
            same_name_count += 1;
            same_name_match = Some(device);
        }
    }
    if same_name_count == 1 {
        return Ok(same_name_match);
    }
    Ok(None)
}

pub(crate) fn audio_input_devices_signature(
    devices: &[AudioInputDevice],
) -> Vec<(String, String, bool)> {
    devices
        .iter()
        .map(|device| (device.id.clone(), device.name.clone(), device.is_default))
        .collect()
}

pub(crate) fn poll_audio_input_devices(app: tauri::AppHandle, interval: Duration) {
    let mut last_signature = list_audio_input_devices()
        .map(|devices| audio_input_devices_signature(&devices))
        .unwrap_or_default();

    loop {
        std::thread::sleep(interval);
        let devices = match list_audio_input_devices() {
            Ok(devices) => devices,
            Err(err) => {
                eprintln!("[enja] audio input device refresh failed: {err}");
                continue;
            }
        };
        let signature = audio_input_devices_signature(&devices);
        if signature != last_signature {
            last_signature = signature;
            let _ = app.emit(AUDIO_INPUT_DEVICES_CHANGED_EVENT, devices);
        }
    }
}

#[cfg(target_os = "macos")]
mod audio_input_device_watcher {
    use super::{
        audio_input_devices_signature, list_audio_input_devices, poll_audio_input_devices,
        AUDIO_INPUT_DEVICES_CHANGED_EVENT,
    };
    use coreaudio_sys::{
        kAudioHardwareNoError, kAudioHardwarePropertyDefaultInputDevice,
        kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMaster,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectAddPropertyListener,
        AudioObjectPropertyAddress, AudioObjectRemovePropertyListener, OSStatus,
    };
    use std::ffi::c_void;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::Duration;
    use tauri::Emitter;

    const REFRESH_DEBOUNCE: Duration = Duration::from_millis(300);

    pub fn spawn(app: tauri::AppHandle) {
        std::thread::spawn(move || {
            let (tx, rx) = mpsc::channel();
            let _listeners = match CoreAudioDeviceListeners::register(tx) {
                Ok(listeners) => listeners,
                Err(err) => {
                    eprintln!("[enja] CoreAudio device watcher unavailable: {err}");
                    poll_audio_input_devices(app, Duration::from_secs(2));
                    return;
                }
            };

            run_event_loop(app, rx);
        });
    }

    fn run_event_loop(app: tauri::AppHandle, rx: Receiver<()>) {
        let mut last_signature = list_audio_input_devices()
            .map(|devices| audio_input_devices_signature(&devices))
            .unwrap_or_default();

        while rx.recv().is_ok() {
            std::thread::sleep(REFRESH_DEBOUNCE);
            while rx.try_recv().is_ok() {}

            let devices = match list_audio_input_devices() {
                Ok(devices) => devices,
                Err(err) => {
                    eprintln!("[enja] audio input device refresh failed: {err}");
                    continue;
                }
            };
            let signature = audio_input_devices_signature(&devices);
            if signature != last_signature {
                last_signature = signature;
                let _ = app.emit(AUDIO_INPUT_DEVICES_CHANGED_EVENT, devices);
            }
        }
    }

    struct CoreAudioDeviceListeners {
        client_data: *mut Sender<()>,
        devices_registered: bool,
        default_input_registered: bool,
    }

    impl CoreAudioDeviceListeners {
        fn register(tx: Sender<()>) -> Result<Self, String> {
            let mut listeners = Self {
                client_data: Box::into_raw(Box::new(tx)),
                devices_registered: false,
                default_input_registered: false,
            };

            unsafe {
                listeners.add_listener(kAudioHardwarePropertyDevices)?;
                listeners.devices_registered = true;
                listeners.add_listener(kAudioHardwarePropertyDefaultInputDevice)?;
                listeners.default_input_registered = true;
            }

            Ok(listeners)
        }

        unsafe fn add_listener(&self, selector: u32) -> Result<(), String> {
            let address = listener_address(selector);
            let status = AudioObjectAddPropertyListener(
                kAudioObjectSystemObject,
                &address,
                Some(audio_device_listener),
                self.client_data.cast::<c_void>(),
            );
            if status == kAudioHardwareNoError as OSStatus {
                Ok(())
            } else {
                Err(format!(
                    "AudioObjectAddPropertyListener({selector}) returned {status}"
                ))
            }
        }

        unsafe fn remove_listener(&self, selector: u32) {
            let address = listener_address(selector);
            let _ = AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject,
                &address,
                Some(audio_device_listener),
                self.client_data.cast::<c_void>(),
            );
        }
    }

    impl Drop for CoreAudioDeviceListeners {
        fn drop(&mut self) {
            unsafe {
                if self.default_input_registered {
                    self.remove_listener(kAudioHardwarePropertyDefaultInputDevice);
                }
                if self.devices_registered {
                    self.remove_listener(kAudioHardwarePropertyDevices);
                }
                drop(Box::from_raw(self.client_data));
            }
        }
    }

    fn listener_address(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        }
    }

    unsafe extern "C" fn audio_device_listener(
        _object_id: u32,
        _address_count: u32,
        _addresses: *const AudioObjectPropertyAddress,
        client_data: *mut c_void,
    ) -> OSStatus {
        if client_data.is_null() {
            return kAudioHardwareNoError as OSStatus;
        }
        let tx = &*(client_data as *const Sender<()>);
        let _ = tx.send(());
        kAudioHardwareNoError as OSStatus
    }
}

#[cfg(not(target_os = "macos"))]
mod audio_input_device_watcher {
    use super::poll_audio_input_devices;
    use std::time::Duration;

    pub fn spawn(app: tauri::AppHandle) {
        std::thread::spawn(move || poll_audio_input_devices(app, Duration::from_secs(2)));
    }
}

pub async fn check_speech_profile_setup(
    app: &tauri::AppHandle,
    profile: SpeechProfile,
    settings: AppSettings,
) -> Result<SpeechSetupCheck, String> {
    match profile {
        SpeechProfile::GoogleChirp3 => check_google_chirp3_setup(&settings).await,
        SpeechProfile::OpenAiGpt4oTranscribe | SpeechProfile::OpenAiGpt4oMiniTranscribe => {
            Ok(check_secret_setup(
                "OpenAI APIキー",
                "openai",
                "OpenAI APIキーが保存されています。",
                "OpenAI APIキーを保存してください。",
            ))
        }
        SpeechProfile::GeminiAudio => Ok(check_secret_setup(
            "Gemini APIキー",
            "gemini",
            "Gemini APIキーが保存されています。",
            "Gemini APIキーを保存してください。",
        )),
        SpeechProfile::AppleSpeechAnalyzer => {
            let status = apple_speech_status(app, true)?;
            Ok(apple_speech_setup_check(&status))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_audio_input_device_id_reads_trailing_index_only() {
        assert_eq!(
            parse_audio_input_device_id("MacBook Pro Microphone#0"),
            Some("MacBook Pro Microphone")
        );
        assert_eq!(
            parse_audio_input_device_id("Studio #1#2"),
            Some("Studio #1")
        );
        assert_eq!(parse_audio_input_device_id("invalid"), None);
        assert_eq!(parse_audio_input_device_id("Mic#abc"), None);
    }
}
