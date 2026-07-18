use std::fs;

use crate::error::Result;
use crate::process;
use crate::settings::Settings;
use crate::ssh;

const SSH: &str = "ssh";

/// Ensure the background adb tunnel matches `settings.adb_port`: stop whatever
/// tunnel might already be running (a stale PID, or one forwarding a previous
/// port), then - if an adb port is configured - spawn a fresh detached
/// `ssh -N -R` tunnel and track its PID.
///
/// Deliberately a "kill old, spawn new" pattern rather than checking liveness:
/// it's called on every `bsdev up`/`bsdev`, so it stays correct regardless of
/// whether a previously tracked process is still alive, already dead, or was
/// never started. The tunnel is spawned fully detached (see
/// `process::spawn_detached`) so it keeps forwarding after the terminal
/// `bsdev` was launched from is closed - VSCode's own terminals attach to the
/// container directly (not through this ssh session), so the tunnel can't
/// depend on that session staying open.
pub fn start(settings: &Settings, verbose: bool) -> Result<()> {
    stop(settings, verbose);
    let Some(port) = settings.adb_port else { return Ok(()) };
    let args = ssh::adb_tunnel_args(settings, port);
    let pid = process::spawn_detached(SSH, args, verbose)?;
    fs::write(settings.adb_tunnel_pid_path(), pid.to_string())?;
    Ok(())
}

/// Best-effort stop of any tracked adb tunnel process. Never fails - a missing
/// PID file or an already-dead process are both fine outcomes, so callers
/// (including `start` itself) don't need to handle an error here.
pub fn stop(settings: &Settings, verbose: bool) {
    let path = settings.adb_tunnel_pid_path();
    if let Ok(contents) = fs::read_to_string(&path) {
        if let Ok(pid) = contents.trim().parse::<u32>() {
            kill(pid, verbose);
        }
    }
    let _ = fs::remove_file(&path);
}

#[cfg(windows)]
fn kill(pid: u32, verbose: bool) {
    let _ = process::run("taskkill", ["/PID".to_string(), pid.to_string(), "/F".to_string()], verbose);
}

#[cfg(not(windows))]
fn kill(pid: u32, verbose: bool) {
    let _ = process::run("kill", ["-TERM".to_string(), pid.to_string()], verbose);
}
