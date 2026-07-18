use std::ffi::OsStr;
use std::process::{Command, Stdio};

use crate::error::{BsdevError, Result};

/// Run an external command with **inherited stdio** (so it shares our terminal -
/// essential for `docker pull` progress and interactive `ssh`) and block until
/// it exits. A missing program becomes a friendly `CommandNotFound`; a non-zero
/// exit becomes `CommandFailed`.
pub fn run<I, S>(program: &str, args: I, verbose: bool) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<S> = args.into_iter().collect();
    if verbose {
        let rendered: Vec<String> = args
            .iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned())
            .collect();
        eprintln!("+ {} {}", program, rendered.join(" "));
    }
    let status = Command::new(program).args(&args).status().map_err(map_spawn(program))?;
    if !status.success() {
        return Err(BsdevError::CommandFailed {
            cmd: program.to_string(),
            code: status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string()),
        });
    }
    Ok(())
}

/// Run a command and capture trimmed stdout. A non-zero exit yields `Ok(None)`
/// so callers can treat "not found" as a state rather than an error (used for
/// `docker inspect`-style queries).
pub fn capture(program: &str, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new(program).args(args).output().map_err(map_spawn(program))?;
    if output.status.success() {
        Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_string()))
    } else {
        Ok(None)
    }
}

/// Spawn `program` fully detached from this process (no inherited stdio, no
/// shared process group/console), so it keeps running after this process (and
/// its controlling terminal) exits. Used for the background adb tunnel, which
/// must outlive the terminal `bsdev` was launched from. Returns the child's PID
/// without waiting for it to exit.
pub fn spawn_detached<I, S>(program: &str, args: I, verbose: bool) -> Result<u32>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<S> = args.into_iter().collect();
    if verbose {
        let rendered: Vec<String> = args
            .iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned())
            .collect();
        eprintln!("+ {} {} (detached)", program, rendered.join(" "));
    }
    let mut cmd = Command::new(program);
    cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut cmd);
    let child = cmd.spawn().map_err(map_spawn(program))?;
    Ok(child.id())
}

/// Detach the child from this process's console/process group so a closed
/// terminal (or exiting parent) doesn't take it down too.
#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
    cmd.creation_flags(0x00000008 | 0x00000200 | 0x08000000);
}

#[cfg(not(windows))]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // A fresh process group so the terminal's SIGHUP (on hangup/close) targets
    // the old foreground group, not this one.
    cmd.process_group(0);
}

fn map_spawn(program: &str) -> impl Fn(std::io::Error) -> BsdevError + '_ {
    move |e| match e.kind() {
        std::io::ErrorKind::NotFound => BsdevError::CommandNotFound(program.to_string()),
        _ => BsdevError::Io(e),
    }
}
