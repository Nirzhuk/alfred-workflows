//! Shared helpers for spawning local agent CLIs.

use directories::BaseDirs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use super::active::RunControl;

const OUTPUT_CHANNEL_CAPACITY: usize = 256;
const MAX_STDOUT_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const MAX_STDERR_CAPTURE_BYTES: usize = 1024 * 1024;
const CAPTURE_TRUNCATED_MARKER: &str = "[... earlier process output omitted ...]\n";

struct BoundedCapture {
    text: String,
    max_bytes: usize,
    truncated: bool,
}

impl BoundedCapture {
    fn new(max_bytes: usize) -> Self {
        Self {
            text: String::new(),
            max_bytes,
            truncated: false,
        }
    }

    fn push_line(&mut self, line: &str) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(line);

        if self.text.len() <= self.max_bytes {
            return;
        }

        self.truncated = true;
        // Drop a chunk instead of a few bytes on every line once full. Keeping
        // the tail preserves terminal JSON/result records from streaming CLIs.
        let target_bytes = self.max_bytes.saturating_mul(3) / 4;
        let mut start = self.text.len().saturating_sub(target_bytes);
        while start < self.text.len() && !self.text.is_char_boundary(start) {
            start += 1;
        }
        if let Some(newline) = self.text[start..].find('\n') {
            start += newline + 1;
        }
        self.text.drain(..start);
    }

    fn finish(mut self) -> String {
        if !self.truncated {
            return self.text;
        }

        let available = self
            .max_bytes
            .saturating_sub(CAPTURE_TRUNCATED_MARKER.len());
        if self.text.len() > available {
            let mut start = self.text.len() - available;
            while start < self.text.len() && !self.text.is_char_boundary(start) {
                start += 1;
            }
            self.text.drain(..start);
        }
        self.text.insert_str(0, CAPTURE_TRUNCATED_MARKER);
        self.text
    }
}

pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub duration_ms: u128,
}

/// Claude Code stores subscription OAuth credentials in one shared macOS
/// Keychain item. Its headless processes can race while rotating that item,
/// invalidating every active session. Keep all Claude processes launched by
/// Alfred mutually exclusive until Claude Code makes Keychain refreshes
/// atomic upstream.
fn claude_invocation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn lock_claude_invocation(
    control: Option<&RunControl>,
) -> Result<MutexGuard<'static, ()>, String> {
    loop {
        match claude_invocation_lock().try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
            Err(TryLockError::WouldBlock) => {
                if control.is_some_and(RunControl::is_cancelled) {
                    return Err("cancelled".into());
                }
                thread::sleep(Duration::from_millis(40));
            }
        }
    }
}

pub fn find_bin(name: &str) -> Option<PathBuf> {
    if let Ok(output) = Command::new("which").arg(name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                let p = PathBuf::from(path);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    let mut candidates = Vec::new();
    if let Some(base) = BaseDirs::new() {
        let home = base.home_dir();
        candidates.push(home.join(format!(".local/bin/{name}")));
        candidates.push(home.join(format!(".opencode/bin/{name}")));
        candidates.push(home.join(format!(".codex/bin/{name}")));
        candidates.push(home.join(format!(".cargo/bin/{name}")));
        candidates.push(home.join(format!("bin/{name}")));
        if name == "codex" {
            candidates.push(home.join("Applications/ChatGPT.app/Contents/Resources/codex"));
        }
    }
    if name == "codex" {
        candidates.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/codex",
        ));
    }
    candidates.push(PathBuf::from(format!("/opt/homebrew/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/local/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/bin/{name}")));

    candidates.into_iter().find(|p| p.is_file())
}

