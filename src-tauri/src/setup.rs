//! Lean toolchain setup: install elan, create a Mathlib-enabled project, and
//! download the prebuilt cache. All long-running work streams progress events
//! to the frontend via the `setup-progress` Tauri event.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use log::info;

// ── Constants ─────────────────────────────────────────────────────────

const MATHLIB_REV: &str = "v4.29.0";
const LEAN_TOOLCHAIN: &str = "leanprover/lean4:v4.29.0";
const SENTINEL_VERSION: &str = "1";

// ── Progress event ────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct SetupProgress {
    pub phase: String,
    pub message: String,
    pub progress_pct: u8,
}

// ── Binary paths ──────────────────────────────────────────────────────

/// Absolute path to the `lean` binary installed by elan.
/// Respects `TURNSTILE_LSP_CMD` env override for development.
pub fn lean_bin() -> PathBuf {
    if let Ok(cmd) = std::env::var("TURNSTILE_LSP_CMD") {
        return PathBuf::from(cmd);
    }
    elan_bin_dir().join(if cfg!(windows) { "lean.exe" } else { "lean" })
}

/// Absolute path to the `lake` binary installed by elan.
pub fn lake_bin() -> PathBuf {
    elan_bin_dir().join(if cfg!(windows) { "lake.exe" } else { "lake" })
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn elan_bin_dir() -> PathBuf {
    home_dir().join(".elan").join("bin")
}

// ── Sentinel check ────────────────────────────────────────────────────

/// Returns true if the Lean project is fully set up and ready.
pub fn check_setup_complete(project_path: &Path) -> bool {
    let sentinel = project_path.join(".turnstile-ready");
    let Ok(contents) = std::fs::read_to_string(&sentinel) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return false;
    };
    parsed.get("mathlib_rev").and_then(|v| v.as_str()) == Some(MATHLIB_REV)
        && parsed.get("version").and_then(|v| v.as_str()) == Some(SENTINEL_VERSION)
}

enum ResumeFrom {
    InstallingElan,
    CreatingProject,
    FetchingMathlib,
    DownloadingCache,
}

fn resume_from(project_path: &Path) -> ResumeFrom {
    if !lean_bin().exists() {
        return ResumeFrom::InstallingElan;
    }
    if !project_path.join("lakefile.toml").exists() {
        return ResumeFrom::CreatingProject;
    }
    if !project_path
        .join(".lake")
        .join("packages")
        .join("mathlib")
        .exists()
    {
        return ResumeFrom::FetchingMathlib;
    }
    ResumeFrom::DownloadingCache
}

// ── Main setup entry point ────────────────────────────────────────────

/// Run the full setup sequence, emitting `setup-progress` events throughout.
/// Designed to be called from `tokio::spawn`.
pub async fn run_setup(app: AppHandle, project_path: PathBuf, setup_running: Arc<AtomicBool>) {
    info!(
        "run_setup: started, project_path={}",
        project_path.display()
    );
    let result = do_setup(&app, &project_path).await;
    setup_running.store(false, Ordering::SeqCst);
    if let Err(e) = result {
        info!("run_setup: error: {e}");
        emit_progress(&app, "error", &e, 0);
    } else {
        info!("run_setup: completed successfully");
    }
}

async fn do_setup(app: &AppHandle, project_path: &Path) -> Result<(), String> {
    emit_progress(app, "checking", "Checking Lean installation...", 0);

    match resume_from(project_path) {
        ResumeFrom::InstallingElan => {
            info!("do_setup: resuming from InstallingElan");
            install_elan(app).await?;
            create_project(app, project_path)?;
            fetch_mathlib(app, project_path).await?;
            download_cache(app, project_path).await?;
        }
        ResumeFrom::CreatingProject => {
            info!("do_setup: resuming from CreatingProject");
            create_project(app, project_path)?;
            fetch_mathlib(app, project_path).await?;
            download_cache(app, project_path).await?;
        }
        ResumeFrom::FetchingMathlib => {
            info!("do_setup: resuming from FetchingMathlib");
            fetch_mathlib(app, project_path).await?;
            download_cache(app, project_path).await?;
        }
        ResumeFrom::DownloadingCache => {
            info!("do_setup: resuming from DownloadingCache");
            download_cache(app, project_path).await?;
        }
    }

    write_sentinel(project_path)?;
    emit_progress(app, "ready", "Turnstile is ready.", 100);
    Ok(())
}

