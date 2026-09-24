//! TUN owns its DNS handoff for the entire connection lifetime. Recovery uses
//! the same lifecycle as Start/Stop, including when the manager or core exits.
use super::*;
use std::{os::fd::AsRawFd, time::Instant};

const MARKER: &str = "tun.pending.json";

pub fn pending(dir: &Path) -> bool {
    cfg!(target_os = "macos") && dir.join(MARKER).exists()
}

pub(super) fn available(dir: &Path) -> bool {
    UnixStream::connect(dir.join("tun.sock")).is_ok()
}

pub(super) fn request(dir: &Path, action: &str) -> Result<String> {
    if action == "start" {
        // Wait for startup recovery before writing the next session's marker.
        ensure!(
            request(dir, "status")? == "stopped",
            "A TUN connection is already running"
        );
    }
    let mut stream = UnixStream::connect(dir.join("tun.sock"))
        .context("macOS connection authorization is needed")?;
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    if action == "start" && cfg!(target_os = "macos") {
        // A user-readable hint, never the backup itself. The original settings
        // stay in the root-owned journal and are verified by the helper.
        model::atomic_write(&dir.join(MARKER), b"{}")?;
    }
    writeln!(stream, "{action}")?;
    let mut line = String::new();
    BufReader::new(stream).take(4096).read_line(&mut line)?;
    let line = line.trim();
    ensure!(!line.is_empty(), "Connection helper closed before replying");
    ensure!(
        !line.starts_with("error:"),
        "{}",
        line.trim_start_matches("error: ")
    );
    ensure!(
        ["running", "stopped"].contains(&line),
        "Invalid connection helper reply"
    );
    if line == "stopped" && pending(dir) {
        fs::remove_file(dir.join(MARKER))?;
    }
    Ok(line.into())
}

trait Dns {
    fn enable(&mut self, config: &serde_json::Value) -> Result<()>;
    fn restore(&mut self) -> Result<()>;
}

struct SystemDns;
impl Dns for SystemDns {
    fn enable(&mut self, _config: &serde_json::Value) -> Result<()> {
        #[cfg(target_os = "macos")]
        crate::tun_dns::enable(_config)?;
        Ok(())
    }
    fn restore(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        crate::tun_dns::restore()?;
        Ok(())
    }
}

// Recovery belongs to the session, not to a retained, already-reaped PID.
// A failed Stop stays in Stopping and retries until the original DNS is back.
struct Session<D: Dns> {
    dns: D,
    child: Option<Child>,
    stopping: bool,
    cleanup: bool,
    error: String,
    retry_at: Instant,
}
impl<D: Dns> Session<D> {
    fn new(dns: D) -> Self {
        Self {
            dns,
            child: None,
            stopping: true,
            cleanup: true,
            error: String::new(),
            retry_at: Instant::now(),
        }
    }
    fn start(&mut self, child: Child, config: &serde_json::Value) -> Result<()> {
        debug_assert!(self.child.is_none() && !self.cleanup);
        self.child = Some(child);
        self.cleanup = true;
        if let Err(start) = self.dns.enable(config) {
            return match self.stop() {
                Ok(()) => Err(start.context("Connection could not start; original DNS restored")),
                Err(recovery) => Err(start.context(format!(
                    "Connection could not start; restoring DNS: {recovery}"
                ))),
            };
        }
        self.stopping = false;
        self.error.clear();
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        self.stopping = true;
        let result = (|| {
            if self.cleanup {
                self.dns.restore()?;
            }
            if let Some(child) = self.child.as_mut() {
                terminate(child)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.child = None;
                self.cleanup = false;
                self.stopping = false;
                self.error.clear();
                Ok(())
            }
            Err(error) => {
                self.error = format!("{error:#}");
                self.retry_at = Instant::now() + Duration::from_secs(1);
                Err(error)
            }
        }
    }
    fn poll(&mut self) -> Result<()> {
        if self
            .child
            .as_mut()
            .map(Child::try_wait)
            .transpose()?
            .flatten()
            .is_some()
        {
            self.child = None;
            self.stopping = true;
        }
        if self.stopping && Instant::now() >= self.retry_at {
            let _ = self.stop(); // Error is retained in status; the next tick retries.
        }
        Ok(())
    }
    fn status(&self) -> Result<String> {
        ensure!(
            self.error.is_empty(),
            "Restoring network settings: {}",
            self.error
        );
        Ok(if self.child.is_some() {
            "running"
        } else {
            "stopped"
        }
        .into())
    }
}

fn terminate(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    for _ in 0..40 {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill()?;
    child.wait()?;
    Ok(())
}

fn unlink_entry(directory: &fs::File, name: &str) -> Result<()> {
    let name = std::ffi::CString::new(name)?;
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error.into());
        }
    }
    Ok(())
}

