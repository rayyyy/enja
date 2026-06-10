//! マイク録音スレッド・AEC・システム音声ミュート/分離。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) const RECORDING_STOP_NOTIFY_TIMEOUT: Duration = Duration::from_millis(250);

pub(crate) enum AudioAux {
    Mute(SystemAudioMuteGuard),
    Isolate(#[allow(dead_code)] Arc<SystemTap>),
}

impl AudioAux {
    pub(crate) fn stop(self) {
        match self {
            AudioAux::Mute(guard) => guard.stop(),
            AudioAux::Isolate(_) => {}
        }
    }
}

#[derive(Clone)]
pub(crate) enum PipelineMode {
    Direct,
    AecIsolate(Arc<SystemTap>),
}

pub(crate) struct Recorder {
    pub(crate) control_tx: std::sync::mpsc::Sender<RecorderCommand>,
    pub(crate) stopped_rx: std::sync::mpsc::Receiver<()>,
    pub(crate) done_rx: std::sync::mpsc::Receiver<Result<AudioClip, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecorderCommand {
    Finish,
    Cancel,
}

pub(crate) struct AudioClip {
    pub(crate) wav: Vec<u8>,
    pub(crate) duration_secs: f32,
    pub(crate) live_transcript: Option<LiveTranscript>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputAudioSnapshot {
    pub(crate) volume: Option<u8>,
    pub(crate) muted: Option<bool>,
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputAudioSnapshot;

pub(crate) struct SystemAudioMuteGuard {
    pub(crate) snapshot: OutputAudioSnapshot,
    pub(crate) stop_tx: std::sync::mpsc::Sender<()>,
    pub(crate) join: Option<std::thread::JoinHandle<()>>,
}

pub(crate) type RecorderSetup = (
    Arc<Mutex<Vec<i16>>>,
    u32,
    u16,
    cpal::Stream,
    Option<LiveTranscriber>,
);

pub(crate) type RecorderInitSignal =
    Arc<Mutex<Option<std::sync::mpsc::Sender<Result<(), String>>>>>;

pub(crate) fn prepare_audio_pipeline(settings: &AppSettings) -> (Option<AudioAux>, PipelineMode) {
    match settings.voice.system_audio_handling {
        SystemAudioHandling::Mute => (
            Some(AudioAux::Mute(SystemAudioMuteGuard::start())),
            PipelineMode::Direct,
        ),
        SystemAudioHandling::Isolate => match SystemTap::start() {
            Ok(capture) => {
                let shared = Arc::new(capture);
                (
                    Some(AudioAux::Isolate(shared.clone())),
                    PipelineMode::AecIsolate(shared),
                )
            }
            Err(err) => {
                eprintln!("[enja] システム音声分離の開始に失敗しました: {err}");
                (
                    Some(AudioAux::Mute(SystemAudioMuteGuard::start())),
                    PipelineMode::Direct,
                )
            }
        },
        SystemAudioHandling::Off => (None, PipelineMode::Direct),
    }
}

pub fn prewarm_microphone() {
    let host = cpal::default_host();
    if let Some(device) = host.default_input_device() {
        let _ = device.default_input_config();
    }
    let _ = list_audio_input_devices();
}

impl Recorder {
    pub(crate) fn start(
        app: tauri::AppHandle,
        selected_device_id: Option<String>,
        max_recording_seconds: u64,
        pipeline: PipelineMode,
        live_transcription_provider: Option<LiveTranscriptionProvider>,
        screen_context: VoiceScreenContext,
    ) -> Result<Self, String> {
        let (control_tx, control_rx) = std::sync::mpsc::channel::<RecorderCommand>();
        let (stopped_tx, stopped_rx) = std::sync::mpsc::channel::<()>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<AudioClip, String>>();
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::spawn(move || {
            let result = run_recording_thread(
                app.clone(),
                selected_device_id,
                max_recording_seconds,
                control_rx,
                stopped_tx,
                init_tx,
                pipeline,
                live_transcription_provider,
                screen_context,
            );
            let _ = done_tx.send(result);
        });
        init_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "マイクの初期化がタイムアウトしました。".to_string())??;
        Ok(Self {
            control_tx,
            stopped_rx,
            done_rx,
        })
    }

    pub(crate) fn finish(
        self,
        after_recording_stopped: impl FnOnce(),
    ) -> Result<AudioClip, String> {
        let _ = self.control_tx.send(RecorderCommand::Finish);
        let _ = self.stopped_rx.recv_timeout(RECORDING_STOP_NOTIFY_TIMEOUT);
        after_recording_stopped();
        let result = match self.done_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => result,
            Err(_) => Err("録音停止処理がタイムアウトしました。".to_string()),
        };
        result
    }

