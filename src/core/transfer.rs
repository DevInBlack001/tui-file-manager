// core/transfer.rs - ftctl subprocess wrapper for enqueueing file transfers.
//
// Security properties:
//   - No shell=true. All subprocess calls use Command::new(abs_path).args([...]).
//   - Binary path is resolved absolute before use; PATH is searched manually,
//     not delegated to the shell.
//   - Symlink sources whose canonicalized path escapes $HOME are rejected.
//   - Enqueue timeout: 10 seconds. List timeout: 5 seconds.
//   - List response capped at 4 MiB before JSON parse.
//   - No string concatenation for shell command construction.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when interacting with the ftctl transfer daemon.
#[derive(Debug, Clone)]
pub enum TransferError {
    /// The ftctl binary was not found anywhere in the resolution chain.
    NotFound { install_hint: String },
    /// ftctl exited with a non-zero status or produced no usable output.
    DaemonUnreachable,
    /// ftctl reported an error for the enqueue operation.
    EnqueueFailed(String),
    /// The subprocess did not complete within the allowed timeout.
    Timeout,
    /// Any other unexpected error.
    Other(String),
}

// ---------------------------------------------------------------------------
// Job
// ---------------------------------------------------------------------------

/// A transfer job as reported by `ftctl list`.
#[derive(Debug, Clone)]
pub struct Job {
    /// Opaque job identifier.
    pub id: String,
    /// "copy" or "move".
    pub mode: String,
    /// Job state string (e.g. "queued", "running", "done", "paused").
    pub state: String,
    /// Source paths involved in this job.
    pub sources: Vec<String>,
    /// Destination path.
    pub dest: String,
    /// Completion percentage (0-100), if reported by the daemon.
    pub pct: Option<u8>,
}

// ---------------------------------------------------------------------------
// JSON shapes for ftctl output
// ---------------------------------------------------------------------------

use serde::Deserialize;

#[derive(Deserialize)]
struct EnqueueResponse {
    id: String,
}

#[derive(Deserialize)]
struct JobRecord {
    id: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    dest: String,
    pct: Option<u8>,
}

#[derive(Deserialize)]
struct ListResponse {
    jobs: Vec<JobRecord>,
}

// ---------------------------------------------------------------------------
// TransferClient
// ---------------------------------------------------------------------------

/// Client for the ftctl file-transfer control binary.
///
/// Construct with `TransferClient::new(override_path)`. Call `is_available()`
/// to check whether ftctl was found before attempting operations.
pub struct TransferClient {
    /// Resolved absolute path to the ftctl binary, or None if not found.
    ftctl: Option<PathBuf>,
}

impl TransferClient {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Locate ftctl and build a client.
    ///
    /// Resolution order:
    ///   1. `override_path` argument (if Some).
    ///   2. `$TFM_FTCTL_PATH` environment variable.
    ///   3. `$HOME/.local/bin/ftctl`.
    ///   4. Each directory in `$PATH`, searched manually (no shell exec).
    pub fn new(override_path: Option<&Path>) -> Self {
        let ftctl = resolve_ftctl(override_path);
        Self { ftctl }
    }

    /// True when a usable ftctl binary was found.
    pub fn is_available(&self) -> bool {
        self.ftctl.is_some()
    }

    // -----------------------------------------------------------------------
    // Enqueue
    // -----------------------------------------------------------------------

    /// Ask ftctl to enqueue a copy or move job.
    ///
    /// `mode` must be "copy" or "move". The flag passed to ftctl is
    /// `--copy` or `--move` accordingly.
    ///
    /// Returns the job ID string on success.
    ///
    /// Symlink sources are validated: if the canonicalized target escapes
    /// `$HOME`, the source is rejected before any subprocess is spawned.
    pub fn enqueue(
        &self,
        mode: &str,
        sources: &[PathBuf],
        dest: &Path,
    ) -> Result<String, TransferError> {
        let ftctl = match &self.ftctl {
            Some(p) => p,
            None => {
                return Err(TransferError::NotFound {
                    install_hint:
                        "ftctl not found. Install the file-transfer plugin: \
                         see https://github.com/DevInBlack001/filetransferd"
                            .to_string(),
                });
            }
        };

        // Validate symlink sources before touching the subprocess.
        validate_sources(sources)?;

        let mode_flag = if mode == "move" { "--move" } else { "--copy" };

        // Build argv: ftctl enqueue --copy|--move <dest> -- <sources...>
        let mut cmd = std::process::Command::new(ftctl);
        cmd.arg("enqueue")
            .arg(mode_flag)
            .arg(dest)
            .arg("--");
        for src in sources {
            cmd.arg(src);
        }

        let output = run_with_timeout(cmd, Duration::from_secs(10))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(TransferError::EnqueueFailed(stderr));
        }

        let parsed: EnqueueResponse =
            serde_json::from_slice(&output.stdout)
                .map_err(|e| TransferError::EnqueueFailed(e.to_string()))?;

