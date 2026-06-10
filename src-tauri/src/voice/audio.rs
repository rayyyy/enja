//! 録音サンプルの整形(VAD トリム・無音圧縮)と WAV エンコード。
//! 純ロジックのみで、プラットフォーム依存・I/O 依存を持たない。

use std::io::Cursor;

const MIN_API_RECORDING_SECS: f32 = 0.7;
const VOICE_FRAME_MS: u32 = 20;
const MIN_ACTIVE_AUDIO_SECS: f32 = 0.08;
const VAD_NOISE_RMS_FLOOR: f32 = 0.0003;
const VAD_NOISE_PEAK_FLOOR: f32 = 0.001;
const VAD_MIN_CONTINUATION_RMS_THRESHOLD: f32 = 0.0008;
const VAD_MIN_WEAK_RMS_THRESHOLD: f32 = 0.0012;
const VAD_MIN_STRONG_RMS_THRESHOLD: f32 = 0.0024;
const VAD_MIN_CONTINUATION_PEAK_THRESHOLD: f32 = 0.004;
const VAD_MIN_WEAK_PEAK_THRESHOLD: f32 = 0.006;
const VAD_MIN_STRONG_PEAK_THRESHOLD: f32 = 0.012;
const VAD_AMBIGUOUS_DYNAMIC_RANGE: f32 = 2.5;
const VAD_MIN_START_MS: u32 = 60;
const VAD_MIN_SEGMENT_MS: u32 = 80;
const VAD_PREFIX_PADDING_MS: u32 = 300;
const VAD_POST_PADDING_MS: u32 = 600;
const VAD_END_SILENCE_MS: u32 = 1_200;
const VAD_SHORT_GAP_MERGE_MS: u32 = 900;
const VAD_AMBIGUOUS_PREFIX_PADDING_MS: u32 = 420;
const VAD_AMBIGUOUS_POST_PADDING_MS: u32 = 900;
const VAD_AMBIGUOUS_END_SILENCE_MS: u32 = 1_800;
const VAD_AMBIGUOUS_SHORT_GAP_MERGE_MS: u32 = 1_400;
const VAD_TERMINAL_PROTECTION_MS: u32 = 2_600;

#[derive(Debug, Clone, Copy)]
pub(crate) struct PreparedAudioAnalysis {
    pub(crate) duration_secs: f32,
    pub(crate) active_audio_secs: f32,
}

#[derive(Debug)]
pub(crate) struct PreparedAudio {
    pub(crate) samples: Vec<i16>,
    pub(crate) analysis: PreparedAudioAnalysis,
}

#[derive(Debug, Clone, Copy)]
struct VoiceFrameStats {
    rms: f32,
    peak: f32,
}

#[derive(Debug, Clone, Copy)]
struct VoiceSegment {
    start_frame: usize,
    end_frame: usize,
}

#[derive(Debug, Clone, Copy)]
struct VoiceVadConfig {
    continuation_rms_threshold: f32,
    weak_rms_threshold: f32,
    strong_rms_threshold: f32,
    continuation_peak_threshold: f32,
    weak_peak_threshold: f32,
    strong_peak_threshold: f32,
    min_start_frames: usize,
    min_segment_frames: usize,
    prefix_padding_frames: usize,
    post_padding_frames: usize,
    end_silence_frames: usize,
    short_gap_merge_frames: usize,
    terminal_protection_frames: usize,
}

#[derive(Debug)]
struct VoiceVadResult {
    segments: Vec<VoiceSegment>,
    speech_frames: usize,
}

