//! JSON-lines client for the Python KittenTTS sidecar.
//!
//! Protocol (one JSON object per line, over the child's stdio):
//! - sidecar → host: `{"event":"ready"}` once the model is loaded, or
//!   `{"event":"load_error","error":"..."}` on startup failure.
//! - host → sidecar: `{"id":N,"cmd":"generate","text":"...","reference":"/p.wav","out":"/o.wav"}`
//!   (or `"voice":"Bruno"` instead of `"reference"`; optional
//!   `"preset":"stable"|"expressive"`, omitted = model default), and
//!   `{"id":N,"cmd":"status"}`.
//! - sidecar → host: `{"id":N,"ok":true,"elapsed_s":1.2,"sample_rate":24000,"samples":123}`
//!   or `{"id":N,"ok":false,"error":"..."}`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::json;

/// What audio source the sidecar should clone.
#[derive(Debug, Clone)]
pub enum GenerateTarget {
    Reference(PathBuf),
    /// Built-in speaker; used by tests and the smoke path.
    #[allow(dead_code)]
    Voice(String),
}

/// Metadata about a successful generation.
#[derive(Debug, Clone, Copy)]
pub struct GenerateInfo {
    pub elapsed_s: f64,
    pub sample_rate: Option<u32>,
    /// Reported for protocol completeness; the UI currently surfaces only
    /// elapsed time and rate.
    #[allow(dead_code)]
    pub samples: Option<u64>,
}

/// Model lifecycle state, updated by the reader thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyState {
    Loading,
    Ready,
    Failed(String),
    Exited,
}

/// One-off events the UI surfaces.
#[derive(Debug, Clone)]
pub enum SidecarEvent {
    LoadError(String),
    Exited,
}

type PendingResult = anyhow::Result<GenerateInfo>;

struct Inner {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    next_id: AtomicU64,
    ready: Mutex<ReadyState>,
    ready_changed: Condvar,
    events: Mutex<Vec<SidecarEvent>>,
    pending: Mutex<HashMap<u64, Sender<PendingResult>>>,
    device_label: Mutex<Option<String>>,
}

/// Handle to a running sidecar process. Cheap to clone via `Arc` at the call
/// site; each method takes `&self` and never blocks the UI thread except for
/// the documented waits.
pub struct SidecarHandle(Arc<Inner>);

impl SidecarHandle {
    /// Spawn a sidecar `program args...` with `cwd` as working directory.
    /// PYTHONUTF8=1 is forced for the child: kittenml reads its voice
    /// metadata JSON without an explicit encoding, and on Windows the cp1252
    /// default raises UnicodeDecodeError during model load (Linux is UTF-8
    /// by default, so this is a no-op there).
    pub fn spawn(program: &str, args: &[&str], cwd: &Path) -> Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env("PYTHONUTF8", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn {program}"))?;
        let stdin = child.stdin.take().context("child has no stdin")?;
        let stdout = child.stdout.take().context("child has no stdout")?;
        let inner = Arc::new(Inner {
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            next_id: AtomicU64::new(1),
            ready: Mutex::new(ReadyState::Loading),
            ready_changed: Condvar::new(),
            events: Mutex::new(Vec::new()),
            pending: Mutex::new(HashMap::new()),
            device_label: Mutex::new(None),
        });
        std::thread::Builder::new()
            .name("sidecar-reader".into())
            .spawn({
                let inner = Arc::clone(&inner);
                move || reader_loop(stdout, inner)
            })
            .context("failed to spawn sidecar reader thread")?;
        Ok(Self(inner))
    }

    /// Spawn the production sidecar (`uv run python server.py` in `sidecar_dir`).
    pub fn spawn_for_project(sidecar_dir: &Path) -> Result<Self> {
        Self::spawn(
            "uv",
            &["run", "--quiet", "python", "server.py"],
            sidecar_dir,
        )
    }

