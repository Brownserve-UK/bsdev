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
/// container into a host-side `code --folder-uri vscode-remote://attached-container+...`
/// launch, opening (and launching, if needed) the host's VSCode attached to the
/// running container via the Dev Containers extension - no ssh, no ssh config.
///
/// Best-effort and non-blocking: it returns immediately, and the listener lives
/// until the process exits (i.e. until the ssh session started by `connect`
/// ends). If the port can't be bound (e.g. another `bsdev` session already owns
/// it) we skip silently - the in-container shim will just report no bridge.
///
/// The shim sends one line per request: `<kind> <absolute-path>`, where `<kind>`
/// is `dir` or `file` (defaulting to a folder open if the marker is absent).
pub fn spawn_listener(settings: &Settings) {
    let listener = match TcpListener::bind(("127.0.0.1", CODE_PORT)) {
        Ok(l) => l,
        Err(_) => return,
    };
    let container = settings.container.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let container = container.clone();
            thread::spawn(move || {
                let mut line = String::new();
                if BufReader::new(stream).read_line(&mut line).is_ok() {
                    if let Some((is_file, path)) = parse_request(&line) {
                        let _ = launch_code(&container, path, is_file);
                    }
                }
            });
        }
    });
}

/// Parse a `<kind> <path>` request line. Returns `(is_file, path)`, or `None` for
/// a blank line. An unrecognised/absent marker is treated as a folder open.
fn parse_request(line: &str) -> Option<(bool, &str)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    match line.split_once(char::is_whitespace) {
        Some(("file", rest)) => Some((true, rest.trim())),
        Some(("dir", rest)) => Some((false, rest.trim())),
        // No recognised marker: treat the whole line as a folder path.
        _ => Some((false, line)),
    }
}

/// Open a container path in the host's VSCode by attaching to the running
/// container (Dev Containers extension). Fire-and-forget: `code` signals the
/// running/new window and returns immediately.
fn launch_code(container: &str, path: &str, is_file: bool) -> std::io::Result<()> {
    let uri = format!(
        "vscode-remote://attached-container+{}{}",
        hex_encode(container),
        path
    );
    let flag = if is_file { "--file-uri" } else { "--folder-uri" };
    code_command()
        .arg(flag)
        .arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// Lower-hex encode a string's bytes - the form the Dev Containers extension
/// expects for an attached-container authority (`attached-container+<hex>`).
fn hex_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        out.push_str(&format!("{b:02x}"));
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encode_matches_container_name() {
        assert_eq!(hex_encode("bsdev"), "6273646576");
    }

    #[test]
    fn parse_request_handles_markers_and_bare_paths() {
        assert_eq!(parse_request("dir /home/bsdev/repo\n"), Some((false, "/home/bsdev/repo")));
        assert_eq!(parse_request("file /home/bsdev/a.txt\n"), Some((true, "/home/bsdev/a.txt")));
        // No marker -> folder open of the whole line.
        assert_eq!(parse_request("/home/bsdev/repo\n"), Some((false, "/home/bsdev/repo")));
        assert_eq!(parse_request("  \n"), None);
    }

    #[test]
    fn attached_container_uri_shape() {
        // Mirror what launch_code builds for a folder.
        let uri = format!(
            "vscode-remote://attached-container+{}{}",
            hex_encode("bsdev"),
            "/home/bsdev/Repositories/PSTools"
        );
        assert_eq!(
            uri,
            "vscode-remote://attached-container+6273646576/home/bsdev/Repositories/PSTools"
        );
    }
}