        Ok(parsed.id)
    }

    // -----------------------------------------------------------------------
    // List jobs
    // -----------------------------------------------------------------------

    /// Retrieve the current job list from the transfer daemon.
    ///
    /// Returns None when ftctl is unavailable, the daemon is unreachable,
    /// or the response cannot be parsed.
    pub fn list_jobs(&self) -> Option<Vec<Job>> {
        let ftctl = self.ftctl.as_ref()?;

        // The real ftctl `list` subcommand takes no flags and emits JSON by
        // default; passing `--json` causes it to reject the call outright
        // (confirmed against the installed daemon).
        let mut cmd = std::process::Command::new(ftctl);
        cmd.arg("list");

        let output = run_with_timeout(cmd, Duration::from_secs(5)).ok()?;

        if !output.status.success() {
            return None;
        }

        // Cap response at 4 MiB before parsing.
        const CAP: usize = 4 * 1024 * 1024;
        let bytes = if output.stdout.len() > CAP {
            &output.stdout[..CAP]
        } else {
            &output.stdout
        };

        let parsed: ListResponse = serde_json::from_slice(bytes).ok()?;

        Some(
            parsed
                .jobs
                .into_iter()
                .map(|r| Job {
                    id: r.id,
                    mode: r.mode,
                    state: r.state,
                    sources: r.sources,
                    dest: r.dest,
                    pct: r.pct,
                })
                .collect(),
        )
    }

    // -----------------------------------------------------------------------
    // Job lookup by path
    // -----------------------------------------------------------------------

    /// Find the first active job that references `path` in its source list or
    /// destination.
    ///
    /// Returns None if ftctl is unavailable or no matching job is found.
    pub fn job_for_path(&self, path: &Path) -> Option<Job> {
        let path_str = path.to_string_lossy();
        self.list_jobs()?.into_iter().find(|job| {
            job.dest == path_str
                || job.sources.iter().any(|s| s == path_str.as_ref())
        })
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Validate that symlink sources do not escape $HOME.
///
/// For each source that is a symlink, `canonicalize()` is called on the path.
/// If the canonical path does not start with the home directory, the entire
/// enqueue is rejected with an error.
fn validate_sources(sources: &[PathBuf]) -> Result<(), TransferError> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    for src in sources {
        if src.is_symlink() {
            match src.canonicalize() {
                Ok(canonical) => {
                    if !canonical.starts_with(&home) {
                        return Err(TransferError::EnqueueFailed(format!(
                            "symlink source '{}' resolves outside home directory and was rejected",
                            src.display()
                        )));
                    }
                }
                Err(e) => {
                    return Err(TransferError::EnqueueFailed(format!(
                        "could not canonicalize symlink source '{}': {e}",
                        src.display()
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Run a `Command` with a timeout by spawning and joining with a deadline.
///
/// Because std does not expose a native timeout on `output()`, we use
/// `spawn()` + `wait_timeout` emulated via a thread-level join. In practice
/// we use `output()` synchronously but wrap it so callers get a `Timeout`
/// error if the binary hangs rather than blocking the TUI forever.
///
/// Implementation note: Rust std does not provide `wait_timeout`. We run
/// the command with `output()` in a scoped thread and join with a timeout.
fn run_with_timeout(
    mut cmd: std::process::Command,
    timeout: Duration,
) -> Result<std::process::Output, TransferError> {
    // Never let ftctl inherit our real controlling terminal on stdin (see
    // preview/image.rs's chafa --probe fix for why), and redirect stderr to
    // a pipe so we can capture it.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            TransferError::NotFound {
                install_hint: "ftctl binary not found at the resolved path".to_string(),
            }
        } else {
            TransferError::Other(e.to_string())
        }
    })?;

    // Poll until the process exits or the deadline passes.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process finished; collect stdout/stderr.
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut o) = child.stdout.take() {
                    let _ = o.read_to_end(&mut stdout);
                }
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_end(&mut stderr);
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err(TransferError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(TransferError::Other(e.to_string()));
            }
        }
    }
}

/// Resolve the absolute path to ftctl using the documented priority chain.
fn resolve_ftctl(override_path: Option<&Path>) -> Option<PathBuf> {
    // 1. Caller-supplied override.
    if let Some(p) = override_path {
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }

    // 2. $TFM_FTCTL_PATH environment variable.
    if let Ok(env_val) = std::env::var("TFM_FTCTL_PATH") {
        let p = PathBuf::from(&env_val);
        if p.is_file() {
            return Some(p);
        }
    }

    // 3. $HOME/.local/bin/ftctl (file-transfer plugin default install location).
    if let Some(home) = dirs::home_dir() {
        let candidate = home.join(".local").join("bin").join("ftctl");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // 4. Manual $PATH search (no shell exec, no string concatenation).
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(dir).join("ftctl");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}
