use webrtc_audio_processing::config::{
    EchoCanceller, HighPassFilter, NoiseSuppression, NoiseSuppressionLevel,
};
use webrtc_audio_processing::{Config, Processor};

pub const SAMPLE_RATE: u32 = 16_000;
pub const FRAME_SAMPLES: usize = 160;

const INITIAL_STREAM_DELAY_MS: u16 = 60;

pub struct Aec {
    processor: Processor,
}

impl Aec {
    pub fn new() -> Result<Self, String> {
        let processor =
            Processor::new(SAMPLE_RATE).map_err(|e| format!("AEC初期化に失敗: {e:?}"))?;
        processor.set_config(Config {
            echo_canceller: Some(EchoCanceller::Full {
                stream_delay_ms: Some(INITIAL_STREAM_DELAY_MS),
            }),
            noise_suppression: Some(NoiseSuppression {
                level: NoiseSuppressionLevel::High,
                analyze_linear_aec_output: false,
            }),
            high_pass_filter: Some(HighPassFilter {
                apply_in_full_band: true,
            }),
            ..Default::default()
        });
        Ok(Self { processor })
    }

    pub fn process(&self, mic: &mut [f32], reference: &[f32]) -> Result<(), String> {
        debug_assert_eq!(mic.len(), FRAME_SAMPLES);
        debug_assert_eq!(reference.len(), FRAME_SAMPLES);

        let mut render = [reference.to_vec()];
        self.processor
            .process_render_frame(&mut render)
            .map_err(|e| format!("AECリファレンス処理に失敗: {e:?}"))?;

        let mut capture = [mic.to_vec()];
        self.processor
            .process_capture_frame(&mut capture)
            .map_err(|e| format!("AECマイク処理に失敗: {e:?}"))?;
        mic.copy_from_slice(&capture[0]);
        Ok(())
    }
}