// ── Setup steps ───────────────────────────────────────────────────────

async fn install_elan(app: &AppHandle) -> Result<(), String> {
    emit_progress(
        app,
        "installing-elan",
        "Installing Lean toolchain manager (elan)...",
        5,
    );

    if cfg!(windows) {
        install_elan_windows(app).await
    } else {
        install_elan_unix(app).await
    }
}

async fn install_elan_unix(app: &AppHandle) -> Result<(), String> {
    let home = home_dir();
    let path_env =
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".to_string());

    let status = Command::new("sh")
        .args([
            "-c",
            "curl -sSf https://elan.lean-lang.org/elan-init.sh | sh -s -- --no-modify-path -y",
        ])
        .env("HOME", &home)
        .env("PATH", &path_env)
        .status()
        .await
        .map_err(|e| format!("Failed to run elan installer: {e}"))?;

    if !status.success() {
        return Err("elan installer exited with a non-zero status".to_string());
    }

    let lean = lean_bin();
    if !lean.exists() {
        return Err(format!(
            "elan installed but lean binary not found at {}",
            lean.display()
        ));
    }

    info!("elan installed successfully");
    emit_progress(app, "installing-elan", "elan installed.", 9);
    Ok(())
}

async fn install_elan_windows(app: &AppHandle) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("elan-init.exe");

    let response = Command::new("powershell")
        .args([
            "-Command",
            &format!(
                "Invoke-WebRequest -Uri https://elan.lean-lang.org/elan-init.exe -OutFile '{}'",
                tmp.display()
            ),
        ])
        .status()
        .await
        .map_err(|e| format!("Failed to download elan installer: {e}"))?;

    if !response.success() {
        return Err("Failed to download elan-init.exe".to_string());
    }

    let status = Command::new(&tmp)
        .args(["/S"])
        .status()
        .await
        .map_err(|e| format!("Failed to run elan installer: {e}"))?;

    let _ = std::fs::remove_file(&tmp);

    if !status.success() {
        return Err("elan installer exited with a non-zero status".to_string());
    }

    let lean = lean_bin();
    if !lean.exists() {
        return Err(format!(
            "elan installed but lean binary not found at {}",
            lean.display()
        ));
    }

    info!("elan installed successfully (Windows)");
    emit_progress(app, "installing-elan", "elan installed.", 9);
    Ok(())
}

fn create_project(app: &AppHandle, project_path: &Path) -> Result<(), String> {
    emit_progress(app, "creating-project", "Creating Lean project...", 10);

    std::fs::create_dir_all(project_path)
        .map_err(|e| format!("Failed to create project directory: {e}"))?;

    std::fs::write(project_path.join("lean-toolchain"), LEAN_TOOLCHAIN)
        .map_err(|e| format!("Failed to write lean-toolchain: {e}"))?;

    let lakefile = format!(
        r#"name = "turnstile-scratch"
version = "0.1.0"

[[require]]
name = "mathlib"
scope = "leanprover-community"
rev = "{MATHLIB_REV}"

[[lean_lib]]
name = "Proof"
"#
    );
    std::fs::write(project_path.join("lakefile.toml"), lakefile)
        .map_err(|e| format!("Failed to write lakefile.toml: {e}"))?;

    let proof = project_path.join("Proof.lean");
    if !proof.exists() {
        std::fs::write(
            &proof,
            "import Mathlib\n\n-- Write Lean 4 + Mathlib proofs here.\n",
        )
        .map_err(|e| format!("Failed to write Proof.lean: {e}"))?;
    }

    emit_progress(app, "creating-project", "Lean project created.", 14);
    Ok(())
}

