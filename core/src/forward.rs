use std::fs;

use crate::error::Result;
use crate::process;
use crate::settings::Settings;
use crate::ssh;
use crate::tunnel::{is_alive, kill};

const SSH: &str = "ssh";

pub fn start(settings: &Settings, port: u16, verbose: bool) -> Result<()> {
    let pid_path = settings.forward_pid_path(port);
    if let Some(pid) = read_tracked(&pid_path) {
        if is_alive(pid) {
            return Ok(());
        }
    }

    let args = ssh::local_forward_args(settings, port);
    let pid = process::spawn_detached(SSH, args, verbose)?;
    fs::write(&pid_path, pid.to_string())?;
    Ok(())
}

pub fn stop(settings: &Settings, port: u16, verbose: bool) {
    let pid_path = settings.forward_pid_path(port);
    if let Some(pid) = read_tracked(&pid_path) {
        kill(pid, verbose);
    }
    let _ = fs::remove_file(&pid_path);
}

fn read_tracked(pid_path: &std::path::Path) -> Option<u32> {
    fs::read_to_string(pid_path).ok()?.trim().parse().ok()
}
