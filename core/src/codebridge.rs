use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;

use crate::settings::Settings;

/// Fixed loopback port for the reverse `code` channel. The `bsdev` launcher
/// listens on this port on the host and reverse-forwards it into the container
/// (`ssh -R`), so the in-container `code` shim can reach it.
pub const CODE_PORT: u16 = 9918;

/// Start a background listener that turns `code <path>` requests from inside the
/// container into `code --remote ssh-remote+<host> <path>` on this host, opening
/// (and launching, if needed) the host's VSCode connected to the container over
/// Remote-SSH.
///
/// Best-effort and non-blocking: it returns immediately, and the listener lives
/// until the process exits (i.e. until the ssh session started by `connect`
/// ends). If the port can't be bound (e.g. another `bsdev` session already owns
/// it) we skip silently - the in-container shim will just report no bridge.
pub fn spawn_listener(settings: &Settings) {
    let listener = match TcpListener::bind(("127.0.0.1", CODE_PORT)) {
        Ok(l) => l,
        Err(_) => return,
    };
    let remote = format!("ssh-remote+{}", settings.ssh_host);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let remote = remote.clone();
            thread::spawn(move || {
                let mut line = String::new();
                if BufReader::new(stream).read_line(&mut line).is_ok() {
                    let path = line.trim();
                    if !path.is_empty() {
                        let _ = launch_code(&remote, path);
                    }
                }
            });
        }
    });
}

/// Open a container path in the host's VSCode over Remote-SSH. Fire-and-forget:
/// `code` signals the running/new window and returns immediately.
fn launch_code(remote: &str, path: &str) -> std::io::Result<()> {
    code_command()
        .arg("--remote")
        .arg(remote)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(windows)]
fn code_command() -> Command {
    // On Windows `code` is `code.cmd`, which `Command::new("code")` won't resolve
    // (no PATHEXT search), so go through the shell.
    let mut c = Command::new("cmd");
    c.arg("/C").arg("code");
    c
}

#[cfg(not(windows))]
fn code_command() -> Command {
    Command::new("code")
}
