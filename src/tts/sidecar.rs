//! JSON-lines client for the Python KittenTTS sidecar.
//!
//! Protocol (one JSON object per line, over the child's stdio):
//! - sidecar → host: `{"event":"ready"}` once the model is loaded, or
//!   `{"event":"load_error","error":"..."}` on startup failure.
//! - host → sidecar: `{"id":N,"cmd":"generate","text":"...","reference":"/p.wav","out":"/o.wav"}`
//!   (or `"voice":"Bruno"` instead of `"reference"`), and
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
    Voice(String),
}

/// Metadata about a successful generation.
#[derive(Debug, Clone, Copy)]
pub struct GenerateInfo {
    pub elapsed_s: f64,
    pub sample_rate: Option<u32>,
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
}

/// Handle to a running sidecar process. Cheap to clone via `Arc` at the call
/// site; each method takes `&self` and never blocks the UI thread except for
/// the documented waits.
pub struct SidecarHandle(Arc<Inner>);

impl SidecarHandle {
    /// Spawn a sidecar `program args...` with `cwd` as working directory.
    pub fn spawn(program: &str, args: &[&str], cwd: &Path) -> Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
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

    /// Drain queued events (non-blocking).
    pub fn poll_events(&self) -> Vec<SidecarEvent> {
        std::mem::take(&mut *self.0.events.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Run one generate request, waiting up to `timeout` total (including a
    /// possibly-still-loading model). Blocks the calling thread only.
    pub fn generate(
        &self,
        text: &str,
        target: GenerateTarget,
        out: &Path,
        timeout: Duration,
    ) -> Result<GenerateInfo> {
        self.ensure_ready(timeout)?;
        let id = self.0.next_id.fetch_add(1, Ordering::SeqCst);
        let request = match &target {
            GenerateTarget::Reference(p) => json!({
                "id": id,
                "cmd": "generate",
                "text": text,
                "reference": p.display().to_string(),
                "out": out.display().to_string(),
            }),
            GenerateTarget::Voice(v) => json!({
                "id": id,
                "cmd": "generate",
                "text": text,
                "voice": v,
                "out": out.display().to_string(),
            }),
        };
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

fn handle_message(inner: &Arc<Inner>, v: serde_json::Value) {
    if let Some(event) = v.get("event").and_then(|e| e.as_str()) {
        match event {
            "ready" => set_ready(inner, ReadyState::Ready),
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
            )
            .unwrap_err();
        assert!(err.to_string().contains("boom"), "error was: {err:#}");
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
            )
            .unwrap_err();
        assert!(err.to_string().contains("timed out"), "error was: {err:#}");
    }
}