    /// Current model lifecycle state.
    pub fn ready_state(&self) -> ReadyState {
        self.0
            .ready
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Device reported by the sidecar's ready event, if any (S7 telemetry).
    pub fn device_label(&self) -> Option<String> {
        self.0
            .device_label
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Drain queued events (non-blocking).
    pub fn poll_events(&self) -> Vec<SidecarEvent> {
        std::mem::take(&mut *self.0.events.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Run one generate request, waiting up to `timeout` total (including a
    /// possibly-still-loading model). Blocks the calling thread only.
    /// `preset` selects the decoding style ("stable" or "expressive");
    /// `None` leaves the choice to the model's configured default.
    pub fn generate(
        &self,
        text: &str,
        target: GenerateTarget,
        out: &Path,
        timeout: Duration,
        transcribe_language: Option<&str>,
        preset: Option<&str>,
    ) -> Result<GenerateInfo> {
        self.ensure_ready(timeout)?;
        let id = self.0.next_id.fetch_add(1, Ordering::SeqCst);
        let request = build_generate_request(id, text, &target, out, transcribe_language, preset);
        let (tx, rx) = mpsc::channel();
        self.0
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        {
            let mut stdin = self.0.stdin.lock().unwrap_or_else(|e| e.into_inner());
            writeln!(stdin, "{request}").context("sidecar stdin closed")?;
            stdin.flush().context("failed to flush sidecar stdin")?;
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .context("timed out waiting for the sidecar response")?;
            match rx.recv_timeout(remaining) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) => {
                    self.0
                        .pending
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&id);
                    bail!("timed out waiting for the sidecar response (id {id})");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    bail!("sidecar exited before responding");
                }
            }
        }
    }

    /// Block until the model leaves the loading state or `timeout` elapses.
    fn ensure_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.0.ready.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match guard.clone() {
                ReadyState::Ready => return Ok(()),
                ReadyState::Failed(e) => bail!("model failed to load: {e}"),
                ReadyState::Exited => bail!("sidecar exited"),
                ReadyState::Loading => {
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        bail!("timed out waiting for the model to load");
                    };
                    let (next, wait_result) = self
                        .0
                        .ready_changed
                        .wait_timeout_while(guard, remaining, |state| {
                            matches!(*state, ReadyState::Loading)
                        })
                        .unwrap_or_else(|e| e.into_inner());
                    guard = next;
                    if wait_result.timed_out() && matches!(*guard, ReadyState::Loading) {
                        bail!("timed out waiting for the model to load");
                    }
                }
            }
        }
    }
}

impl Drop for SidecarHandle {
    fn drop(&mut self) {
        let _ = self.0.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
    }
}

fn set_ready(inner: &Inner, state: ReadyState) {
    let mut guard = inner.ready.lock().unwrap_or_else(|e| e.into_inner());
    if *guard == ReadyState::Exited {
        return;
    }
    *guard = state;
    inner.ready_changed.notify_all();
}

fn finish_exit(inner: &Arc<Inner>) {
    set_ready(inner, ReadyState::Exited);
    inner
        .events
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(SidecarEvent::Exited);
    fail_all_pending(inner, "sidecar exited before responding");
}

fn fail_all_pending(inner: &Arc<Inner>, message: &str) {
    let mut pending = inner.pending.lock().unwrap_or_else(|e| e.into_inner());
    for (_, sender) in pending.drain() {
        let _ = sender.send(Err(anyhow!(message.to_string())));
    }
}

/// Build the generate request JSON, resolving `reference` and `out` to
/// absolute paths: the sidecar runs with its own working directory, so
/// relative paths would resolve against the wrong location.
fn build_generate_request(
    id: u64,
    text: &str,
    target: &GenerateTarget,
    out: &Path,
    transcribe_language: Option<&str>,
    preset: Option<&str>,
) -> serde_json::Value {
    let out_abs = std::path::absolute(out).expect("current directory is valid");
    match target {
        GenerateTarget::Reference(p) => {
            let reference_abs = std::path::absolute(p).expect("current directory is valid");
            json!({
                "id": id,
                "cmd": "generate",
                "text": text,
                "reference": reference_abs.display().to_string(),
                "out": out_abs.display().to_string(),
                "transcribe_language": transcribe_language,
                "preset": preset,
            })
        }
        GenerateTarget::Voice(v) => json!({
            "id": id,
            "cmd": "generate",
            "text": text,
            "voice": v,
            "out": out_abs.display().to_string(),
            "transcribe_language": transcribe_language,
            "preset": preset,
        }),
    }
}

fn handle_message(inner: &Arc<Inner>, v: serde_json::Value) {
    if let Some(event) = v.get("event").and_then(|e| e.as_str()) {
        match event {
            "ready" => {
                set_ready(inner, ReadyState::Ready);
                if let Some(device) = v.get("device").and_then(|d| d.as_str()) {
                    *inner
                        .device_label
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(device.to_string());
                }
            }
            "load_error" => {
                let err = v
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown load error")
                    .to_string();
                set_ready(inner, ReadyState::Failed(err.clone()));
                inner
                    .events
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(SidecarEvent::LoadError(err));
            }
            _ => {}
        }
        return;
    }
    if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
        let ok = v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false);
        let sender = inner
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        if let Some(sender) = sender {
            let result: PendingResult = if ok {
                Ok(GenerateInfo {
                    elapsed_s: v.get("elapsed_s").and_then(|e| e.as_f64()).unwrap_or(0.0),
                    sample_rate: v
                        .get("sample_rate")
                        .and_then(|s| s.as_u64())
                        .map(|s| s as u32),
                    samples: v.get("samples").and_then(|s| s.as_u64()),
                })
            } else {
                Err(anyhow!(
                    v.get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("sidecar reported failure")
                        .to_string()
                ))
            };
            let _ = sender.send(result);
        }
    }
}