fn manager_alive(lock: &fs::File) -> Result<bool> {
    match lock.try_lock_exclusive() {
        Ok(()) => {
            FileExt::unlock(lock)?;
            Ok(false)
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
        Err(e) => Err(e.into()),
    }
}

pub fn run(dir: &Path, core: Option<&Path>) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "macOS connection management requires administrator access"
    );
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(dir)?;
    let meta = directory.metadata()?;
    ensure!(
        meta.is_dir() && meta.mode() & 0o077 == 0,
        "Data directory must be private"
    );
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join("tun.lock"))?;
    lock.try_lock_exclusive()
        .context("Connection management is already running")?;
    #[cfg(target_os = "macos")]
    let _dns_lock = crate::tun_dns::lock()?;

    // Recovery after a killed helper also works if the core was uninstalled.
    // The existing authorized helper role performs it; no user recovery CLI.
    let Some(core) = core else {
        SystemDns.restore()?;
        unlink_entry(&directory, MARKER)?;
        return Ok(());
    };
    ensure!(
        core.is_absolute() && core.is_file(),
        "Core must be an absolute executable path"
    );
    let manager = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join("manager.lock"))?;
    ensure!(manager_alive(&manager)?, "Start the connection from sing");
    unlink_entry(&directory, "tun.sock")?;
    let socket = dir.join("tun.sock");
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let cpath = std::ffi::CString::new(socket.as_os_str().as_encoded_bytes())?;
    ensure!(
        unsafe { libc::chown(cpath.as_ptr(), meta.uid(), meta.gid()) } == 0,
        "Cannot set helper socket owner"
    );

    // Background helper messages must never overwrite the terminal interface.
    let null = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    unsafe {
        libc::setsid();
        for fd in 0..3 {
            libc::dup2(null.as_raw_fd(), fd);
        }
    }
    static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    extern "C" fn signal(_: libc::c_int) {
        STOP.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    unsafe {
        libc::signal(libc::SIGTERM, signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, signal as *const () as libc::sighandler_t);
    }
    serve(dir, core, &directory, &manager, listener, &STOP, SystemDns)
}

fn serve<D: Dns>(
    dir: &Path,
    core: &Path,
    directory: &fs::File,
    manager: &fs::File,
    listener: UnixListener,
    stop: &std::sync::atomic::AtomicBool,
    dns: D,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    let owner_uid = directory.metadata()?.uid();
    let mut session = Session::new(dns);
    let mut exiting = false;
    let mut last_owner_check = Instant::now();
    loop {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            exiting = true;
        }
        if last_owner_check.elapsed() >= Duration::from_secs(1) {
            exiting |= !manager_alive(manager).unwrap_or(false);
            last_owner_check = Instant::now();
        }
        if exiting {
            session.stopping = true;
        }
        let was_pending = session.cleanup;
        if let Err(error) = session.poll() {
            session.error = error.to_string();
            session.stopping = true;
        }
        if was_pending && !session.cleanup {
            unlink_entry(directory, MARKER)?;
        }
        if exiting && !session.cleanup && session.child.is_none() {
            break;
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                exiting = true;
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        #[cfg(target_os = "macos")]
        {
            let (mut uid, mut gid) = (0, 0);
            if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0
                || uid != owner_uid
            {
                continue;
            }
        }
        if stream.set_nonblocking(false).is_err()
            || stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .is_err()
            || stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .is_err()
        {
            continue;
        }
        let mut action = String::new();
        if BufReader::new(&stream)
            .take(64)
            .read_line(&mut action)
            .is_err()
        {
            continue;
        }
        let result: Result<String> = (|| {
            session.poll()?;
            match action.trim() {
                "start" => {
                    ensure!(
                        !exiting && !session.cleanup && session.child.is_none(),
                        "Previous connection is still active or being restored"
                    );
                    let config: serde_json::Value =
                        serde_json::from_slice(&fs::read(dir.join("runtime.json"))?)?;
                    ensure!(
                        config["inbounds"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|i| i["type"] == "tun")),
                        "No TUN configured"
                    );
                    session.start(super::spawn_core(core, dir)?, &config)?;
                }
                "stop" | "exit" => {
                    if action.trim() == "exit" {
                        exiting = true;
                    }
                    session.stop()?;
                    unlink_entry(directory, MARKER)?;
                }
                "status" => {}
                _ => bail!("Unknown connection helper request"),
            }
            session.status()
        })();
        let _ = writeln!(
            stream,
            "{}",
            result.unwrap_or_else(|e| format!("error: {}", model::clean(&format!("{e:#}"))))
        );
    }
    unlink_entry(directory, MARKER)?;
    unlink_entry(directory, "tun.sock")?;
    Ok(())
}

pub fn authorize(dir: &Path, core: &str) -> Result<()> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(dir.join("core.log"))?;
    let result = Command::new("sudo")
        .arg("-b")
        .arg(std::env::current_exe()?)
        .arg("--data-dir")
        .arg(dir)
        .arg("--tun-helper")
        .arg("--core")
        .arg(fs::canonicalize(core)?)
        .status()?;
    ensure!(
        result.success(),
        "Administrator authorization cancelled or failed"
    );
    for _ in 0..40 {
        if available(dir) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("macOS connection management could not start")
}

