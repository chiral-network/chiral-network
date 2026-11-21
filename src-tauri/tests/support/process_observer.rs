//! Process creation observed from outside the observed process, on macOS
//! (`application-startup-boundaries` tasks 3.1 and 3.2).
//!
//! `Observed::spawn` starts a program with `posix_spawn` and `POSIX_SPAWN_START_SUSPENDED`, so
//! that it runs no instruction before a kqueue `EVFILT_PROC` filter with `NOTE_FORK`, `NOTE_EXEC`
//! and `NOTE_EXIT` is registered on its process id; only then is it resumed with `SIGCONT`. The
//! kernel posts `NOTE_FORK` for every `fork`, `vfork` and `posix_spawn` of the observed process
//! (Rust's `std::process::Command` and tokio's use `posix_spawn` on macOS), so `forks() == 0`
//! after the process has exited means that it created no process at all during its whole life.
//! Notifications that arrive before they are read are coalesced, so the count is a lower bound
//! of the processes created; only zero is exact.
//!
//! `children` lists the current children of a process, zombies included, and
//! `reaped_children_elapsed` reads the total elapsed time of the children it has reaped
//! (`proc_pid_rusage`); a process that has created processes is the parent of every one it has
//! not reaped, and has a nonzero reaped total for every one it has.
//!
//! No Linux implementation: the Linux evidence that the tasks name (`/proc`) is still open.
#![cfg(target_os = "macos")]
#![allow(dead_code)]

use std::ffi::{CString, OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};

/// The environment for a child: this process's, without the variables named in `remove`,
/// with `set` added or replaced.
pub fn environment(remove: &[&str], set: &[(&str, &OsStr)]) -> Vec<(OsString, OsString)> {
    let mut variables: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| !remove.iter().any(|removed| name == OsStr::new(removed)))
        .filter(|(name, _)| !set.iter().any(|(set, _)| name == OsStr::new(set)))
        .collect();
    variables.extend(set.iter().map(|(name, value)| (OsString::from(name), value.to_os_string())));
    variables
}

/// Proxy variables, removed so that a loopback chain RPC is contacted directly.
pub const PROXY_VARIABLES: [&str; 8] =
    ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"];

fn c_string(bytes: &[u8]) -> CString {
    CString::new(bytes).expect("no interior NUL byte")
}

/// A process started suspended and watched by kqueue from before its first instruction.
pub struct Observed {
    pid: libc::pid_t,
    queue: libc::c_int,
    forks: usize,
    exited: bool,
    status: Option<libc::c_int>,
}

