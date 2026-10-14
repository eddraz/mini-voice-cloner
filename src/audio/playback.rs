//! Audio playback via rodio. Only compiled with the `audio` feature
//! (requires ALSA headers on Linux).

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// What a play-button click means given the current playback state (S9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayAction {
    Start(PathBuf),
    Pause,
    Resume,
}

/// Decide what a click on a source's play button should do: clicking the
/// playing clip pauses it, clicking it again resumes, any other click starts
/// that clip (stopping whatever was playing).
pub fn next_action(now_playing: Option<&Path>, paused: bool, clicked: &Path) -> PlayAction {
    match now_playing {
        Some(current) if current == clicked => {
            if paused {
                PlayAction::Resume
            } else {
                PlayAction::Pause
            }
        }
        _ => PlayAction::Start(clicked.to_path_buf()),
    }
}

/// Persistent single-channel playback: one output sink, at most one active
/// `Player`. Clicking ▶ starts the clip, again pauses, again resumes; a
/// different clip stops the current one — audio never overlaps (S9).
pub struct PlaybackController {
    sink: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    now_playing: Option<PathBuf>,
    paused: bool,
}

impl PlaybackController {
    pub fn new() -> Self {
        Self {
            sink: None,
            player: None,
            now_playing: None,
            paused: false,
        }
    }

    /// React to a play-button click: start, pause or resume.
    pub fn handle_click(&mut self, path: &Path) -> Result<()> {
        match next_action(self.now_playing.as_deref(), self.paused, path) {
            PlayAction::Pause => {
                if let Some(player) = &self.player {
                    player.pause();
                }
                self.paused = true;
            }
            PlayAction::Resume => {
                if let Some(player) = &self.player {
                    player.play();
                }
                self.paused = false;
            }
            PlayAction::Start(path) => {
                self.stop();
                if self.sink.is_none() {
                    let mut sink = rodio::DeviceSinkBuilder::open_default_sink()
                        .context("no audio output device available")?;
                    sink.log_on_drop(false);
                    self.sink = Some(sink);
                }
                let sink = self.sink.as_ref().expect("sink just created");
                let file =
                    File::open(&path).with_context(|| format!("cannot open {}", path.display()))?;
                let player = rodio::play(sink.mixer(), BufReader::new(file))
                    .with_context(|| format!("cannot play {}", path.display()))?;
                self.player = Some(player);
                self.now_playing = Some(path);
                self.paused = false;
            }
        }
        Ok(())
    }

    /// Stop playback and clear the current-clip state.
    pub fn stop(&mut self) {
        if let Some(player) = &self.player {
            player.stop();
        }
        self.player = None;
        self.now_playing = None;
        self.paused = false;
    }

    /// Clear state when the queue drained on its own. Returns true when it
    /// just finished, so the caller can refresh the UI.
    pub fn poll_finished(&mut self) -> bool {
        let drained = self.player.as_ref().map(|p| p.empty()).unwrap_or(false);
        if drained {
            self.stop();
        }
        drained
    }

    pub fn now_playing_path(&self) -> Option<&Path> {
        self.now_playing.as_deref()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> PathBuf {
        PathBuf::from("recordings/ref.wav")
    }

    fn result() -> PathBuf {
        PathBuf::from("recordings/out.wav")
    }

    #[test]
    fn click_when_nothing_plays_starts_the_clip() {
        assert_eq!(
            next_action(None, false, &reference()),
            PlayAction::Start(reference())
        );
    }

    #[test]
    fn click_on_playing_clip_pauses_it() {
        assert_eq!(
            next_action(Some(&reference()), false, &reference()),
            PlayAction::Pause
        );
    }

    #[test]
    fn click_on_paused_clip_resumes_it() {
        assert_eq!(
            next_action(Some(&reference()), true, &reference()),
            PlayAction::Resume
        );
    }

    #[test]
    fn click_on_another_clip_switches_playback() {
        assert_eq!(
            next_action(Some(&reference()), false, &result()),
            PlayAction::Start(result())
        );
    }
}
