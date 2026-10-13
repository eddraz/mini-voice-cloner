use std::path::Path;

use anyhow::{Context, Result};

/// Mix `channels` interleaved channels down to mono by averaging each frame.
pub fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    assert!(channels >= 1, "channels must be >= 1");
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Write mono `f32` samples (expected in -1.0..=1.0) as 16-bit PCM WAV at
/// `sample_rate`. Values outside -1.0..=1.0 are clamped.
pub fn write_wav_i16_mono(path: &Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("cannot create {}", path.display()))?;
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        // Scale to full 16-bit range; float->int `as` saturates, so +1.0 maps
        // to i16::MAX and -1.0 to i16::MIN exactly.
        writer.write_sample((clamped * 32768.0) as i16)?;
    }
    writer.finalize().context("failed to finalize wav")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_stereo_averages_pairs() {
        let stereo = [1.0, 3.0, 2.0, 4.0];
        assert_eq!(downmix_to_mono(&stereo, 2), vec![2.0, 3.0]);
    }

    #[test]
    fn downmix_mono_is_identity() {
        let mono = [0.5, -0.25];
        assert_eq!(downmix_to_mono(&mono, 1), vec![0.5, -0.25]);
    }

    #[test]
    fn downmix_trailing_partial_frame_is_averaged() {
        let stereo = [1.0, 3.0, 2.0];
        assert_eq!(downmix_to_mono(&stereo, 2), vec![2.0, 2.0]);
    }

    #[test]
    fn written_wav_roundtrips_rate_channels_and_samples() {
        let dir = std::env::temp_dir().join(format!("vcd-wav-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("roundtrip.wav");
        let samples: Vec<f32> = vec![0.0, 0.5, -0.5, 1.0];
        write_wav_i16_mono(&path, &samples, 24_000).unwrap();

        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 24_000);
        assert_eq!(spec.channels, 1);
        let decoded: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        assert_eq!(decoded.len(), samples.len());
        for (a, b) in decoded.iter().zip(&samples) {
            assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writing_clamps_out_of_range_samples() {
        let dir = std::env::temp_dir().join(format!("vcd-wav-clamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clamp.wav");
        write_wav_i16_mono(&path, &[2.0, -2.0], 16_000).unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        let decoded: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(decoded, vec![i16::MAX, i16::MIN]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