impl Observed {
    /// Starts `program` with `args` and `environment`, its standard input `/dev/null` and its
    /// standard output and error written to `stdout` and `stderr`. No other descriptor of this
    /// process is inherited (`POSIX_SPAWN_CLOEXEC_DEFAULT`); `SIGPIPE` has its default action and
    /// no signal is blocked, as `std::process::Command` arranges.
    pub fn spawn(program: &Path, args: &[&OsStr], environment: &[(OsString, OsString)], stdout: &Path, stderr: &Path) -> Self {
        let program_c = c_string(program.as_os_str().as_bytes());
        let mut argv_c = vec![program_c.clone()];
        argv_c.extend(args.iter().map(|arg| c_string(arg.as_bytes())));
        let mut argv: Vec<*mut libc::c_char> = argv_c.iter().map(|arg| arg.as_ptr() as *mut libc::c_char).collect();
        argv.push(std::ptr::null_mut());
        let envp_c: Vec<CString> = environment
            .iter()
            .map(|(name, value)| {
                let mut entry = name.as_bytes().to_vec();
                entry.push(b'=');
                entry.extend_from_slice(value.as_bytes());
                c_string(&entry)
            })
            .collect();
        let mut envp: Vec<*mut libc::c_char> = envp_c.iter().map(|entry| entry.as_ptr() as *mut libc::c_char).collect();
        envp.push(std::ptr::null_mut());
        let null = c_string(b"/dev/null");
        let out = c_string(stdout.as_os_str().as_bytes());
        let err = c_string(stderr.as_os_str().as_bytes());
        let mut pid: libc::pid_t = 0;
        // SAFETY: every pointer passed lives until `posix_spawn` returns; the attribute and file
        // action objects are initialized before use and destroyed after it.
        unsafe {
            let mut attributes: libc::posix_spawnattr_t = std::ptr::null_mut();
            assert_eq!(libc::posix_spawnattr_init(&mut attributes), 0);
            let flags = libc::POSIX_SPAWN_START_SUSPENDED | libc::POSIX_SPAWN_CLOEXEC_DEFAULT | libc::POSIX_SPAWN_SETSIGDEF | libc::POSIX_SPAWN_SETSIGMASK;
            assert_eq!(libc::posix_spawnattr_setflags(&mut attributes, flags as libc::c_short), 0);
            let mut defaults: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut defaults);
            libc::sigaddset(&mut defaults, libc::SIGPIPE);
            assert_eq!(libc::posix_spawnattr_setsigdefault(&mut attributes, &defaults), 0);
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            assert_eq!(libc::posix_spawnattr_setsigmask(&mut attributes, &mask), 0);
            let mut actions: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
            assert_eq!(libc::posix_spawn_file_actions_init(&mut actions), 0);
            assert_eq!(libc::posix_spawn_file_actions_addopen(&mut actions, 0, null.as_ptr(), libc::O_RDONLY, 0), 0);
            let write = libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
            assert_eq!(libc::posix_spawn_file_actions_addopen(&mut actions, 1, out.as_ptr(), write, 0o600), 0);
            assert_eq!(libc::posix_spawn_file_actions_addopen(&mut actions, 2, err.as_ptr(), write, 0o600), 0);
            let spawned = libc::posix_spawn(&mut pid, program_c.as_ptr(), &actions, &attributes, argv.as_ptr(), envp.as_ptr());
            libc::posix_spawn_file_actions_destroy(&mut actions);
            libc::posix_spawnattr_destroy(&mut attributes);
            assert_eq!(spawned, 0, "posix_spawn {program:?}: {}", std::io::Error::from_raw_os_error(spawned));
        }
        // SAFETY: plain system calls on descriptors and a process id this value owns.
        let queue = unsafe { libc::kqueue() };
        assert!(queue >= 0, "kqueue: {}", std::io::Error::last_os_error());
        let mut observed = Self { pid, queue, forks: 0, exited: false, status: None };
        // SAFETY: an all-zero `kevent` is a valid value to fill in.
        let mut change: libc::kevent = unsafe { std::mem::zeroed() };
        change.ident = pid as libc::uintptr_t;
        change.filter = libc::EVFILT_PROC;
        change.flags = libc::EV_ADD | libc::EV_CLEAR;
        change.fflags = libc::NOTE_FORK | libc::NOTE_EXEC | libc::NOTE_EXIT;
        // SAFETY: one valid change, no event output.
        let registered = unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
        assert_eq!(registered, 0, "kevent registration: {}", std::io::Error::last_os_error());
        // SAFETY: the process is our suspended child.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGCONT) }, 0, "resume: {}", std::io::Error::last_os_error());
        observed.drain(Duration::ZERO);
        observed
    }

    pub fn pid(&self) -> u32 {
        self.pid as u32
    }

    /// Reads the notifications that arrive within `timeout`, stopping at the exit notification.
    fn drain(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let wait = libc::timespec { tv_sec: left.as_secs() as libc::time_t, tv_nsec: left.subsec_nanos() as libc::c_long };
            // SAFETY: an all-zero array of `kevent` is valid output storage.
            let mut events: [libc::kevent; 8] = unsafe { std::mem::zeroed() };
            // SAFETY: no changes; output into `events`, with its length.
            let count = unsafe { libc::kevent(self.queue, std::ptr::null(), 0, events.as_mut_ptr(), events.len() as libc::c_int, &wait) };
            if count < 0 {
                assert_eq!(std::io::Error::last_os_error().kind(), std::io::ErrorKind::Interrupted, "kevent");
                continue;
            }
            for event in &events[..count as usize] {
                let flags = { event.fflags };
                if flags & libc::NOTE_FORK != 0 {
                    self.forks += 1;
                }
                if flags & libc::NOTE_EXIT != 0 {
                    self.exited = true;
                }
            }
            if count == 0 || self.exited || Instant::now() >= deadline {
                return;
            }
        }
    }

    /// The fork notifications received so far: zero exactly when the process has created no
    /// process so far.
    pub fn forks(&mut self) -> usize {
        self.drain(Duration::ZERO);
        self.forks
    }

    /// Waits, at most `timeout`, for the process to exit, without reaping it: whether it did.
    pub fn exited_within(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !self.exited && Instant::now() < deadline {
            self.drain(deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(200)));
        }
        self.exited
    }

    /// Reaps the process if it has exited: its exit code, or `128 + signal` when a signal ended
    /// it. Every notification posted before the exit has been read by then.
    pub fn try_wait(&mut self) -> Option<i32> {
        if self.status.is_none() {
            let mut status: libc::c_int = 0;
            // SAFETY: waiting for our own child.
            let waited = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if waited == self.pid {
                self.status = Some(status);
                self.exited = true;
            } else {
                assert_eq!(waited, 0, "waitpid: {}", std::io::Error::last_os_error());
            }
        }
        // The exit notification follows every other one; read all of them.
        self.drain(Duration::ZERO);
        self.status.map(|status| if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) })
    }

    /// Waits, at most `timeout`, for the process to exit and reaps it.
    pub fn wait_timeout(&mut self, timeout: Duration) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(code) = self.try_wait() {
                return Some(code);
            }
            if Instant::now() >= deadline {
                return None;
            }
            self.drain(Duration::from_millis(50));
        }
    }

    pub fn signal(&self, signal: libc::c_int) {
        if self.status.is_none() {
            // SAFETY: our own unreaped child.
            assert_eq!(unsafe { libc::kill(self.pid, signal) }, 0, "kill: {}", std::io::Error::last_os_error());
        }
    }
}