    pub(crate) fn cancel(self) {
        let _ = self.control_tx.send(RecorderCommand::Cancel);
        let _ = self.done_rx.recv_timeout(Duration::from_secs(2));
    }
}

impl SystemAudioMuteGuard {
    pub(crate) fn start() -> Self {
        let snapshot = current_output_audio_snapshot();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let join = std::thread::spawn(move || {
            mute_system_output();
            loop {
                match stop_rx.recv_timeout(Duration::from_millis(450)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => mute_system_output(),
                }
            }
        });
        Self {
            snapshot,
            stop_tx,
            join: Some(join),
        }
    }

    pub(crate) fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        restore_system_output(self.snapshot);
    }
}

impl Drop for SystemAudioMuteGuard {
    fn drop(&mut self) {
        if self.join.is_some() {
            let _ = self.stop_tx.send(());
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
            restore_system_output(self.snapshot);
        }
    }
}

pub(crate) fn run_recording_thread(
    app: tauri::AppHandle,
    selected_device_id: Option<String>,
    max_recording_seconds: u64,
    control_rx: std::sync::mpsc::Receiver<RecorderCommand>,
    stopped_tx: std::sync::mpsc::Sender<()>,
    init_tx: std::sync::mpsc::Sender<Result<(), String>>,
    pipeline: PipelineMode,
    live_transcription_provider: Option<LiveTranscriptionProvider>,
    screen_context: VoiceScreenContext,
) -> Result<AudioClip, String> {
    let init_signal: RecorderInitSignal = Arc::new(Mutex::new(Some(init_tx)));
    let setup: Result<RecorderSetup, String> = (|| {
        let host = cpal::default_host();
        let device = input_device_by_id(&host, selected_device_id.as_deref())?
            .or_else(|| host.default_input_device())
            .ok_or_else(|| "利用できるマイクが見つかりません。".to_string())?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let device_sample_rate = supported.sample_rate().0;
        let device_channels = supported.channels();
        let config: cpal::StreamConfig = supported.clone().into();
        let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
        let (output_sample_rate, output_channels) = match &pipeline {
            PipelineMode::Direct => (device_sample_rate, device_channels),
            PipelineMode::AecIsolate(_) => (aec::SAMPLE_RATE, 1u16),
        };
        let max_samples = (output_sample_rate as usize)
            * (output_channels as usize)
            * max_recording_seconds.clamp(5, 600) as usize;
        let live_transcriber = live_transcription_provider.and_then(|provider| {
            match start_live_transcriber(
                &app,
                provider,
                output_sample_rate,
                output_channels,
                &screen_context,
            ) {
                Ok(transcriber) => Some(transcriber),
                Err(err) => {
                    eprintln!("[enja] live transcription unavailable: {err}");
                    None
                }
            }
        });
        let live_sample_tx = live_transcriber
            .as_ref()
            .and_then(LiveTranscriber::sample_sender);
        let last_emit = Arc::new(Mutex::new(Instant::now()));
        let err_fn = |err| eprintln!("[enja] audio input stream error: {err}");

        let aec_pipeline = match &pipeline {
            PipelineMode::Direct => None,
            PipelineMode::AecIsolate(system) => Some(AecPipeline::new(
                system.clone(),
                device_sample_rate,
                device_channels,
            )?),
        };

        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app.clone(),
                device_channels,
                aec_pipeline,
                live_sample_tx.clone(),
                init_signal.clone(),
                err_fn,
            ),
            cpal::SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app.clone(),
                device_channels,
                aec_pipeline,
                live_sample_tx.clone(),
                init_signal.clone(),
                err_fn,
            ),
            cpal::SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app.clone(),
                device_channels,
                aec_pipeline,
                live_sample_tx.clone(),
                init_signal.clone(),
                err_fn,
            ),
            _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        Ok((
            samples,
            output_sample_rate,
            output_channels,
            stream,
            live_transcriber,
        ))
    })();

    let (samples, sample_rate, channels, stream, live_transcriber) = match setup {
        Ok(values) => values,
        Err(err) => {
            send_recorder_init(&init_signal, Err(err.clone()));
            return Err(err);
        }
    };

    let max_wait = Duration::from_secs(max_recording_seconds.clamp(5, 600));
    let command = match control_rx.recv_timeout(max_wait) {
        Ok(command) => command,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => RecorderCommand::Finish,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => RecorderCommand::Cancel,
    };
    drop(stream);
    let _ = stopped_tx.send(());

    if command == RecorderCommand::Cancel {
        if let Some(transcriber) = live_transcriber {
            transcriber.cancel();
        }
        return Err("録音をキャンセルしました。".to_string());
    }

    let live_transcript = live_transcriber.map(|transcriber| {
        let provider = transcriber.provider;
        LiveTranscript {
            provider,
            result: transcriber.finish(),
        }
    });

    let samples = samples.lock().map_err(|e| e.to_string())?.clone();
    if samples.is_empty() {
        return Err("音声が録音されていません。".to_string());
    }

    let prepared = prepare_recorded_audio_for_api(&samples, sample_rate, channels)?;
    let wav = samples_to_wav(&prepared.samples, sample_rate, channels)?;
    Ok(AudioClip {
        wav,
        duration_secs: prepared.analysis.duration_secs,
        live_transcript,
    })
}