pub fn authorize_recovery(dir: &Path) -> Result<()> {
    let result = Command::new("sudo")
        .arg(std::env::current_exe()?)
        .arg("--data-dir")
        .arg(dir)
        .arg("--tun-helper")
        .status()?;
    ensure!(
        result.success(),
        "macOS could not finish restoring network settings"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Uses isolated Unix sockets and a fake core; never changes system DNS"]
    fn helper_lifecycle_recovers_when_manager_disappears() {
        let dir = tempfile::tempdir().unwrap();
        let core = dir.path().join("fake-core");
        fs::write(&core, "#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
        fs::set_permissions(&core, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            dir.path().join("runtime.json"),
            r#"{"inbounds":[{"type":"tun"}]}"#,
        )
        .unwrap();
        let owner = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.path().join("manager.lock"))
            .unwrap();
        owner.lock_exclusive().unwrap();
        let watcher = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.path().join("manager.lock"))
            .unwrap();
        let directory = fs::File::open(dir.path()).unwrap();
        let listener = UnixListener::bind(dir.path().join("tun.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let path = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            serve(
                &path,
                &core,
                &directory,
                &watcher,
                listener,
                &std::sync::atomic::AtomicBool::new(false),
                FakeDns::default(),
            )
        });
        let checks: Result<()> = (|| {
            ensure!(request(dir.path(), "start")? == "running", "start");
            ensure!(
                !cfg!(target_os = "macos") || pending(dir.path()),
                "active session must retain recovery marker"
            );
            ensure!(request(dir.path(), "stop")? == "stopped", "stop");
            ensure!(!pending(dir.path()), "restore marker must be cleared");
            ensure!(request(dir.path(), "start")? == "running", "restart");
            Ok(())
        })();
        drop(owner); // Simulate a killed manager while its core is running.
        let result = server.join().unwrap();
        checks.unwrap();
        result.unwrap();
        assert!(!pending(dir.path()));
        assert!(!dir.path().join("tun.sock").exists());
    }
    #[derive(Default)]
    struct FakeDns {
        restores: usize,
        fail_restore: bool,
        fail_enable: bool,
    }
    impl Dns for FakeDns {
        fn enable(&mut self, _: &serde_json::Value) -> Result<()> {
            ensure!(!self.fail_enable, "DNS unavailable");
            Ok(())
        }
        fn restore(&mut self) -> Result<()> {
            self.restores += 1;
            ensure!(!self.fail_restore, "settings busy");
            Ok(())
        }
    }
    fn sleeping_child() -> Child {
        Command::new("/bin/sleep").arg("30").spawn().unwrap()
    }
    #[test]
    fn failed_stop_retries_and_keeps_core_until_dns_is_restored() {
        let mut session = Session::new(FakeDns::default());
        session.poll().unwrap();
        session
            .start(sleeping_child(), &serde_json::json!({}))
            .unwrap();
        session.dns.fail_restore = true;
        assert!(session.stop().is_err());
        assert!(session
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none());
        assert!(session.status().is_err());
        session.dns.fail_restore = false;
        session.retry_at = Instant::now();
        session.poll().unwrap();
        assert_eq!(session.status().unwrap(), "stopped");
        assert!(!session.cleanup);
    }
    #[test]
    fn core_crash_restores_without_client_request_and_can_restart() {
        let mut session = Session::new(FakeDns::default());
        session.poll().unwrap();
        session
            .start(sleeping_child(), &serde_json::json!({}))
            .unwrap();
        session.child.as_mut().unwrap().kill().unwrap();
        session.child.as_mut().unwrap().wait().unwrap();
        session.dns.fail_restore = true;
        session.poll().unwrap();
        assert!(session.child.is_none());
        assert!(session.cleanup && session.stopping);
        session.dns.fail_restore = false;
        session.retry_at = Instant::now();
        session.poll().unwrap();
        assert_eq!(session.status().unwrap(), "stopped");
        session
            .start(sleeping_child(), &serde_json::json!({}))
            .unwrap();
        session.stop().unwrap();
    }
    #[test]
    fn failed_start_rolls_back_and_does_not_leave_a_core() {
        let mut session = Session::new(FakeDns {
            fail_enable: true,
            ..Default::default()
        });
        session.poll().unwrap();
        assert!(session
            .start(sleeping_child(), &serde_json::json!({}))
            .is_err());
        assert!(session.child.is_none() && !session.cleanup);
    }
    #[test]
    fn manager_watchdog_observes_process_lock_not_ui_lifetime() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manager.lock");
        let owner = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        owner.lock_exclusive().unwrap();
        let watcher = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(manager_alive(&watcher).unwrap());
        drop(owner);
        assert!(!manager_alive(&watcher).unwrap());
    }
}