impl Drop for Observed {
    fn drop(&mut self) {
        if self.status.is_none() {
            // SAFETY: our own unreaped child: killed, then reaped.
            unsafe {
                libc::kill(self.pid, libc::SIGKILL);
                let mut status = 0;
                libc::waitpid(self.pid, &mut status, 0);
            }
        }
        // SAFETY: the descriptor this value owns.
        unsafe { libc::close(self.queue) };
    }
}

/// The current children of `pid`, zombies included.
pub fn children(pid: u32) -> Vec<u32> {
    let mut buffer = [0 as libc::pid_t; 256];
    // SAFETY: output into `buffer`, with its size in bytes.
    let count = unsafe { libc::proc_listchildpids(pid as libc::pid_t, buffer.as_mut_ptr().cast(), std::mem::size_of_val(&buffer) as libc::c_int) };
    assert!(count >= 0, "proc_listchildpids: {}", std::io::Error::last_os_error());
    buffer[..(count as usize).min(buffer.len())].iter().map(|pid| *pid as u32).collect()
}

/// The executable file that process `pid` runs.
pub fn executable(pid: u32) -> std::path::PathBuf {
    let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: output into `buffer`, with its size.
    let length = unsafe { libc::proc_pidpath(pid as libc::c_int, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    assert!(length > 0, "proc_pidpath: {}", std::io::Error::last_os_error());
    std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&buffer[..length as usize]))
}

/// The total elapsed time, in Mach absolute time units, of the children that `pid` has reaped:
/// zero exactly when it has reaped none.
pub fn reaped_children_elapsed(pid: u32) -> u64 {
    // SAFETY: an all-zero `rusage_info_v4` is valid output storage for this flavor.
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let read = unsafe { libc::proc_pid_rusage(pid as libc::c_int, libc::RUSAGE_INFO_V4, (&mut info as *mut libc::rusage_info_v4).cast()) };
    assert_eq!(read, 0, "proc_pid_rusage: {}", std::io::Error::last_os_error());
    info.ri_child_elapsed_abstime
}