fn enrich_path(command: &mut Command) {
    if let Some(base) = BaseDirs::new() {
        let home = base.home_dir();
        let extras = [
            home.join(".local/bin"),
            home.join(".opencode/bin"),
            home.join(".cargo/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ];
        let mut path = std::env::var("PATH").unwrap_or_default();
        for extra in extras {
            let s = extra.to_string_lossy();
            if !path.split(':').any(|p| p == s) {
                path = format!("{s}:{path}");
            }
        }
        command.env("PATH", path);
    }
}

fn configure_agent_environment(command: &mut Command, bin: &Path) {
    if bin.file_name().and_then(|name| name.to_str()) == Some("claude") {
        // A workflow should never replace the CLI executable underneath
        // itself. Updates still happen from normal interactive Claude use or
        // an explicit `claude update`.
        command.env("DISABLE_AUTOUPDATER", "1");
    }
}

/// Spawn a CLI, stream stdout/stderr line-by-line, honor cancel/timeout.
pub fn run_cmd(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    control: Option<&RunControl>,
    on_line: Option<&dyn Fn(&str)>,
) -> Result<CmdOutput, String> {
    run_cmd_inner(bin, args, cwd, timeout, control, on_line, None)
}

/// Variant used by custom agents that accept their prompt over stdin.
pub fn run_cmd_with_stdin(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    control: Option<&RunControl>,
    on_line: Option<&dyn Fn(&str)>,
    stdin_payload: &str,
) -> Result<CmdOutput, String> {
    run_cmd_inner(
        bin,
        args,
        cwd,
        timeout,
        control,
        on_line,
        Some(stdin_payload),
    )
}

fn run_cmd_inner(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    control: Option<&RunControl>,
    on_line: Option<&dyn Fn(&str)>,
    stdin_payload: Option<&str>,
) -> Result<CmdOutput, String> {
    let mut command = Command::new(bin);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin_payload.is_some() {
        command.stdin(Stdio::piped());
    }

    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    enrich_path(&mut command);
    configure_agent_environment(&mut command, bin);

    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

    let mut stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("missing stdout for {}", bin.display()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("missing stderr for {}", bin.display()))?;

    // Own the child either in RunControl (cancelable) or a local slot.
    let local_slot: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
    if let Some(ctrl) = control {
        ctrl.set_child(child);
    } else if let Ok(mut slot) = local_slot.lock() {
        *slot = Some(child);
    }

    let cancel = control
        .map(|c| Arc::clone(&c.cancel))
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

    let stdout_buf = Arc::new(Mutex::new(BoundedCapture::new(MAX_STDOUT_CAPTURE_BYTES)));
    let stderr_buf = Arc::new(Mutex::new(BoundedCapture::new(MAX_STDERR_CAPTURE_BYTES)));
    let stdout_acc = Arc::clone(&stdout_buf);
    let stderr_acc = Arc::clone(&stderr_buf);

    // Backpressure prevents a noisy child from duplicating an arbitrary amount
    // of unread output in Alfred's heap.
    let (tx, rx) = std::sync::mpsc::sync_channel::<String>(OUTPUT_CHANNEL_CAPACITY);
    let tx_out = tx.clone();
    let tx_err = tx.clone();
    drop(tx);

    let t_out = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().flatten() {
            if let Ok(mut buf) = stdout_acc.lock() {
                buf.push_line(&line);
            }
            let _ = tx_out.send(line);
        }
    });

    let t_err = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().flatten() {
            if let Ok(mut buf) = stderr_acc.lock() {
                buf.push_line(&line);
            }
            let _ = tx_err.send(line);
        }
    });

    let start = Instant::now();
    // Start draining stdout/stderr before writing a potentially large prompt.
    // Otherwise a child that emits output while reading stdin can fill its
    // stdout pipe while this thread is blocked filling the stdin pipe.
    let stdin_error = stdin_payload.and_then(|payload| match stdin.as_mut() {
        Some(handle) => handle
            .write_all(payload.as_bytes())
            .err()
            .map(|error| format!("failed to write prompt to stdin: {error}")),
        None => Some(format!("missing stdin for {}", bin.display())),
    });
    drop(stdin);
    if stdin_error.is_some() {
        kill_child(control, &local_slot);
    }

    let mut cancelled = false;
    let mut timed_out = false;

    loop {
        for _ in 0..OUTPUT_CHANNEL_CAPACITY {
            let Ok(line) = rx.try_recv() else {
                break;
            };
            if let Some(cb) = on_line {
                cb(&line);
            }
        }

        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            kill_child(control, &local_slot);
            break;
        }

        if start.elapsed() > timeout {
            timed_out = true;
            kill_child(control, &local_slot);
            break;
        }

        let child_done = child_exited(control, &local_slot);
        if child_done && t_out.is_finished() && t_err.is_finished() {
            break;
        }

        thread::sleep(Duration::from_millis(40));
    }

    // A bounded sender may be waiting for capacity. Continue consuming while
    // the pipe readers finish so joining them cannot deadlock.
    while !t_out.is_finished() || !t_err.is_finished() {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(40)) {
            if let Some(cb) = on_line {
                cb(&line);
            }
        }
    }
    while let Ok(line) = rx.try_recv() {
        if let Some(cb) = on_line {
            cb(&line);
        }
    }

    let _ = t_out.join();
    let _ = t_err.join();

    let status_ok = wait_child(control, &local_slot);

    if let Some(error) = stdin_error {
        return Err(error);
    }
    if cancelled {
        return Err("cancelled".into());
    }
    if timed_out {
        return Err(format!(
            "{} timed out after {}s",
            bin.display(),
            timeout.as_secs()
        ));
    }

    let stdout = take_capture(stdout_buf);
    let stderr = take_capture(stderr_buf);

    Ok(CmdOutput {
        stdout,
        stderr,
        success: status_ok,
        duration_ms: start.elapsed().as_millis(),
    })
}

