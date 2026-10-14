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
    // rodio 0.23: DeviceSinkBuilder opens the output stream; `play` decodes
    // and appends a Player to its mixer.
    let mixer = rodio::DeviceSinkBuilder::open_default_sink()
        .context("no audio output device available")?;
    let player = rodio::play(mixer.mixer(), BufReader::new(file))
        .with_context(|| format!("cannot play {}", path.display()))?;
    player.sleep_until_end();
    Ok(())
}
