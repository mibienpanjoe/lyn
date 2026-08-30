use std::io::Cursor;

use crate::media::staging::StagingError;

pub(crate) const WAV_SAMPLE_RATE: u32 = 16_000;

pub(crate) fn encode_mono_pcm_wav(
    interleaved_samples: &[f32],
    input_sample_rate: u32,
    input_channels: u16,
) -> Result<(Vec<u8>, u64), StagingError> {
    if input_sample_rate == 0 || input_channels == 0 || interleaved_samples.is_empty() {
        return Err(StagingError::InvalidMedia);
    }
    let channels = usize::from(input_channels);
    if !interleaved_samples.len().is_multiple_of(channels) {
        return Err(StagingError::InvalidMedia);
    }

    let mono = interleaved_samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / input_channels as f32)
        .collect::<Vec<_>>();
    let output_frames = ((mono.len() as u64 * u64::from(WAV_SAMPLE_RATE))
        / u64::from(input_sample_rate))
    .max(1) as usize;
    let mut resampled = Vec::with_capacity(output_frames);
    for output_index in 0..output_frames {
        let source_position =
            output_index as f64 * input_sample_rate as f64 / WAV_SAMPLE_RATE as f64;
        let lower = source_position.floor() as usize;
        let upper = (lower + 1).min(mono.len() - 1);
        let fraction = (source_position - lower as f64) as f32;
        resampled.push(mono[lower] + (mono[upper] - mono[lower]) * fraction);
    }

    let duration_ms = resampled.len() as u64 * 1_000 / u64::from(WAV_SAMPLE_RATE);
    let mut cursor = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WAV_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer =
            hound::WavWriter::new(&mut cursor, spec).map_err(|_| StagingError::InvalidMedia)?;
        for sample in resampled {
            let quantized = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            writer
                .write_sample(quantized)
                .map_err(|_| StagingError::InvalidMedia)?;
        }
        writer.finalize().map_err(|_| StagingError::InvalidMedia)?;
    }
    Ok((cursor.into_inner(), duration_ms))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{WAV_SAMPLE_RATE, encode_mono_pcm_wav};

    #[test]
    fn normalizes_stereo_audio_to_valid_16khz_mono_16bit_pcm_wav() {
        let input = (0..4_800)
            .flat_map(|index| {
                let sample = ((index as f32 / 48_000.0) * 440.0 * std::f32::consts::TAU).sin();
                [sample, sample]
            })
            .collect::<Vec<_>>();

        let (wav, duration_ms) = encode_mono_pcm_wav(&input, 48_000, 2).unwrap();
        let reader = hound::WavReader::new(Cursor::new(wav)).unwrap();

        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, WAV_SAMPLE_RATE);
        assert_eq!(reader.spec().bits_per_sample, 16);
        assert_eq!(reader.spec().sample_format, hound::SampleFormat::Int);
        assert_eq!(reader.duration(), 1_600);
        assert_eq!(duration_ms, 100);
    }

    #[test]
    fn rejects_empty_or_incomplete_audio_frames() {
        assert!(encode_mono_pcm_wav(&[], 48_000, 2).is_err());
        assert!(encode_mono_pcm_wav(&[0.0, 0.1, 0.2], 48_000, 2).is_err());
    }
}
