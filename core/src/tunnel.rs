use std::process::{Command, Stdio};

use crate::process;

fn run_quiet(program: &str, args: &[&str], verbose: bool) {
    if verbose {
        eprintln!("+ {} {}", program, args.join(" "));
    }
    let _ = Command::new(program).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

#[cfg(windows)]
pub(crate) fn kill(pid: u32, verbose: bool) {
    run_quiet("taskkill", &["/PID", &pid.to_string(), "/F"], verbose);
}

#[cfg(not(windows))]
pub(crate) fn kill(pid: u32, verbose: bool) {
    run_quiet("kill", &["-TERM", &pid.to_string()], verbose);
}

#[cfg(windows)]
pub(crate) fn is_alive(pid: u32) -> bool {
    let filter = format!("PID eq {pid}");
    match process::capture("tasklist", &["/FI", &filter, "/NH"]) {
        // Note: tasklist still exits 0 with no match, it just prints an "INFO: No tasks ..." line.
        Ok(Some(out)) => out.contains(&pid.to_string()),
        _ => false,
    }
}

#[cfg(not(windows))]
pub(crate) fn is_alive(pid: u32) -> bool {
    // Note: kill -0 sends no signal, it just checks the process exists and is ours to signal.
    matches!(process::capture("kill", &["-0", &pid.to_string()]), Ok(Some(_)))
}