pub(crate) fn send_recorder_init(signal: &RecorderInitSignal, result: Result<(), String>) {
    if let Ok(mut guard) = signal.lock() {
        if let Some(tx) = guard.take() {
            let _ = tx.send(result);
        }
    }
}

pub(crate) struct AecPipeline {
    pub(crate) aec: Aec,
    pub(crate) system: Arc<SystemTap>,
    pub(crate) step: f64,
    pub(crate) next_read: f64,
    pub(crate) input_count: u64,
    pub(crate) prev_in: f32,
    pub(crate) mic_frame: Vec<f32>,
}

impl AecPipeline {
    fn new(
        system: Arc<SystemTap>,
        device_sample_rate: u32,
        _device_channels: u16,
    ) -> Result<Self, String> {
        Ok(Self {
            aec: Aec::new()?,
            system,
            step: device_sample_rate as f64 / aec::SAMPLE_RATE as f64,
            next_read: 0.0,
            input_count: 0,
            prev_in: 0.0,
            mic_frame: Vec::with_capacity(aec::FRAME_SAMPLES * 4),
        })
    }

    fn push_mono(&mut self, sample: f32) {
        let cur = self.input_count as f64;
        let prev = cur - 1.0;
        while self.next_read <= cur {
            let frac = (self.next_read - prev) as f32;
            let value = self.prev_in + (sample - self.prev_in) * frac;
            self.mic_frame.push(value.clamp(-1.0, 1.0));
            self.next_read += self.step;
        }
        self.prev_in = sample;
        self.input_count += 1;
    }

