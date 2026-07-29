use anyhow::Context;
use std::path::PathBuf;
use std::process;

/// Get the path to the PID file.
pub fn pid_file_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir()
            .unwrap_or_default()
            .join("cliplinkd")
            .join("daemon.pid")
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".config")
            .join("cliplinkd")
            .join("daemon.pid")
    }
}

/// Check if a daemon process is currently running.
pub fn get_running_pid() -> Option<u32> {
    let path = pid_file_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let pid: u32 = content.trim().parse().ok()?;

    if is_process_alive(pid) {
        Some(pid)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Write PID to the PID file.
pub fn write_pid_file(pid: u32) -> anyhow::Result<()> {
    let path = pid_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, pid.to_string())?;
    Ok(())
}

/// Remove the PID file.
pub fn remove_pid_file() {
    let _ = std::fs::remove_file(pid_file_path());
}

/// Check whether a process with the given PID is alive.
fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_INFORMATION, false, pid);
            if handle.is_err() {
                return false;
            }
            let handle = handle.unwrap();
            let mut exit_code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut exit_code).is_ok();
            let _ = CloseHandle(handle);
            ok && exit_code == STILL_ACTIVE.0 as u32
        }
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        false
    }
}

/// Kill the process with the given PID.
pub fn kill_process(pid: u32) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        unsafe {
            if libc::kill(pid as i32, libc::SIGTERM) != 0 {
                anyhow::bail!("Failed to send SIGTERM to PID {}", pid);
            }
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
                .context("Failed to open process")?;
            TerminateProcess(handle, 0)
                .context("Failed to terminate process")?;
            let _ = CloseHandle(handle);
        }
        Ok(())
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        anyhow::bail!("Process management not supported on this platform");
    }
}

/// Start the daemon in background.
pub fn spawn_daemon() -> anyhow::Result<u32> {
    let exe = std::env::current_exe().context("Failed to get executable path")?;

    #[cfg(unix)]
    {
        let child = process::Command::new(&exe)
            .arg("--daemon")
            .stdin(process::Stdio::null())
            .stdout(process::Stdio::null())
            .stderr(process::Stdio::null())
            .spawn()
            .context("Failed to spawn daemon process")?;
        Ok(child.id())
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let child = process::Command::new(&exe)
            .arg("--daemon")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("Failed to spawn daemon process")?;
        Ok(child.id())
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        anyhow::bail!("Background daemon not supported on this platform");
    }
}

/// Called by the child process with --daemon flag.
/// Writes PID file; terminal detachment is handled by spawn_daemon().
pub fn daemonize() -> anyhow::Result<()> {
    let pid = std::process::id();
    write_pid_file(pid)?;
    Ok(())
}
