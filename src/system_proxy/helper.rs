use super::ProxyStatus;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Request {
    Status,
    Enable { port: u16 },
    Restore,
    Heartbeat,
    Exit,
}
#[derive(Serialize, Deserialize)]
struct Response {
    ok: bool,
    status: ProxyStatus,
    error: String,
}
pub const MARKER: &str = "system-proxy.pending.json";

pub fn query(dir: &Path, request: Request) -> Result<ProxyStatus> {
    let mut stream = UnixStream::connect(dir.join("system-proxy.sock"))
        .context("System proxy helper unavailable; authorization / recovery required")?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    serde_json::to_writer(&mut stream, &request)?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).take(65536).read_line(&mut line)?;
    let response: Response = serde_json::from_str(&line)?;
    ensure!(response.ok, "{}", response.error);
    Ok(response.status)
}
pub fn status(dir: &Path, port: u16) -> ProxyStatus {
    if let Ok(s) = query(dir, Request::Status) {
        return s;
    }
    let pending = dir.join(MARKER).exists();
    #[cfg(target_os = "macos")]
    let effective = super::macos::effective(port).unwrap_or(false);
    #[cfg(not(target_os = "macos"))]
    let effective = false;
    ProxyStatus {
        supported: cfg!(target_os = "macos"),
        pending_restore: pending,
        safe_to_stop: !pending,
        effective,
        detail: if pending {
            "System proxy recovery may be pending; authorize Restore before stopping the core"
        } else {
            "System proxy not managed by this instance"
        }
        .into(),
        ..Default::default()
    }
}
pub fn restore(dir: &Path) -> Result<ProxyStatus> {
    if !dir.join(MARKER).exists() {
        return Ok(ProxyStatus {
            supported: cfg!(target_os = "macos"),
            safe_to_stop: true,
            ..Default::default()
        });
    }
    let status = query(dir, Request::Restore)?;
    ensure!(status.safe_to_stop, "{}", status.detail);
    match std::fs::remove_file(dir.join(MARKER)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(status)
}
pub fn enable(dir: &Path, port: u16) -> Result<ProxyStatus> {
    crate::model::atomic_write(
        &dir.join(MARKER),
        &serde_json::to_vec(&serde_json::json!({"port":port,"version":1}))?,
    )?;
    query(dir, Request::Enable { port })
}
pub fn authorize(dir: &Path) -> Result<()> {
    ensure!(
        cfg!(target_os = "macos"),
        "System proxy integration is macOS-only"
    );
    #[cfg(target_os = "macos")]
    {
        // Only the unprivileged caller touches its application directory. The
        // privileged helper binds in a root-owned directory, avoiding pathname
        // replacement races during chmod/chown in a user-writable directory.
        use std::os::unix::fs::{symlink, FileTypeExt};
        ensure!(
            unsafe { libc::geteuid() } != 0,
            "Run sing as your normal user"
        );
        let link = dir.join("system-proxy.sock");
        if let Ok(meta) = std::fs::symlink_metadata(&link) {
            ensure!(
                meta.file_type().is_socket() || meta.file_type().is_symlink(),
                "Unexpected file at helper socket path; refusing to replace it"
            );
            std::fs::remove_file(&link)?;
        }
        symlink(privileged_socket(dir, unsafe { libc::geteuid() })?, link)?;
    }
    let result = std::process::Command::new("sudo")
        .arg("-b")
        .arg(std::env::current_exe()?)
        .arg("--data-dir")
        .arg(dir)
        .arg("--system-proxy-helper")
        .status()?;
    ensure!(
        result.success(),
        "System proxy authorization cancelled or failed"
    );
    for _ in 0..40 {
        if query(dir, Request::Status).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("Helper did not start. Another sing instance may own the system proxy, or recovery may require attention.")
}

#[cfg(target_os = "macos")]
fn privileged_socket(dir: &Path, uid: u32) -> Result<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    let canonical = std::fs::canonicalize(dir)?;
    let digest = format!(
        "{:x}",
        Sha256::digest(canonical.as_os_str().as_encoded_bytes())
    );
    Ok(Path::new("/private/var/run/sing-proxy").join(format!("{uid}-{}.sock", &digest[..20])))
}

#[derive(Default)]
struct Watchdog {
    dead_checks: u8,
}
impl Watchdog {
    fn should_restore(&mut self, heartbeat_age: Duration, port_alive: bool) -> bool {
        self.dead_checks = if port_alive {
            0
        } else {
            self.dead_checks.saturating_add(1)
        };
        heartbeat_age > Duration::from_secs(12) || self.dead_checks >= 3
    }
}

/// A daemon-owned lease survives closing the UI but not a killed manager.
pub struct Lease {
    enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Lease {
    pub fn start(dir: &Path) -> Self {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let enabled = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let e = enabled.clone();
        let s = stop.clone();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                if e.load(Ordering::Relaxed) {
                    let _ = query(&dir, Request::Heartbeat);
                }
                for _ in 0..20 {
                    if s.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        Self { enabled, stop }
    }
    pub fn set(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn run(_dir: &Path) -> Result<()> {
    anyhow::bail!("System proxy integration is macOS-only")
}

#[cfg(target_os = "macos")]
pub fn run(dir: &Path) -> Result<()> {
    use super::{macos::MacBackend, Controller};
    use fs2::FileExt;
    use std::{
        fs::{self, OpenOptions},
        os::unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            io::AsRawFd,
            net::UnixListener,
        },
        sync::atomic::{AtomicBool, Ordering},
        time::Instant,
    };
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "Explicit sudo authorization required"
    );
    let dir = fs::canonicalize(dir)?;
    let meta = fs::symlink_metadata(&dir)?;
    let uid = std::env::var("SUDO_UID")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .context("Run this helper through sudo from your user account")?;
    ensure!(
        uid != 0 && meta.uid() == uid && meta.is_dir() && meta.mode() & 0o077 == 0,
        "Application directory must belong to the requesting user and be private"
    );
    let root = Path::new("/private/var/db/sing");
    if root.exists() {
        let m = fs::symlink_metadata(root)?;
        ensure!(
            m.is_dir() && !m.file_type().is_symlink() && m.uid() == 0 && m.mode() & 0o077 == 0,
            "Unsafe root recovery directory"
        );
    } else {
        crate::model::private_dir(root)?;
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(root.join("system-proxy.lock"))?;
    lock.try_lock_exclusive()
        .context("Another sing instance owns the system proxy helper")?;
    let mut controller = Controller::load(
        &root.join("system-proxy.json"),
        format!("{uid}:{}", dir.display()),
    )?;
    let mut backend = MacBackend::new()?;
    let mut detail = String::new();
    // A previous helper may have died after committing. Recover before accepting a new session.
    if controller.pending() {
        detail = match controller.restore(&mut backend) {
            Ok(s) => s.detail,
            Err(e) => format!("Recovery pending: {e}"),
        };
    }
    let socket_dir = Path::new("/private/var/run/sing-proxy");
    if !socket_dir.exists() {
        fs::DirBuilder::new().create(socket_dir)?;
        fs::set_permissions(socket_dir, fs::Permissions::from_mode(0o711))?;
    }
    let m = fs::symlink_metadata(socket_dir)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink() && m.uid() == 0 && m.mode() & 0o022 == 0,
        "Unsafe privileged socket directory"
    );
    let socket = privileged_socket(&dir, uid)?;
    if socket.exists() {
        fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let path = std::ffi::CString::new(socket.as_os_str().as_encoded_bytes())?;
    ensure!(
        unsafe { libc::chown(path.as_ptr(), uid, meta.gid()) } == 0,
        "Cannot set helper socket owner"
    );
    // Detach only after sudo has handled its terminal password prompt.
    unsafe {
        libc::setsid();
    }
    let null = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    unsafe {
        for fd in 0..3 {
            libc::dup2(null.as_raw_fd(), fd);
        }
    }
    static STOP: AtomicBool = AtomicBool::new(false);
    extern "C" fn signal(_: libc::c_int) {
        STOP.store(true, Ordering::Relaxed);
    }
    unsafe {
        libc::signal(libc::SIGTERM, signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, signal as *const () as libc::sighandler_t);
    }
    let mut heartbeat = Instant::now();
    let mut activity = Instant::now();
    let mut check = Instant::now();
    let mut watchdog = Watchdog::default();
    while !STOP.load(Ordering::Relaxed) {
        if check.elapsed() > Duration::from_secs(1) {
            check = Instant::now();
            if let Some(port) = controller.port() {
                let alive = std::net::TcpStream::connect_timeout(
                    &format!("127.0.0.1:{port}").parse()?,
                    Duration::from_millis(150),
                )
                .is_ok();
                if watchdog.should_restore(heartbeat.elapsed(), alive) {
                    detail = match controller.restore(&mut backend) {
                        Ok(s) => format!("Safety recovery: {}", s.detail),
                        Err(e) => format!("Safety recovery failed; use Restore: {e}"),
                    };
                }
            } else if activity.elapsed() > Duration::from_secs(300) {
                break;
            }
        }
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let mut peer = 0;
        let mut group = 0;
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut peer, &mut group) } != 0
            || peer != uid
        {
            continue;
        }
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut line = String::new();
        if BufReader::new(&stream)
            .take(4096)
            .read_line(&mut line)
            .is_err()
        {
            continue;
        }
        let mut exit = false;
        let result = (|| -> Result<ProxyStatus> {
            match serde_json::from_str::<Request>(&line)? {
                Request::Enable { port } => {
                    heartbeat = Instant::now();
                    activity = Instant::now();
                    watchdog = Watchdog::default();
                    ensure!(
                        std::net::TcpStream::connect_timeout(
                            &format!("127.0.0.1:{port}").parse()?,
                            Duration::from_millis(200)
                        )
                        .is_ok(),
                        "Core port is not ready; system proxy was not changed"
                    );
                    let s = controller.enable(&mut backend, port)?;
                    detail = s.detail;
                }
                Request::Restore => {
                    activity = Instant::now();
                    detail = controller.restore(&mut backend)?.detail;
                }
                Request::Heartbeat => {
                    heartbeat = Instant::now();
                }
                Request::Exit => {
                    let s = controller.restore(&mut backend)?;
                    ensure!(s.safe_to_stop, "{}", s.detail);
                    detail = s.detail;
                    exit = true;
                }
                Request::Status => {}
            }
            let mut s = controller.inspect(&mut backend)?;
            if !detail.is_empty() && (!s.pending_restore || s.configured) {
                s.detail = detail.clone();
            }
            if let Some(port) = controller.port() {
                s.effective = super::macos::effective(port).unwrap_or(false);
            }
            Ok(s)
        })();
        let response = match result {
            Ok(status) => Response {
                ok: true,
                status,
                error: String::new(),
            },
            Err(e) => Response {
                ok: false,
                status: ProxyStatus::default(),
                error: crate::model::clean(&format!("{e:#}")),
            },
        };
        let _ = serde_json::to_writer(&mut stream, &response);
        let _ = stream.write_all(b"\n");
        if exit {
            break;
        }
    }
    // SIGKILL / power loss cannot execute cleanup; the root journal is retained for recovery.
    let _ = controller.restore(&mut backend);
    let _ = fs::remove_file(socket);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watchdog_recovers_dead_manager_even_when_core_port_is_alive() {
        let mut w = Watchdog::default();
        assert!(!w.should_restore(Duration::from_secs(12), true));
        assert!(w.should_restore(Duration::from_secs(13), true));
    }
    #[test]
    fn watchdog_requires_three_consecutive_failed_port_checks() {
        let mut w = Watchdog::default();
        let age = Duration::from_secs(1);
        assert!(!w.should_restore(age, false));
        assert!(!w.should_restore(age, false));
        assert!(!w.should_restore(age, true));
        assert!(!w.should_restore(age, false));
        assert!(!w.should_restore(age, false));
        assert!(w.should_restore(age, false));
    }
}
