use std::fs;
use std::process::{Command, Stdio};

use crate::error::Result;
use crate::process;
use crate::settings::Settings;
use crate::ssh;

const SSH: &str = "ssh";

/// Ensure the background adb tunnel matches `settings.adb_port`. A no-op if a
/// tracked tunnel is already alive and forwarding the right port - important
/// because `ensure_up` runs on every `bsdev`/`bsdev up`, including opening a
/// second terminal/tab while a first session is still using the tunnel;
/// unconditionally restarting would sever any adb command in flight through
/// it for no reason. Only a missing/dead PID or a changed port triggers a
/// restart.
///
/// The tunnel is spawned fully detached (see `process::spawn_detached`) so it
/// keeps forwarding after the terminal `bsdev` was launched from is closed -
/// VSCode's own terminals attach to the container directly (not through this
/// ssh session), so the tunnel can't depend on that session staying open.
pub fn start(settings: &Settings, verbose: bool) -> Result<()> {
    let Some(port) = settings.adb_port else {
        stop(settings, verbose);
        return Ok(());
    };

    if let Some((pid, Some(tracked_port))) = read_tracked(settings) {
        if tracked_port == port && is_alive(pid) {
            return Ok(());
        }
    }

    stop(settings, verbose);
    let args = ssh::adb_tunnel_args(settings, port);
    let pid = process::spawn_detached(SSH, args, verbose)?;
    fs::write(settings.adb_tunnel_pid_path(), format!("{pid}:{port}"))?;
    Ok(())
}

/// Best-effort stop of any tracked adb tunnel process. Never fails - a missing
/// PID file or an already-dead process are both fine outcomes, so callers
/// (including `start` itself) don't need to handle an error here.
pub fn stop(settings: &Settings, verbose: bool) {
    if let Some((pid, _)) = read_tracked(settings) {
        kill(pid, verbose);
    }
    let _ = fs::remove_file(settings.adb_tunnel_pid_path());
}

/// Parse a pid file's contents: `"<pid>:<port>"`, or a bare `"<pid>"` (the
/// format written before port-tracking was added) with an unknown port -
/// callers treat an unknown port as "always needs a restart" so an old-format
/// tracked tunnel gets migrated to the new format on the next `start`.
fn parse_tracked(contents: &str) -> Option<(u32, Option<u16>)> {
    let contents = contents.trim();
    match contents.split_once(':') {
        Some((pid, port)) => Some((pid.parse().ok()?, port.parse().ok())),
        None => Some((contents.parse().ok()?, None)),
    }
}

fn read_tracked(settings: &Settings) -> Option<(u32, Option<u16>)> {
    parse_tracked(&fs::read_to_string(settings.adb_tunnel_pid_path()).ok()?)
}

/// Best-effort kill, discarding the child's own stdout/stderr - unlike
/// `process::run`'s deliberately inherited stdio, `taskkill`/`kill` chatter
/// ("SUCCESS: ...", "No such process") isn't something the user needs to see
/// on every `bsdev up`/`bsdev` restarting the tunnel.
fn run_quiet(program: &str, args: &[&str], verbose: bool) {
    if verbose {
        eprintln!("+ {} {}", program, args.join(" "));
    }
    let _ = Command::new(program).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

#[cfg(windows)]
fn kill(pid: u32, verbose: bool) {
    run_quiet("taskkill", &["/PID", &pid.to_string(), "/F"], verbose);
}

#[cfg(not(windows))]
fn kill(pid: u32, verbose: bool) {
    run_quiet("kill", &["-TERM", &pid.to_string()], verbose);
}

/// Whether a process with `pid` is currently running.
#[cfg(windows)]
fn is_alive(pid: u32) -> bool {
    let filter = format!("PID eq {pid}");
    match process::capture("tasklist", &["/FI", &filter, "/NH"]) {
        // No match still exits 0 and just prints an "INFO: No tasks ..." line.
        Ok(Some(out)) => out.contains(&pid.to_string()),
        _ => false,
    }
}

#[cfg(not(windows))]
fn is_alive(pid: u32) -> bool {
    // `kill -0` sends no signal, just checks the process exists and is ours to signal.
    matches!(process::capture("kill", &["-0", &pid.to_string()]), Ok(Some(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pid_and_port() {
        assert_eq!(parse_tracked("1234:5037"), Some((1234, Some(5037))));
    }

    #[test]
    fn parses_legacy_pid_only_as_unknown_port() {
        assert_eq!(parse_tracked("1234"), Some((1234, None)));
    }

    #[test]
    fn rejects_an_unparseable_pid() {
        assert_eq!(parse_tracked("not-a-pid"), None);
        assert_eq!(parse_tracked(""), None);
    }

    #[test]
    fn tolerates_an_unparseable_port_as_unknown() {
        // A valid pid with garbage after the colon still forces a restart
        // (unknown port), rather than discarding the whole entry.
        assert_eq!(parse_tracked("1234:not-a-port"), Some((1234, None)));
    }
}