fn take_capture(capture: Arc<Mutex<BoundedCapture>>) -> String {
    match Arc::try_unwrap(capture) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(capture) => capture.finish(),
            Err(poisoned) => poisoned.into_inner().finish(),
        },
        Err(capture) => capture
            .lock()
            .map(|value| value.text.clone())
            .unwrap_or_default(),
    }
}

fn kill_child(control: Option<&RunControl>, local_slot: &Arc<Mutex<Option<std::process::Child>>>) {
    if let Some(ctrl) = control {
        ctrl.request_cancel();
        return;
    }
    if let Ok(mut slot) = local_slot.lock() {
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn child_exited(
    control: Option<&RunControl>,
    local_slot: &Arc<Mutex<Option<std::process::Child>>>,
) -> bool {
    if let Some(ctrl) = control {
        if let Ok(mut slot) = ctrl.child.lock() {
            return match slot.as_mut() {
                Some(child) => matches!(child.try_wait(), Ok(Some(_)) | Err(_)),
                None => true,
            };
        }
        return false;
    }
    if let Ok(mut slot) = local_slot.lock() {
        return match slot.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_)) | Err(_)),
            None => true,
        };
    }
    false
}

fn wait_child(
    control: Option<&RunControl>,
    local_slot: &Arc<Mutex<Option<std::process::Child>>>,
) -> bool {
    if let Some(ctrl) = control {
        return match ctrl.take_child() {
            Some(mut child) => child.wait().map(|s| s.success()).unwrap_or(false),
            None => false,
        };
    }
    if let Ok(mut slot) = local_slot.lock() {
        return match slot.take() {
            Some(mut child) => child.wait().map(|s| s.success()).unwrap_or(false),
            None => false,
        };
    }
    false
}

pub fn cwd_from_request(working_directory: &Option<String>) -> Option<PathBuf> {
    working_directory
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

pub fn prefer_stdout(output: &CmdOutput) -> String {
    let stdout = output.stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }
    output.stderr.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn serializes_claude_invocations() {
        let first = lock_claude_invocation(None).expect("first lock");
        let (sender, receiver) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let _second = lock_claude_invocation(None).expect("second lock");
            sender.send(()).expect("notify lock acquired");
        });

        assert!(receiver.recv_timeout(Duration::from_millis(80)).is_err());
        drop(first);
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("waiter should acquire released lock");
        waiter.join().expect("waiter thread");
    }

    #[test]
    fn bounded_capture_keeps_recent_output_within_its_budget() {
        let mut capture = BoundedCapture::new(96);
        for index in 0..20 {
            capture.push_line(&format!("event-{index:02}-abcdefghij"));
        }

        let output = capture.finish();
        assert!(output.len() <= 96);
        assert!(output.starts_with(CAPTURE_TRUNCATED_MARKER));
        assert!(output.contains("event-19"));
        assert!(!output.contains("event-00"));
    }

    #[cfg(unix)]
    #[test]
    fn shared_process_path_writes_stdin_and_captures_output() {
        let output = run_cmd_with_stdin(
            Path::new("/bin/sh"),
            &["-c".into(), "cat".into()],
            None,
            Duration::from_secs(2),
            None,
            None,
            "hello from stdin\n",
        )
        .expect("stdin command should run");

        assert!(output.success);
        assert_eq!(output.stdout.trim(), "hello from stdin");
    }
}
