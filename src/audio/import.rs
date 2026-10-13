//! Reference-audio import: WAV passes through, anything else is converted to
//! 24 kHz mono 16-bit PCM WAV with ffmpeg (KittenTTS accepts any rate; 24 kHz
//! matches its output and keeps files small).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Return `src` unchanged when it is already a WAV file; otherwise convert it
/// with ffmpeg into `out_dir` as `<stem>-24k.wav`.
pub fn to_wav_24k_mono(src: &Path, out_dir: &Path) -> Result<PathBuf> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "wav" {
        return Ok(src.to_path_buf());
    }
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .context("non-utf8 filename")?;
    std::fs::create_dir_all(out_dir)?;
    let out = out_dir.join(format!("{stem}-24k.wav"));
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(src)
        .args(["-ac", "1", "-ar", "24000", "-c:a", "pcm_s16le"])
        .arg(&out)
        .status()
        .context("failed to run ffmpeg; is it installed?")?;
    if !status.success() {
        bail!("ffmpeg failed for {}", src.display());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};

    fn write_sine_wav(path: &Path, sample_rate: u32, samples: usize) {
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::create(path, spec).unwrap();
        for i in 0..samples {
            let v = ((i as f32 / 100.0).sin() * 8000.0) as i16;
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vcd-import-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn wav_files_pass_through_unconverted() {
        let dir = temp_dir("passthrough");
        let wav = dir.join("ref.wav");
        write_sine_wav(&wav, 44_100, 100);
        let out = to_wav_24k_mono(&wav, &dir).unwrap();
        assert_eq!(out, wav);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn converts_compressed_audio_via_ffmpeg() {
        let probe = std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if probe.is_err() {
            eprintln!("ffmpeg not available; skipping conversion test");
            return;
        }
        let dir = temp_dir("convert");
        let wav = dir.join("src.wav");
        write_sine_wav(&wav, 44_100, 4_410);
        let mp3 = dir.join("src.mp3");
        let ok = std::process::Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-i"])
            .arg(&wav)
            .arg(&mp3)
            .status()
            .unwrap()
            .success();
        assert!(ok, "ffmpeg could not create the test mp3");

        let out = to_wav_24k_mono(&mp3, &dir).unwrap();
        assert_eq!(out.file_name().unwrap(), "src-24k.wav");
        let reader = hound::WavReader::open(&out).unwrap();
        assert_eq!(reader.spec().sample_rate, 24_000);
        assert_eq!(reader.spec().channels, 1);
        assert!(reader.duration() > 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