async fn fetch_mathlib(app: &AppHandle, project_path: &Path) -> Result<(), String> {
    emit_progress(
        app,
        "fetching-mathlib",
        "Fetching Mathlib (this may take several minutes)...",
        15,
    );

    run_with_heartbeat(
        app,
        &lake_bin(),
        &["update"],
        project_path,
        "fetching-mathlib",
        15,
        49,
    )
    .await?;

    emit_progress(app, "fetching-mathlib", "Mathlib fetched.", 49);
    Ok(())
}

async fn download_cache(app: &AppHandle, project_path: &Path) -> Result<(), String> {
    emit_progress(
        app,
        "downloading-cache",
        "Downloading prebuilt Mathlib cache...",
        50,
    );

    run_with_heartbeat(
        app,
        &lake_bin(),
        &["exe", "cache", "get"],
        project_path,
        "downloading-cache",
        50,
        99,
    )
    .await?;

    emit_progress(app, "downloading-cache", "Mathlib cache downloaded.", 99);
    Ok(())
}

fn write_sentinel(project_path: &Path) -> Result<(), String> {
    let sentinel = json!({
        "version": SENTINEL_VERSION,
        "mathlib_rev": MATHLIB_REV,
        "lean_toolchain": LEAN_TOOLCHAIN,
    });
    std::fs::write(
        project_path.join(".turnstile-ready"),
        serde_json::to_string_pretty(&sentinel).unwrap(),
    )
    .map_err(|e| format!("Failed to write sentinel file: {e}"))
}

// ── Subprocess streaming ──────────────────────────────────────────────

async fn run_with_heartbeat(
    app: &AppHandle,
    cmd: &Path,
    args: &[&str],
    cwd: &Path,
    phase: &str,
    pct_start: u8,
    pct_end: u8,
) -> Result<(), String> {
    let app_hb = app.clone();
    let phase_hb = phase.to_string();
    let message_hb = format!(
        "Running {} {}…",
        cmd.file_name().unwrap_or_default().to_string_lossy(),
        args.join(" ")
    );
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();

    let heartbeat = tokio::spawn(async move {
        let mut tick: u8 = 0;
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                () = tokio::time::sleep(tokio::time::Duration::from_secs(3)) => {
                    let pct = pct_start.saturating_add(
                        tick.saturating_mul(2).min(pct_end.saturating_sub(pct_start).saturating_sub(1))
                    );
                    emit_progress(&app_hb, &phase_hb, &message_hb, pct);
                    tick = tick.saturating_add(1);
                }
            }
        }
    });

    let result = run_streaming(app, cmd, args, cwd, phase, pct_start, pct_end).await;
    let _ = stop_tx.send(());
    let _ = heartbeat.await;
    result
}

fn spawn_line_forwarder<R>(
    reader: R,
    app: AppHandle,
    phase: String,
    pct: u8,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                emit_progress(&app, &phase, &line, pct);
            }
        }
    })
}

