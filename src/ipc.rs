//! Daemon control channel. The `radiall` CLI (and the tray menu) reach the
//! running daemon over a local IPC endpoint — a Unix domain socket on Linux,
//! a named pipe on Windows — so binding a key to `radiall --apps` works on
//! any desktop, no matter which process fires the command.
//!
//! Protocol (identical on both transports): one ASCII line per request
//! ("apps", "windows", "actions", "settings", "ping", "quit"), one line reply
//! ("ok" / "unknown").

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Apps,
    Windows,
    Actions,
    Settings,
    Quit,
}

impl Command {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "apps" => Some(Self::Apps),
            "windows" => Some(Self::Windows),
            "actions" => Some(Self::Actions),
            "settings" => Some(Self::Settings),
            "quit" => Some(Self::Quit),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apps => "apps",
            Self::Windows => "windows",
            Self::Actions => "actions",
            Self::Settings => "settings",
            Self::Quit => "quit",
        }
    }
}

/// Map one request line to its reply, dispatching real commands. Shared by
/// both transports so the wire protocol can never drift between them.
fn handle_line(line: &str, on_command: &impl Fn(Command)) -> &'static [u8] {
    if line.trim() == "ping" {
        return b"ok\n";
    }
    match Command::parse(line) {
        Some(cmd) => {
            on_command(cmd);
            b"ok\n"
        }
        None => b"unknown\n",
    }
}

/// True if a live daemon answers on the endpoint.
pub fn ping() -> bool {
    matches!(platform::request("ping").as_deref(), Ok("ok"))
}

/// Send a ring/settings command to the running daemon.
pub fn send(cmd: Command) -> Result<(), String> {
    match platform::request(cmd.as_str()) {
        Ok(r) if r == "ok" => Ok(()),
        Ok(r) => Err(format!("daemon replied: {r}")),
        Err(e) => Err(format!(
            "couldn't reach the launcher ({e}). Is it running?  (radiall --start)"
        )),
    }
}

pub use platform::{bind, serve};
#[cfg(unix)]
pub use platform::socket_path;

// ======================= Unix: domain socket =======================
#[cfg(unix)]
mod platform {
    use super::{handle_line, Command};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::time::Duration;

    pub type Listener = UnixListener;

    pub fn socket_path() -> PathBuf {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        dir.join("radiall.sock")
    }

    /// Bind the control socket, replacing a stale one if the old daemon is
    /// gone. Err if another live daemon already owns it.
    pub fn bind() -> Result<Listener, String> {
        let path = socket_path();
        match UnixListener::bind(&path) {
            Ok(l) => Ok(l),
            Err(_) => {
                if super::ping() {
                    return Err(format!("another radiall daemon is running ({})", path.display()));
                }
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
                UnixListener::bind(&path).map_err(|e| e.to_string())
            }
        }
    }

    /// Serve forever on a background thread, invoking `on_command` per request.
    pub fn serve(listener: Listener, on_command: impl Fn(Command) + Send + 'static) {
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                handle_client(stream, &on_command);
            }
        });
    }

    fn handle_client(stream: UnixStream, on_command: &impl Fn(Command)) {
        stream.set_read_timeout(Some(Duration::from_millis(500))).ok();
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let mut out = &stream;
        out.write_all(handle_line(&line, on_command)).ok();
    }

    pub(super) fn request(line: &str) -> Result<String, std::io::Error> {
        let mut stream = UnixStream::connect(socket_path())?;
        stream.set_read_timeout(Some(Duration::from_millis(1500)))?;
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply)?;
        Ok(reply.trim().to_owned())
    }
}

