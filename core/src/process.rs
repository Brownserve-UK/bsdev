use std::ffi::OsStr;
use std::process::Command;

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

fn map_spawn(program: &str) -> impl Fn(std::io::Error) -> BsdevError + '_ {
    move |e| match e.kind() {
        std::io::ErrorKind::NotFound => BsdevError::CommandNotFound(program.to_string()),
        _ => BsdevError::Io(e),
    }
}