    fn drain_frames<F: FnMut(&[f32])>(&mut self, mut emit: F) {
        while self.mic_frame.len() >= aec::FRAME_SAMPLES {
            let mut frame: Vec<f32> = self.mic_frame.drain(..aec::FRAME_SAMPLES).collect();
            let reference = self.system.take_reference(aec::FRAME_SAMPLES);
            if let Err(err) = self.aec.process(&mut frame, &reference) {
                eprintln!("[enja] AEC処理に失敗: {err}");
            }
            emit(&frame);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<i16>>>,
    last_emit: Arc<Mutex<Instant>>,
    max_samples: usize,
    app: tauri::AppHandle,
    device_channels: u16,
    mut aec_pipeline: Option<AecPipeline>,
    live_sample_tx: Option<std::sync::mpsc::Sender<Vec<i16>>>,
    init_signal: RecorderInitSignal,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::Sample + cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let chan = device_channels.max(1) as usize;
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            send_recorder_init(&init_signal, Ok(()));
            let mut peak = 0.0f32;
            let mut sum = 0.0f32;
            let mut count = 0usize;

            if let Some(pipeline) = aec_pipeline.as_mut() {
                for chunk in data.chunks(chan) {
                    let mut mono = 0.0_f32;
                    for sample in chunk {
                        mono += f32::from_sample(*sample);
                    }
                    let mono = (mono / chunk.len().max(1) as f32).clamp(-1.0, 1.0);
                    peak = peak.max(mono.abs());
                    sum += mono * mono;
                    count += 1;
                    pipeline.push_mono(mono);
                }
                let samples_buf = samples.clone();
                let live_tx = live_sample_tx.clone();
                pipeline.drain_frames(|frame| {
                    let mut live_samples = Vec::new();
                    if let Ok(mut guard) = samples_buf.lock() {
                        let remaining = max_samples.saturating_sub(guard.len());
                        for value in frame.iter().take(remaining) {
                            let sample = (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                            guard.push(sample);
                            live_samples.push(sample);
                        }
                    }
                    if !live_samples.is_empty() {
                        if let Some(tx) = live_tx.as_ref() {
                            let _ = tx.send(live_samples);
                        }
                    }
                });
            } else if let Ok(mut guard) = samples.lock() {
                let remaining = max_samples.saturating_sub(guard.len());
                let mut live_samples = Vec::new();
                for sample in data.iter().take(remaining) {
                    let value = f32::from_sample(*sample).clamp(-1.0, 1.0);
                    peak = peak.max(value.abs());
                    sum += value * value;
                    count += 1;
                    let pcm = (value * i16::MAX as f32) as i16;
                    guard.push(pcm);
                    live_samples.push(pcm);
                }
                if !live_samples.is_empty() {
                    if let Some(tx) = live_sample_tx.as_ref() {
                        let _ = tx.send(live_samples);
                    }
                }
            }

            if count > 0 {
                let rms = (sum / count as f32).sqrt().clamp(0.0, 1.0);
                if let Ok(mut last) = last_emit.lock() {
                    if last.elapsed() >= Duration::from_millis(45) {
                        *last = Instant::now();
                        let _ = app.emit(
                            "voice-level",
                            VoiceLevelEvent {
                                rms,
                                peak: peak.clamp(0.0, 1.0),
                            },
                        );
                    }
                }
            }
        },
        err_fn,
        None,
    )
}

pub(crate) fn play_interaction_sound(kind: &str) {
    #[cfg(target_os = "macos")]
    {
        let sound = if kind == "start" { "Pop" } else { "Tink" };
        let path = format!("/System/Library/Sounds/{sound}.aiff");
        let _ = std::process::Command::new("afplay").arg(path).spawn();
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn current_output_audio_snapshot() -> OutputAudioSnapshot {
    OutputAudioSnapshot {
        volume: read_output_volume(),
        muted: read_output_muted(),
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn current_output_audio_snapshot() -> OutputAudioSnapshot {
    OutputAudioSnapshot
}

#[cfg(target_os = "macos")]
pub(crate) fn read_osascript_value(script: &str) -> Option<String> {
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn read_output_volume() -> Option<u8> {
    read_osascript_value("output volume of (get volume settings)")?
        .parse()
        .ok()
}

#[cfg(target_os = "macos")]
pub(crate) fn read_output_muted() -> Option<bool> {
    match read_osascript_value("output muted of (get volume settings)")?
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn mute_system_output() {
    let _ = std::process::Command::new("osascript")
        .args([
            "-e",
            "set volume with output muted",
            "-e",
            "set volume output volume 0",
            "-e",
            "set volume with output muted",
        ])
        .output();
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn mute_system_output() {}

#[cfg(target_os = "macos")]
pub(crate) fn restore_system_output(snapshot: OutputAudioSnapshot) {
    if let Some(volume) = snapshot.volume {
        set_output_volume(volume);
    }
    if let Some(muted) = snapshot.muted {
        set_output_muted(muted);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn restore_system_output(_snapshot: OutputAudioSnapshot) {}

#[cfg(target_os = "macos")]
pub(crate) fn set_output_muted(muted: bool) {
    let script = if muted {
        "set volume with output muted"
    } else {
        "set volume without output muted"
    };
    let _ = std::process::Command::new("osascript")
        .args(["-e", script])
        .output();
}

#[cfg(target_os = "macos")]
pub(crate) fn set_output_volume(volume: u8) {
    let script = format!("set volume output volume {}", volume.min(100));
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output();
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_output_volume(_volume: u8) {}