// ======================= Windows: named pipe =======================
#[cfg(windows)]
mod platform {
    use super::{handle_line, Command};
    use std::io;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW,
    };

    // Win32 flag literals (kept local so we don't depend on which windows-sys
    // module re-exports each constant).
    const GENERIC_RW: u32 = 0x8000_0000 | 0x4000_0000; // GENERIC_READ | GENERIC_WRITE
    const OPEN_EXISTING: u32 = 3;
    const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    const ERROR_PIPE_BUSY: u32 = 231;
    const ERROR_PIPE_CONNECTED: u32 = 535;

    /// A named pipe has no persistent listener object: `serve` creates a fresh
    /// pipe instance per client. This marker just carries the "we won the
    /// start race" result of `bind` so the call sites mirror the Unix path.
    pub struct Listener;

    /// Per-user pipe name so two logged-in users never collide.
    fn pipe_name() -> Vec<u16> {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
        let user: String = user.chars().filter(|c| *c != '\\').collect();
        format!(r"\\.\pipe\radiall.{user}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Refuse when a live daemon already answers; otherwise clear to serve.
    pub fn bind() -> Result<Listener, String> {
        if super::ping() {
            return Err("another radiall daemon is running".into());
        }
        Ok(Listener)
    }

    /// One synchronous pipe server thread: create instance, wait for a client,
    /// handle its single request, tear down, repeat. Commands are rare and
    /// human-driven, so serializing clients is fine.
    pub fn serve(_listener: Listener, on_command: impl Fn(Command) + Send + 'static) {
        let name = pipe_name();
        std::thread::spawn(move || loop {
            let h = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE, // byte read+write, blocking
                    PIPE_UNLIMITED_INSTANCES,
                    512,
                    512,
                    0,
                    null(),
                )
            };
            if h == INVALID_HANDLE_VALUE {
                log::error!("ipc: CreateNamedPipeW failed ({})", unsafe { GetLastError() });
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
            let connected =
                unsafe { ConnectNamedPipe(h, null_mut()) } != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            if connected {
                handle_client(h, &on_command);
            }
            unsafe {
                DisconnectNamedPipe(h);
                CloseHandle(h);
            }
        });
    }

    fn handle_client(h: HANDLE, on_command: &impl Fn(Command)) {
        let line = match read_line(h) {
            Some(l) => l,
            None => return,
        };
        let reply = handle_line(&line, on_command);
        let mut written = 0u32;
        unsafe {
            WriteFile(h, reply.as_ptr(), reply.len() as u32, &mut written, null_mut());
            // Block until the client has drained the reply before we disconnect.
            FlushFileBuffers(h);
        }
    }

    /// Read one newline-terminated request/reply line from a pipe handle.
    fn read_line(h: HANDLE) -> Option<String> {
        let mut acc: Vec<u8> = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, &mut read, null_mut())
            };
            if ok == 0 || read == 0 {
                break;
            }
            acc.extend_from_slice(&buf[..read as usize]);
            if acc.contains(&b'\n') || acc.len() > 4096 {
                break;
            }
        }
        if acc.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&acc).into_owned())
        }
    }

    fn open_client(name: &[u16]) -> Result<HANDLE, io::Error> {
        for _ in 0..10 {
            let h = unsafe {
                CreateFileW(name.as_ptr(), GENERIC_RW, 0, null(), OPEN_EXISTING, 0, 0 as HANDLE)
            };
            if h != INVALID_HANDLE_VALUE {
                return Ok(h);
            }
            let err = unsafe { GetLastError() };
            if err == ERROR_PIPE_BUSY {
                unsafe { WaitNamedPipeW(name.as_ptr(), 200) };
                continue;
            }
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "pipe busy"))
    }

    pub(super) fn request(line: &str) -> Result<String, io::Error> {
        let name = pipe_name();
        let h = open_client(&name)?;
        let msg = format!("{line}\n");
        let mut written = 0u32;
        let wrote = unsafe {
            WriteFile(h, msg.as_ptr(), msg.len() as u32, &mut written, null_mut())
        };
        if wrote == 0 {
            let e = io::Error::last_os_error();
            unsafe { CloseHandle(h) };
            return Err(e);
        }
        let reply = read_line(h).unwrap_or_default();
        unsafe { CloseHandle(h) };
        Ok(reply.trim().to_owned())
    }
}
