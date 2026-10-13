pub mod import;
pub mod wav;

#[cfg(feature = "audio")]
pub mod capture;
#[cfg(feature = "audio")]
pub mod playback;

#[cfg(feature = "audio")]
pub use capture::Recorder;
#[cfg(feature = "audio")]
pub use playback::play;

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

    pub fn play(_path: &Path) -> Result<()> {
        bail!(
            "built without the `audio` feature: install libasound2-dev and rebuild with --features audio"
        )
    }
}

#[cfg(not(feature = "audio"))]
pub use noop::{play, Recorder};
