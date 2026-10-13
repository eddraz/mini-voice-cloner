mod audio;
mod tts;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use slint::ComponentHandle;

slint::include_modules!();

struct Shared {
    recorder: audio::Recorder,
    started_at: Option<Instant>,
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn main() -> anyhow::Result<()> {
    let recordings_dir = PathBuf::from("recordings");
    std::fs::create_dir_all(&recordings_dir)?;

    let app = AppWindow::new()?;
    let weak = app.as_weak();
    app.set_model_status("loading model…".into());

    let sidecar = Arc::new(tts::SidecarHandle::spawn_for_project(Path::new("sidecar"))?);
    let shared = Arc::new(Mutex::new(Shared {
        recorder: audio::Recorder::new()?,
        started_at: None,
    }));

    // Ticker: recording elapsed time, sidecar events, model status.
    let timer = slint::Timer::default();
    {
        let shared = Arc::clone(&shared);
        let sidecar = Arc::clone(&sidecar);
        let weak = weak.clone();
        timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
            let Some(app) = weak.upgrade() else { return };
            {
                let sh = shared.lock().unwrap();
                if let Some(t0) = sh.started_at {
                    app.set_elapsed(format!("{:.1}s", t0.elapsed().as_secs_f32()).into());
                }
            }
            for ev in sidecar.poll_events() {
                match ev {
                    tts::SidecarEvent::LoadError(e) => {
                        app.set_model_status(format!("model error: {e}").into())
                    }
                    tts::SidecarEvent::Exited => {
                        app.set_model_status("sidecar exited".into())
                    }
                }
            }
            app.set_model_status(
                match sidecar.ready_state() {
                    tts::ReadyState::Loading => "loading model…".to_string(),
                    tts::ReadyState::Ready => "model ready".to_string(),
                    tts::ReadyState::Failed(e) => format!("model error: {e}"),
                    tts::ReadyState::Exited => "sidecar exited".to_string(),
                }
                .into(),
            );
        });
    }
    std::mem::forget(timer);

    app.on_start_recording({
        let shared = Arc::clone(&shared);
        let weak = weak.clone();
        move || {
            let mut sh = shared.lock().unwrap();
            match sh.recorder.start() {
                Ok(()) => {
                    sh.started_at = Some(Instant::now());
                    if let Some(app) = weak.upgrade() {
                        app.set_recording(true);
                        app.set_status("".into());
                    }
                }
                Err(e) => {
                    if let Some(app) = weak.upgrade() {
                        app.set_status(format!("record error: {e}").into());
                    }
                }
            }
        }
    });

    app.on_stop_recording({
        let shared = Arc::clone(&shared);
        let weak = weak.clone();
        let dir = recordings_dir.clone();
        move || {
            let out = dir.join(format!("ref-{}.wav", unix_secs()));
            let result = {
                let mut sh = shared.lock().unwrap();
                sh.started_at = None;
                sh.recorder.stop(&out)
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_recording(false);
                app.set_elapsed("".into());
                match result {
                    Ok(rate) => {
                        app.set_reference_path(out.display().to_string().into());
                        app.set_status(format!("reference saved ({} Hz)", rate).into());
                    }
                    Err(e) => app.set_status(format!("record error: {e}").into()),
                }
            });
        }
    });

    app.on_import_audio({
        let weak = weak.clone();
        move || {
            let weak = weak.clone();
            std::thread::spawn(move || {
                let picked = rfd::FileDialog::new()
                    .add_filter(
                        "Audio",
                        &["wav", "mp3", "ogg", "flac", "m4a", "opus", "webm"],
                    )
                    .pick_file();
                let Some(picked) = picked else { return };
                let result = audio::import::to_wav_24k_mono(&picked, Path::new("recordings"));
                let _ = weak.upgrade_in_event_loop(move |app| match result {
                    Ok(p) => {
                        app.set_reference_path(p.display().to_string().into());
                        app.set_status(format!("reference imported: {}", p.display()).into());
                    }
                    Err(e) => app.set_status(format!("import error: {e}").into()),
                });
            });
        }
    });

    let spawn_playback = |path: String| {
        std::thread::spawn(move || {
            if let Err(e) = audio::play(Path::new(&path)) {
                eprintln!("playback error: {e}");
            }
        })
    };

    app.on_play_reference({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                let path = app.get_reference_path().to_string();
                if !path.is_empty() {
                    spawn_playback(path);
                }
            }
        }
    });

    app.on_play_result({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                let path = app.get_result_path().to_string();
                if !path.is_empty() {
                    spawn_playback(path);
                }
            }
        }
    });

    app.on_generate({
        let sidecar = Arc::clone(&sidecar);
        let weak = weak.clone();
        let dir = recordings_dir.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            if app.get_busy() {
                return;
            }
            let text = app.get_text_to_speak().to_string();
            let reference = app.get_reference_path().to_string();
            app.set_busy(true);
            app.set_status("waiting for model / generating…".into());

            let sidecar = Arc::clone(&sidecar);
            let weak = weak.clone();
            let dir = dir.clone();
            std::thread::spawn(move || {
                let out = dir.join(format!("out-{}.wav", unix_secs()));
                let result = sidecar.generate(
                    &text,
                    tts::GenerateTarget::Reference(PathBuf::from(reference)),
                    &out,
                    Duration::from_secs(600),
                );
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.set_busy(false);
                    match result {
                        Ok(info) => {
                            app.set_result_path(out.display().to_string().into());
                            let rate = info
                                .sample_rate
                                .map(|r| r.to_string())
                                .unwrap_or_else(|| "?".into());
                            app.set_status(format!("generated in {:.1}s at {rate} Hz", info.elapsed_s).into());
                        }
                        Err(e) => app.set_status(format!("generate error: {e}").into()),
                    }
                });
            });
        }
    });

    app.run()?;
    Ok(())
}
