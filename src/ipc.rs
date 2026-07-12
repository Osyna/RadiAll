//! Daemon control socket. The `radiall` CLI (and the tray menu) reach the
//! running daemon over a unix socket — compositor-agnostic, works everywhere
//! a user can bind a key to a shell command.
//!
//! Protocol: one ASCII line per request ("apps", "windows", "actions",
//! "settings", "ping", "quit"), one line reply ("ok" / "unknown").

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

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

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("radiall.sock")
}

/// Bind the control socket, replacing a stale one if the old daemon is gone.
/// Returns Err if another live daemon already owns the socket.
pub fn bind() -> Result<UnixListener, String> {
    let path = socket_path();
    match UnixListener::bind(&path) {
        Ok(l) => Ok(l),
        Err(_) => {
            // Socket file exists: probe it. Live daemon -> refuse; stale -> replace.
            if ping() {
                return Err(format!("another radiall daemon is running ({})", path.display()));
            }
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            UnixListener::bind(&path).map_err(|e| e.to_string())
        }
    }
}

/// Serve forever, invoking `on_command` (from the accept thread) per request.
pub fn serve(listener: UnixListener, on_command: impl Fn(Command) + Send + 'static) {
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
    if line.trim() == "ping" {
        out.write_all(b"ok\n").ok();
        return;
    }
    match Command::parse(&line) {
        Some(cmd) => {
            on_command(cmd);
            out.write_all(b"ok\n").ok();
        }
        None => {
            out.write_all(b"unknown\n").ok();
        }
    }
}

fn request(line: &str) -> Result<String, std::io::Error> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(Duration::from_millis(1500)))?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    Ok(reply.trim().to_owned())
}

/// True if a live daemon answers on the socket.
pub fn ping() -> bool {
    matches!(request("ping").as_deref(), Ok("ok"))
}

/// Send a ring/settings command to the running daemon.
pub fn send(cmd: Command) -> Result<(), String> {
    match request(cmd.as_str()) {
        Ok(r) if r == "ok" => Ok(()),
        Ok(r) => Err(format!("daemon replied: {r}")),
        Err(e) => Err(format!(
            "couldn't reach the launcher ({e}). Is it running?  (radiall --start)"
        )),
    }
}