async fn run_streaming(
    app: &AppHandle,
    cmd: &Path,
    args: &[&str],
    cwd: &Path,
    phase: &str,
    pct_start: u8,
    _pct_end: u8,
) -> Result<(), String> {
    let elan_bin = elan_bin_dir();
    let home = home_dir();
    let path_env = format!(
        "{}:{}",
        elan_bin.display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".to_string())
    );

    info!("run_streaming: {} {:?}", cmd.display(), args);

    let mut child = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .env("HOME", &home)
        .env("PATH", &path_env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {e}", cmd.display()))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let stdout_task = spawn_line_forwarder(stdout, app.clone(), phase.to_string(), pct_start);
    let stderr_task = spawn_line_forwarder(stderr, app.clone(), phase.to_string(), pct_start);

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait for {}: {e}", cmd.display()))?;

    let _ = tokio::join!(stdout_task, stderr_task);

    if !status.success() {
        return Err(format!(
            "{} exited with status {}",
            cmd.display(),
            status.code().unwrap_or(-1)
        ));
    }

    Ok(())
}

pub fn emit_progress(app: &AppHandle, phase: &str, message: &str, progress_pct: u8) {
    app.emit(
        "setup-progress",
        SetupProgress {
            phase: phase.to_string(),
            message: message.to_string(),
            progress_pct,
        },
    )
    .ok();
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_project() -> TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    #[test]
    fn setup_incomplete_when_no_sentinel() {
        let dir = temp_project();
        assert!(!check_setup_complete(dir.path()));
    }

    #[test]
    fn setup_incomplete_when_sentinel_is_not_json() {
        let dir = temp_project();
        fs::write(dir.path().join(".turnstile-ready"), "not json").unwrap();
        assert!(!check_setup_complete(dir.path()));
    }

    #[test]
    fn setup_incomplete_when_mathlib_rev_wrong() {
        let dir = temp_project();
        let sentinel = json!({
            "version": SENTINEL_VERSION,
            "mathlib_rev": "v0.0.0",
            "lean_toolchain": LEAN_TOOLCHAIN,
        });
        fs::write(
            dir.path().join(".turnstile-ready"),
            serde_json::to_string(&sentinel).unwrap(),
        )
        .unwrap();
        assert!(!check_setup_complete(dir.path()));
    }

    #[test]
    fn setup_complete_when_sentinel_valid() {
        let dir = temp_project();
        write_sentinel(dir.path()).unwrap();
        assert!(check_setup_complete(dir.path()));
    }

    #[test]
    fn write_sentinel_creates_file_with_expected_fields() {
        let dir = temp_project();
        write_sentinel(dir.path()).unwrap();
        let contents = fs::read_to_string(dir.path().join(".turnstile-ready")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["version"], SENTINEL_VERSION);
        assert_eq!(parsed["mathlib_rev"], MATHLIB_REV);
        assert_eq!(parsed["lean_toolchain"], LEAN_TOOLCHAIN);
    }

    #[test]
    fn lean_bin_respects_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("TURNSTILE_LSP_CMD", "/custom/lean");
        let path = lean_bin();
        std::env::remove_var("TURNSTILE_LSP_CMD");
        assert_eq!(path, PathBuf::from("/custom/lean"));
    }

    #[test]
    fn lean_bin_default_is_in_elan_dir() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("TURNSTILE_LSP_CMD");
        let path = lean_bin();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains(".elan"),
            "expected .elan in path, got {path_str}"
        );
        assert!(path_str.ends_with("lean") || path_str.ends_with("lean.exe"));
    }

    #[test]
    fn lake_bin_is_in_elan_dir() {
        let path = lake_bin();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains(".elan"),
            "expected .elan in path, got {path_str}"
        );
        assert!(path_str.ends_with("lake") || path_str.ends_with("lake.exe"));
    }

    #[test]
    fn setup_incomplete_when_version_wrong() {
        let dir = temp_project();
        let sentinel = json!({
            "version": "999",
            "mathlib_rev": MATHLIB_REV,
            "lean_toolchain": LEAN_TOOLCHAIN,
        });
        fs::write(
            dir.path().join(".turnstile-ready"),
            serde_json::to_string(&sentinel).unwrap(),
        )
        .unwrap();
        assert!(!check_setup_complete(dir.path()));
    }

    #[test]
    fn setup_incomplete_when_version_missing() {
        let dir = temp_project();
        let sentinel = json!({
            "mathlib_rev": MATHLIB_REV,
            "lean_toolchain": LEAN_TOOLCHAIN,
        });
        fs::write(
            dir.path().join(".turnstile-ready"),
            serde_json::to_string(&sentinel).unwrap(),
        )
        .unwrap();
        assert!(!check_setup_complete(dir.path()));
    }

    #[test]
    fn setup_complete_ignores_extra_fields() {
        let dir = temp_project();
        let mut sentinel = json!({
            "version": SENTINEL_VERSION,
            "mathlib_rev": MATHLIB_REV,
            "lean_toolchain": LEAN_TOOLCHAIN,
        });
        sentinel["extra_field"] = json!("ignored");
        fs::write(
            dir.path().join(".turnstile-ready"),
            serde_json::to_string(&sentinel).unwrap(),
        )
        .unwrap();
        assert!(check_setup_complete(dir.path()));
    }

    #[test]
    fn setup_incomplete_when_sentinel_empty() {
        let dir = temp_project();
        fs::write(dir.path().join(".turnstile-ready"), "").unwrap();
        assert!(!check_setup_complete(dir.path()));
    }
}