pub(crate) fn samples_to_wav(
    samples: &[i16],
    sample_rate: u32,
    channels: u16,
) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = Vec::new();
    {
        let cursor = Cursor::new(&mut wav);
        let mut writer = hound::WavWriter::new(cursor, spec).map_err(|e| e.to_string())?;
        for sample in samples {
            writer.write_sample(*sample).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(wav)
}

pub(crate) fn prepare_recorded_audio_for_api(
    samples: &[i16],
    sample_rate: u32,
    channels: u16,
) -> Result<PreparedAudio, String> {
    let prepared = trim_recorded_audio(samples, sample_rate, channels);
    let analysis = prepared.analysis;
    if analysis.active_audio_secs < MIN_ACTIVE_AUDIO_SECS {
        return Err(
            "音声が検出できなかったため、API送信をスキップしました。マイク入力を確認してください。"
                .to_string(),
        );
    }
    if analysis.duration_secs < MIN_API_RECORDING_SECS {
        return Err(
            "録音が短すぎるため、API送信をスキップしました。もう少し長く話してください。"
                .to_string(),
        );
    }
    Ok(prepared)
}

fn trim_recorded_audio(samples: &[i16], sample_rate: u32, channels: u16) -> PreparedAudio {
    let channels = channels.max(1) as usize;
    let frame_len = voice_frame_len(sample_rate, channels);
    let frame_stats = samples
        .chunks(frame_len)
        .map(voice_frame_stats)
        .collect::<Vec<_>>();

    let vad = detect_voice_segments(&frame_stats);
    if vad.segments.is_empty() {
        return PreparedAudio {
            samples: Vec::new(),
            analysis: PreparedAudioAnalysis {
                duration_secs: 0.0,
                active_audio_secs: 0.0,
            },
        };
    }

    let trimmed = render_voice_segments(samples, frame_len, &vad.segments);

    PreparedAudio {
        analysis: PreparedAudioAnalysis {
            duration_secs: audio_duration_secs(trimmed.len(), sample_rate, channels),
            active_audio_secs: vad.speech_frames as f32 * VOICE_FRAME_MS as f32 / 1000.0,
        },
        samples: trimmed,
    }
}

fn voice_frame_len(sample_rate: u32, channels: usize) -> usize {
    ((sample_rate as usize * channels * VOICE_FRAME_MS as usize) / 1000).max(1)
}

fn ms_to_frame_count(ms: u32) -> usize {
    ms.div_ceil(VOICE_FRAME_MS) as usize
}

fn detect_voice_segments(stats: &[VoiceFrameStats]) -> VoiceVadResult {
    if stats.is_empty() {
        return VoiceVadResult {
            segments: Vec::new(),
            speech_frames: 0,
        };
    }

    let config = estimate_voice_vad_config(stats);
    let mut segments = Vec::new();
    let mut speech_frames = 0usize;
    let mut weak_run = 0usize;
    let mut weak_run_start = 0usize;
    let mut segment_start = 0usize;
    let mut last_voice_frame = 0usize;
    let mut silence_run = 0usize;
    let mut in_segment = false;

    for (frame_index, stat) in stats.iter().enumerate() {
        let strong = is_strong_voice_frame(*stat, &config);
        let weak = strong || is_weak_voice_frame(*stat, &config);
        let continuation = weak || is_continuation_voice_frame(*stat, &config);

        if weak {
            speech_frames += 1;
        }

        if !in_segment {
            if weak {
                if weak_run == 0 {
                    weak_run_start = frame_index;
                }
                weak_run += 1;
            } else {
                weak_run = 0;
            }

            if strong || weak_run >= config.min_start_frames {
                let detected_start = if weak_run > 0 {
                    weak_run_start
                } else {
                    frame_index
                };
                segment_start = detected_start.saturating_sub(config.prefix_padding_frames);
                last_voice_frame = frame_index;
                silence_run = 0;
                in_segment = true;
            }

            continue;
        }

        if continuation {
            last_voice_frame = frame_index;
            silence_run = 0;
        } else {
            silence_run += 1;
            if silence_run >= config.end_silence_frames {
                let segment_end =
                    (last_voice_frame + 1 + config.post_padding_frames).min(stats.len());
                push_voice_segment(&mut segments, segment_start, segment_end, &config);
                in_segment = false;
                weak_run = 0;
                silence_run = 0;
            }
        }
    }

    if in_segment {
        let segment_end = (last_voice_frame + 1 + config.post_padding_frames).min(stats.len());
        push_voice_segment(&mut segments, segment_start, segment_end, &config);
    }

    if segments.is_empty() && has_possible_voice_signal(stats) {
        speech_frames = stats.len();
        segments.push(VoiceSegment {
            start_frame: 0,
            end_frame: stats.len(),
        });
    } else {
        protect_terminal_audio(&mut segments, stats.len(), &config);
    }

    VoiceVadResult {
        segments,
        speech_frames,
    }
}

fn estimate_voice_vad_config(stats: &[VoiceFrameStats]) -> VoiceVadConfig {
    let rms_values = stats.iter().map(|stat| stat.rms).collect::<Vec<_>>();
    let peak_values = stats.iter().map(|stat| stat.peak).collect::<Vec<_>>();
    let noise_rms = percentile(&rms_values, 0.20).max(VAD_NOISE_RMS_FLOOR);
    let noise_peak = percentile(&peak_values, 0.20).max(VAD_NOISE_PEAK_FLOOR);
    let p90_rms = percentile(&rms_values, 0.90);
    let p90_peak = percentile(&peak_values, 0.90);
    let rms_dynamic_range = p90_rms / noise_rms;
    let peak_dynamic_range = p90_peak / noise_peak;
    let ambiguous = rms_dynamic_range < VAD_AMBIGUOUS_DYNAMIC_RANGE
        && peak_dynamic_range < VAD_AMBIGUOUS_DYNAMIC_RANGE;

    let (
        continuation_rms_multiplier,
        weak_rms_multiplier,
        strong_rms_multiplier,
        continuation_peak_multiplier,
        weak_peak_multiplier,
        strong_peak_multiplier,
        prefix_padding_ms,
        post_padding_ms,
        end_silence_ms,
        short_gap_merge_ms,
    ) = if ambiguous {
        (
            1.00,
            1.03,
            1.25,
            1.00,
            1.06,
            1.30,
            VAD_AMBIGUOUS_PREFIX_PADDING_MS,
            VAD_AMBIGUOUS_POST_PADDING_MS,
            VAD_AMBIGUOUS_END_SILENCE_MS,
            VAD_AMBIGUOUS_SHORT_GAP_MERGE_MS,
        )
    } else {
        (
            1.08,
            1.30,
            2.30,
            1.12,
            1.45,
            2.40,
            VAD_PREFIX_PADDING_MS,
            VAD_POST_PADDING_MS,
            VAD_END_SILENCE_MS,
            VAD_SHORT_GAP_MERGE_MS,
        )
    };

    let possible_rms_signal = p90_rms >= VAD_MIN_CONTINUATION_RMS_THRESHOLD;
    let possible_peak_signal = p90_peak >= VAD_MIN_CONTINUATION_PEAK_THRESHOLD;

    let mut continuation_rms_threshold =
        (noise_rms * continuation_rms_multiplier).max(VAD_MIN_CONTINUATION_RMS_THRESHOLD);
    let mut weak_rms_threshold = (noise_rms * weak_rms_multiplier).max(VAD_MIN_WEAK_RMS_THRESHOLD);
    let mut strong_rms_threshold =
        (noise_rms * strong_rms_multiplier).max(VAD_MIN_STRONG_RMS_THRESHOLD);

    if possible_rms_signal {
        continuation_rms_threshold = continuation_rms_threshold
            .min((p90_rms * 0.75).max(VAD_MIN_CONTINUATION_RMS_THRESHOLD));
        weak_rms_threshold =
            weak_rms_threshold.min((p90_rms * 0.90).max(VAD_MIN_CONTINUATION_RMS_THRESHOLD));
        strong_rms_threshold =
            strong_rms_threshold.min((p90_rms * 0.98).max(VAD_MIN_CONTINUATION_RMS_THRESHOLD));
    }
    weak_rms_threshold = weak_rms_threshold.max(continuation_rms_threshold);
    strong_rms_threshold = strong_rms_threshold.max(weak_rms_threshold);

    let mut continuation_peak_threshold =
        (noise_peak * continuation_peak_multiplier).max(VAD_MIN_CONTINUATION_PEAK_THRESHOLD);
    let mut weak_peak_threshold =
        (noise_peak * weak_peak_multiplier).max(VAD_MIN_WEAK_PEAK_THRESHOLD);
    let mut strong_peak_threshold =
        (noise_peak * strong_peak_multiplier).max(VAD_MIN_STRONG_PEAK_THRESHOLD);

    if possible_peak_signal {
        continuation_peak_threshold = continuation_peak_threshold
            .min((p90_peak * 0.75).max(VAD_MIN_CONTINUATION_PEAK_THRESHOLD));
        weak_peak_threshold =
            weak_peak_threshold.min((p90_peak * 0.90).max(VAD_MIN_CONTINUATION_PEAK_THRESHOLD));
        strong_peak_threshold =
            strong_peak_threshold.min((p90_peak * 0.98).max(VAD_MIN_CONTINUATION_PEAK_THRESHOLD));
    }
    weak_peak_threshold = weak_peak_threshold.max(continuation_peak_threshold);
    strong_peak_threshold = strong_peak_threshold.max(weak_peak_threshold);

    VoiceVadConfig {
        continuation_rms_threshold,
        weak_rms_threshold,
        strong_rms_threshold,
        continuation_peak_threshold,
        weak_peak_threshold,
        strong_peak_threshold,
        min_start_frames: ms_to_frame_count(VAD_MIN_START_MS),
        min_segment_frames: ms_to_frame_count(VAD_MIN_SEGMENT_MS),
        prefix_padding_frames: ms_to_frame_count(prefix_padding_ms),
        post_padding_frames: ms_to_frame_count(post_padding_ms),
        end_silence_frames: ms_to_frame_count(end_silence_ms),
        short_gap_merge_frames: ms_to_frame_count(short_gap_merge_ms),
        terminal_protection_frames: ms_to_frame_count(VAD_TERMINAL_PROTECTION_MS),
    }
}

fn is_strong_voice_frame(stat: VoiceFrameStats, config: &VoiceVadConfig) -> bool {
    stat.rms >= config.strong_rms_threshold || stat.peak >= config.strong_peak_threshold
}

fn is_weak_voice_frame(stat: VoiceFrameStats, config: &VoiceVadConfig) -> bool {
    stat.rms >= config.weak_rms_threshold || stat.peak >= config.weak_peak_threshold
}

fn is_continuation_voice_frame(stat: VoiceFrameStats, config: &VoiceVadConfig) -> bool {
    stat.rms >= config.continuation_rms_threshold || stat.peak >= config.continuation_peak_threshold
}

fn push_voice_segment(
    segments: &mut Vec<VoiceSegment>,
    start_frame: usize,
    end_frame: usize,
    config: &VoiceVadConfig,
) {
    if end_frame <= start_frame || end_frame.saturating_sub(start_frame) < config.min_segment_frames
    {
        return;
    }

    if let Some(last) = segments.last_mut() {
        if start_frame <= last.end_frame.saturating_add(config.short_gap_merge_frames) {
            last.end_frame = last.end_frame.max(end_frame);
            return;
        }
    }

    segments.push(VoiceSegment {
        start_frame,
        end_frame,
    });
}

fn protect_terminal_audio(
    segments: &mut [VoiceSegment],
    total_frames: usize,
    config: &VoiceVadConfig,
) {
    let Some(last) = segments.last_mut() else {
        return;
    };

    if total_frames.saturating_sub(last.end_frame) <= config.terminal_protection_frames {
        last.end_frame = total_frames;
    }
}

fn has_possible_voice_signal(stats: &[VoiceFrameStats]) -> bool {
    let rms_values = stats.iter().map(|stat| stat.rms).collect::<Vec<_>>();
    let peak_values = stats.iter().map(|stat| stat.peak).collect::<Vec<_>>();
    percentile(&rms_values, 0.90) >= VAD_MIN_CONTINUATION_RMS_THRESHOLD
        || percentile(&peak_values, 0.90) >= VAD_MIN_CONTINUATION_PEAK_THRESHOLD
}

fn render_voice_segments(samples: &[i16], frame_len: usize, segments: &[VoiceSegment]) -> Vec<i16> {
    let retained_samples = segments
        .iter()
        .map(|segment| {
            let start = segment.start_frame.saturating_mul(frame_len);
            let end = segment
                .end_frame
                .saturating_mul(frame_len)
                .min(samples.len());
            end.saturating_sub(start)
        })
        .sum();
    let mut trimmed = Vec::with_capacity(retained_samples);

    for segment in segments {
        let start = segment.start_frame.saturating_mul(frame_len);
        let end = segment
            .end_frame
            .saturating_mul(frame_len)
            .min(samples.len());
        if start < end {
            trimmed.extend_from_slice(&samples[start..end]);
        }
    }

    trimmed
}

fn audio_duration_secs(sample_count: usize, sample_rate: u32, channels: usize) -> f32 {
    let samples_per_second = sample_rate as usize * channels.max(1);
    if samples_per_second == 0 {
        0.0
    } else {
        sample_count as f32 / samples_per_second as f32
    }
}

fn voice_frame_stats(samples: &[i16]) -> VoiceFrameStats {
    if samples.is_empty() {
        return VoiceFrameStats {
            rms: 0.0,
            peak: 0.0,
        };
    }

    let mut peak = 0.0f32;
    let sum = samples.iter().fold(0.0f32, |sum, sample| {
        let value = (*sample as f32 / i16::MAX as f32).clamp(-1.0, 1.0);
        peak = peak.max(value.abs());
        sum + value * value
    });

    VoiceFrameStats {
        rms: (sum / samples.len() as f32).sqrt(),
        peak,
    }
}

fn percentile(values: &[f32], fraction: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let rank = ((sorted.len() - 1) as f32 * fraction.clamp(0.0, 1.0)).round() as usize;
    sorted[rank]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_to_wav_writes_valid_header_and_samples() {
        let samples = [0_i16, i16::MAX, i16::MIN + 1, 42];
        let wav = samples_to_wav(&samples, 16_000, 1).expect("wav");
        let cursor = Cursor::new(wav);
        let reader = hound::WavReader::new(cursor).expect("reader");

        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.into_samples::<i16>().count(), samples.len());
    }

    #[test]
    fn prepare_recorded_audio_rejects_short_clip() {
        let samples = vec![2_000_i16; 8_000];

        let err = prepare_recorded_audio_for_api(&samples, 16_000, 1).expect_err("too short");

        assert!(err.contains("短すぎる"));
    }

    #[test]
    fn prepare_recorded_audio_rejects_silent_clip() {
        let samples = vec![0_i16; 16_000];

        let err = prepare_recorded_audio_for_api(&samples, 16_000, 1).expect_err("silent");

        assert!(err.contains("音声が検出"));
    }

    #[test]
    fn prepare_recorded_audio_accepts_audible_clip() {
        let samples = vec![2_000_i16; 16_000];

        let prepared = prepare_recorded_audio_for_api(&samples, 16_000, 1).expect("audible");

        assert!((prepared.analysis.duration_secs - 1.0).abs() < 0.001);
        assert!(prepared.analysis.active_audio_secs >= MIN_ACTIVE_AUDIO_SECS);
    }

    #[test]
    fn prepare_recorded_audio_trims_edge_silence() {
        let mut samples = Vec::new();
        samples.extend(vec![0_i16; 500]);
        samples.extend(vec![2_000_i16; 1_000]);
        samples.extend(vec![0_i16; 500]);

        let prepared = prepare_recorded_audio_for_api(&samples, 1_000, 1).expect("trimmed");

        assert!(prepared.samples.len() < samples.len());
        assert!((prepared.analysis.duration_secs - 1.8).abs() < 0.001);
    }

    #[test]
    fn prepare_recorded_audio_compresses_internal_silence() {
        let mut samples = Vec::new();
        samples.extend(vec![2_000_i16; 1_000]);
        samples.extend(vec![0_i16; 2_000]);
        samples.extend(vec![2_000_i16; 1_000]);

        let prepared = prepare_recorded_audio_for_api(&samples, 1_000, 1).expect("trimmed");

        assert!(prepared.samples.len() < samples.len());
        assert!((prepared.analysis.duration_secs - 2.9).abs() < 0.001);
    }

    #[test]
    fn prepare_recorded_audio_preserves_low_volume_tail_before_stop() {
        let mut samples = Vec::new();
        samples.extend(vec![2_000_i16; 1_000]);
        samples.extend(vec![80_i16; 800]);
        samples.extend(vec![0_i16; 500]);

        let prepared = prepare_recorded_audio_for_api(&samples, 1_000, 1).expect("tail preserved");

        assert_eq!(prepared.samples.len(), samples.len());
    }
}
