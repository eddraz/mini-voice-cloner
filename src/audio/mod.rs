pub mod import;
pub mod wav;

#[cfg(feature = "audio")]
pub mod capture;
#[cfg(feature = "audio")]
pub mod playback;

#[cfg(feature = "audio")]
pub use capture::Recorder;
#[cfg(feature = "audio")]
pub use playback::PlaybackController;

#[cfg(not(feature = "audio"))]
pub mod noop {
    //! Placeholders used when the crate is built without the `audio` feature
    //! (e.g. before ALSA headers are available).

    use std::path::Path;

    use anyhow::{bail, Result};

    pub struct Recorder;

    impl Recorder {
        pub fn new() -> Result<Self> {
            Ok(Self)
        }
        pub fn start(&mut self) -> Result<()> {
            bail!(
                "built without the `audio` feature: install libasound2-dev and rebuild with --features audio"
            )
        }
        pub fn stop(&mut self, _out: &Path) -> Result<u32> {
            bail!(
                "built without the `audio` feature: install libasound2-dev and rebuild with --features audio"
            )
        }
    }

    pub struct PlaybackController;

    impl PlaybackController {
        pub fn new() -> Self {
            Self
        }
        pub fn handle_click(&mut self, _path: &Path) -> Result<()> {
            bail!(
                "built without the `audio` feature: install libasound2-dev and rebuild with --features audio"
            )
        }
        pub fn stop(&mut self) {}
        pub fn poll_finished(&mut self) -> bool {
            false
        }
        pub fn now_playing_path(&self) -> Option<&Path> {
            None
        }
        pub fn is_paused(&self) -> bool {
            false
        }
    }

    impl Default for PlaybackController {
        fn default() -> Self {
            Self
        }
    }
}

#[cfg(not(feature = "audio"))]
pub use noop::{PlaybackController, Recorder};
