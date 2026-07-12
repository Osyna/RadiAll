//! Hyprland IPC adapter (full capabilities).
//!
//! Talks to Hyprland's two unix sockets directly (no hyprland crate):
//!   - `.socket.sock`  — request/response: write command bytes, read to EOF.
//!   - `.socket2.sock` — event stream: newline-delimited `EVENT>>DATA` lines.

use super::{Compositor, CompositorEvent, WindowId, WindowInfo};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

pub struct HyprlandCompositor {
    req_path: PathBuf,
    event_path: PathBuf,
}

impl HyprlandCompositor {
    pub fn connect() -> Result<Self, String> {
        let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .map_err(|_| "HYPRLAND_INSTANCE_SIGNATURE not set".to_string())?;
        let runtime = std::env::var("XDG_RUNTIME_DIR")
            .map_err(|_| "XDG_RUNTIME_DIR not set".to_string())?;
        let dir = PathBuf::from(runtime).join("hypr").join(sig);
        let req_path = dir.join(".socket.sock");
        let event_path = dir.join(".socket2.sock");
        if !req_path.exists() {
            return Err(format!("Hyprland socket missing: {}", req_path.display()));
        }
        Ok(Self { req_path, event_path })
    }

    fn dispatch(&self, cmd: &str) {
        match request(&self.req_path, &format!("dispatch {cmd}")) {
            Ok(reply) if reply.trim() == "ok" => {}
            Ok(reply) => log::warn!("hyprland: dispatch {cmd} -> {reply}"),
            Err(e) => log::warn!("hyprland: dispatch {cmd} failed: {e}"),
        }
    }
}

impl Compositor for HyprlandCompositor {
    fn backend(&self) -> &'static str {
        "hyprland"
    }
    fn can_float(&self) -> bool {
        true
    }
    fn can_send_keys(&self) -> bool {
        true
    }
    fn can_manage_keybinds(&self) -> bool {
        true
    }

    fn windows(&mut self) -> Vec<WindowInfo> {
        query_windows(&self.req_path)
    }

    fn active_window(&mut self) -> Option<WindowInfo> {
        query_active(&self.req_path)
    }

    fn watch(&mut self, tx: Sender<CompositorEvent>) {
        let req_path = self.req_path.clone();
        let event_path = self.event_path.clone();
        std::thread::Builder::new()
            .name("hypr-events".into())
            .spawn(move || loop {
                let stream = match UnixStream::connect(&event_path) {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("hyprland: event socket connect failed: {e}");
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                };
                for line in BufReader::new(stream).lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break, // socket dropped -> reconnect
                    };
                    let event = line.split(">>").next().unwrap_or("");
                    let sent = match event {
                        "openwindow" | "closewindow" | "movewindow" | "windowtitle"
                        | "windowtitlev2" | "changefloatingmode" | "fullscreen" => {
                            tx.send(CompositorEvent::Windows(query_windows(&req_path)))
                        }
                        "activewindow" | "activewindowv2" => {
                            tx.send(CompositorEvent::Active(query_active(&req_path)))
                        }
                        _ => continue,
                    };
                    if sent.is_err() {
                        return; // receiver gone -> exit silently
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            })
            .expect("spawn hypr-events thread");
    }

    fn activate(&mut self, id: &WindowId) {
        self.dispatch(&format!("focuswindow address:{id}"));
    }
    fn close_window(&mut self, id: &WindowId) {
        self.dispatch(&format!("closewindow address:{id}"));
    }
    fn fullscreen(&mut self, id: &WindowId) {
        self.dispatch(&format!("focuswindow address:{id}"));
        self.dispatch("fullscreen 1");
    }
    fn toggle_float(&mut self, id: &WindowId) {
        self.dispatch(&format!("togglefloating address:{id}"));
    }
    fn send_keys(&mut self, id: &WindowId, mods: &str, key: &str) {
        self.dispatch(&format!("sendshortcut {mods}, {key}, address:{id}"));
    }
}

/// One request round-trip: connect, write, read to EOF.
fn request(path: &PathBuf, cmd: &str) -> Result<String, String> {
    let mut stream = UnixStream::connect(path).map_err(|e| e.to_string())?;
    stream.write_all(cmd.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn query_json(path: &PathBuf, cmd: &str) -> Option<Value> {
    let raw = match request(path, cmd) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("hyprland: {cmd} failed: {e}");
            return None;
        }
    };
    match serde_json::from_str(&raw) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("hyprland: {cmd} returned invalid JSON: {e}");
            None
        }
    }
}

/// Map one j/clients entry. None = unusable (unmapped or no class at all).
fn window_info(client: &Value, active_addr: Option<&str>) -> Option<WindowInfo> {
    if !client["mapped"].as_bool().unwrap_or(false) {
        return None;
    }
    let address = client["address"].as_str().unwrap_or("");
    let mut app_id = client["class"].as_str().unwrap_or("");
    if app_id.is_empty() {
        app_id = client["initialClass"].as_str().unwrap_or("");
    }
    if app_id.is_empty() || address.is_empty() {
        return None;
    }
    Some(WindowInfo {
        id: address.to_string(),
        app_id: app_id.to_string(),
        title: client["title"].as_str().unwrap_or("").to_string(),
        focused: active_addr == Some(address),
    })
}

fn query_windows(path: &PathBuf) -> Vec<WindowInfo> {
    let active_addr = query_json(path, "j/activewindow")
        .and_then(|v| v["address"].as_str().map(str::to_string));
    let Some(Value::Array(clients)) = query_json(path, "j/clients") else {
        return Vec::new();
    };
    clients
        .iter()
        .filter_map(|c| window_info(c, active_addr.as_deref()))
        .collect()
}

fn query_active(path: &PathBuf) -> Option<WindowInfo> {
    let v = query_json(path, "j/activewindow")?;
    let addr = v["address"].as_str()?.to_string();
    window_info(&v, Some(&addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live smoke test — runs only inside a Hyprland session.
    #[test]
    fn live_smoke() {
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
            return; // not on Hyprland; skip silently
        }
        let mut c = HyprlandCompositor::connect().expect("connect to Hyprland IPC");
        let windows = c.windows();
        println!("--- {} windows ---", windows.len());
        for w in &windows {
            println!(
                "{} {} [{}]{}",
                w.id,
                w.app_id,
                w.title,
                if w.focused { " *focused*" } else { "" }
            );
        }
        assert!(!windows.is_empty(), "expected at least one mapped window");
        let active = c.active_window();
        println!("active: {active:?}");
    }
}
