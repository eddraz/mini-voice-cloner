//! Audio playback via rodio. Only compiled with the `audio` feature
//! (requires ALSA headers on Linux).

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result};

/// Play a WAV/audio file to the default output device. Blocks until playback
/// ends, so call it from a worker thread.
pub fn play(path: &Path) -> Result<()> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let source = rodio::Decoder::new(BufReader::new(file))
        .with_context(|| format!("cannot decode {}", path.display()))?;
    let (_stream, handle) =
        rodio::OutputStream::try_default().context("no audio output device available")?;
    let sink = rodio::Sink::try_new(&handle)?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}
