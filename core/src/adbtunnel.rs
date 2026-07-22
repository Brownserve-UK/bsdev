use std::fs;

use crate::error::Result;
use crate::process;
use crate::settings::Settings;
use crate::ssh;
use crate::tunnel::{is_alive, kill};

const SSH: &str = "ssh";

/// Ensure the background adb tunnel matches `settings.adb_port`. A no-op if a
/// tracked tunnel is already alive, forwarding the right port, and dialled
/// into the container's current session - important because `ensure_up` runs
/// on every `bsdev`/`bsdev up`, including opening a second terminal/tab while
/// a first session is still using the tunnel; unconditionally restarting
/// would sever any adb command in flight through it for no reason.
///
/// `container_started_at` (the container's `State.StartedAt`, see
/// `docker::started_at`) is what catches a restarted/recreated container: the
/// host-side ssh process can easily still be alive (its TCP connection to the
/// old sshd can take a while to notice it's gone) even though sshd - and so
/// any tunnel dialled into it - died with the old session. Without this check
/// we'd wrongly treat that stale process as a working tunnel, and the first
/// `adb` command run inside the new container would spin up its own local
/// server on the port instead, which then wins the bind race against any
/// later tunnel attempt too.
///
/// The tunnel is spawned fully detached (see `process::spawn_detached`) so it
/// keeps forwarding after the terminal `bsdev` was launched from is closed -
/// VSCode's own terminals attach to the container directly (not through this
/// ssh session), so the tunnel can't depend on that session staying open.
pub fn start(settings: &Settings, container_started_at: &str, verbose: bool) -> Result<()> {
    let Some(port) = settings.adb_port else {
        stop(settings, verbose);
        return Ok(());
    };

    if let Some((pid, Some(tracked_port), Some(tracked_started_at))) = read_tracked(settings) {
        if tracked_port == port && tracked_started_at == container_started_at && is_alive(pid) {
            return Ok(());
        }
    }

    stop(settings, verbose);
    let args = ssh::adb_tunnel_args(settings, port);
    let pid = process::spawn_detached(SSH, args, verbose)?;
    fs::write(
        settings.adb_tunnel_pid_path(),
        format!("{pid}:{port}:{container_started_at}"),
    )?;
    Ok(())
}

/// Best-effort stop of any tracked adb tunnel process. Never fails - a missing
/// PID file or an already-dead process are both fine outcomes, so callers
/// (including `start` itself) don't need to handle an error here.
pub fn stop(settings: &Settings, verbose: bool) {
    if let Some((pid, _, _)) = read_tracked(settings) {
        kill(pid, verbose);
    }
    let _ = fs::remove_file(settings.adb_tunnel_pid_path());
}

/// Parse a pid file's contents: `"<pid>:<port>:<container_started_at>"`, or an
/// older `"<pid>:<port>"`/bare `"<pid>"` (formats written before this field or
/// port-tracking were added) with an unknown port/started_at - callers treat
/// either as unknown as "always needs a restart" so an old-format tracked
/// tunnel gets migrated to the new format on the next `start`. `splitn(3, ..)`
/// because `container_started_at` is an RFC3339 timestamp and so contains
/// colons of its own.
fn parse_tracked(contents: &str) -> Option<(u32, Option<u16>, Option<String>)> {
    let contents = contents.trim();
    let mut parts = contents.splitn(3, ':');
    let pid = parts.next()?.parse().ok()?;
    let port = parts.next().and_then(|p| p.parse().ok());
    let started_at = parts.next().map(str::to_string);
    Some((pid, port, started_at))
}

fn read_tracked(settings: &Settings) -> Option<(u32, Option<u16>, Option<String>)> {
    parse_tracked(&fs::read_to_string(settings.adb_tunnel_pid_path()).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pid_port_and_started_at() {
        assert_eq!(
            parse_tracked("1234:5037:2026-07-22T10:34:58.123456789Z"),
            Some((1234, Some(5037), Some("2026-07-22T10:34:58.123456789Z".to_string())))
        );
    }

    #[test]
    fn parses_legacy_pid_and_port_as_unknown_started_at() {
        assert_eq!(parse_tracked("1234:5037"), Some((1234, Some(5037), None)));
    }

    #[test]
    fn parses_legacy_pid_only_as_unknown_port_and_started_at() {
        assert_eq!(parse_tracked("1234"), Some((1234, None, None)));
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
        assert_eq!(parse_tracked("1234:not-a-port"), Some((1234, None, None)));
    }
}