fn reader_loop(stdout: ChildStdout, inner: Arc<Inner>) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            handle_message(&inner, v);
        }
    }
    finish_exit(&inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn mock_handle(extra_args: &[&str]) -> SidecarHandle {
        let script = manifest().join("tests/mock_sidecar.py");
        let mut args: Vec<&str> = vec![script.to_str().unwrap()];
        args.extend_from_slice(extra_args);
        SidecarHandle::spawn("python3", &args, &manifest())
            .expect("failed to spawn mock sidecar")
    }

    fn wait_until_settled(h: &SidecarHandle, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !matches!(h.ready_state(), ReadyState::Loading) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("sidecar never left the loading state");
    }

    fn out_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vcd-sidecar-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("out.wav")
    }

    #[test]
    fn ready_then_generate_roundtrip() {
        let h = mock_handle(&[]);
        wait_until_settled(&h, Duration::from_secs(10));
        assert_eq!(h.ready_state(), ReadyState::Ready);

        let out = out_path("roundtrip");
        let info = h
            .generate(
                "hola",
                GenerateTarget::Voice("Bruno".into()),
                &out,
                Duration::from_secs(10),
                Some("auto"),
                Some("expressive"),
            )
            .unwrap();
        assert!((info.elapsed_s - 0.05).abs() < 0.5, "elapsed: {info:?}");
        assert_eq!(info.sample_rate, Some(24_000));

        // A second request gets a fresh id and works too.
        let info2 = h
            .generate(
                "mundo",
                GenerateTarget::Reference(PathBuf::from("ref.wav")),
                &out,
                Duration::from_secs(10),
                None,
                None,
            )
            .unwrap();
        assert_eq!(info2.samples, Some(1_200));
    }

    #[test]
    fn generate_failure_surfaces_error() {
        let h = mock_handle(&[]);
        wait_until_settled(&h, Duration::from_secs(10));
        let out = out_path("failure");
        let err = h
            .generate(
                "fail",
                GenerateTarget::Voice("Bruno".into()),
                &out,
                Duration::from_secs(10),
                None,
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains("boom"), "error was: {err:#}");
    }

    #[test]
    fn ready_event_reports_device() {
        let h = mock_handle(&[]);
        wait_until_settled(&h, Duration::from_secs(10));
        assert_eq!(h.ready_state(), ReadyState::Ready);
        assert_eq!(h.device_label().as_deref(), Some("cuda:0"));
    }

    #[test]
    fn unresponsive_sidecar_times_out() {
        let h = mock_handle(&["--stall"]);
        wait_until_settled(&h, Duration::from_secs(10));
        let out = out_path("timeout");
        let err = h
            .generate(
                "hola",
                GenerateTarget::Voice("Bruno".into()),
                &out,
                Duration::from_millis(400),
                None,
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains("timed out"), "error was: {err:#}");
    }

    #[test]
    fn request_paths_are_made_absolute() {
        let req = build_generate_request(
            7,
            "hola",
            &GenerateTarget::Reference(PathBuf::from("recordings/ref.wav")),
            Path::new("recordings/out.wav"),
            None,
            None,
        );
        let reference = req.get("reference").and_then(|v| v.as_str()).unwrap();
        let out = req.get("out").and_then(|v| v.as_str()).unwrap();
        assert!(Path::new(reference).is_absolute(), "reference: {reference}");
        assert!(Path::new(out).is_absolute(), "out: {out}");
        // Path::ends_with compares components, so it holds on Windows (\)
        // and Unix (/) alike — a bare str::ends_with would fail on Windows.
        assert!(Path::new(reference).ends_with("recordings/ref.wav"), "reference: {reference}");
        assert!(Path::new(out).ends_with("recordings/out.wav"), "out: {out}");

        let req = build_generate_request(
            8,
            "hola",
            &GenerateTarget::Voice("Bruno".into()),
            Path::new("recordings/out2.wav"),
            None,
            None,
        );
        assert!(req.get("reference").is_none(), "voice target must not send reference");
        let out = req.get("out").and_then(|v| v.as_str()).unwrap();
        assert!(Path::new(out).is_absolute(), "out: {out}");
    }

    #[test]
    fn request_includes_transcribe_language() {
        let req = build_generate_request(
            9,
            "hola",
            &GenerateTarget::Reference(PathBuf::from("r.wav")),
            Path::new("o.wav"),
            Some("es"),
            None,
        );
        assert_eq!(
            req.get("transcribe_language").and_then(|v| v.as_str()),
            Some("es")
        );

        let req = build_generate_request(
            10,
            "hola",
            &GenerateTarget::Voice("Bruno".into()),
            Path::new("o.wav"),
            Some("auto"),
            None,
        );
        assert_eq!(
            req.get("transcribe_language").and_then(|v| v.as_str()),
            Some("auto")
        );
    }

    #[test]
    fn request_includes_preset() {
        let req = build_generate_request(
            11,
            "hola",
            &GenerateTarget::Reference(PathBuf::from("r.wav")),
            Path::new("o.wav"),
            None,
            Some("expressive"),
        );
        assert_eq!(
            req.get("preset").and_then(|v| v.as_str()),
            Some("expressive")
        );

        // Omitting the preset must serialize to null (sidecar reads that as
        // "use the model default"), not "" or "stable".
        let req = build_generate_request(
            12,
            "hola",
            &GenerateTarget::Voice("Bruno".into()),
            Path::new("o.wav"),
            None,
            None,
        );
        assert!(req.get("preset").and_then(|v| v.as_str()).is_none());
    }
}
