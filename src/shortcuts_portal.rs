//! XDG desktop portal GlobalShortcuts provider
//! (org.freedesktop.portal.GlobalShortcuts, session bus).
//!
//! Serves KDE Plasma, GNOME, and any other Wayland desktop shipping a portal
//! backend with this interface — including Hyprland via
//! xdg-desktop-portal-hyprland. Flow: CreateSession, BindShortcuts with our
//! preferred triggers, then listen for the Activated signal and call `fire`
//! with the shortcut id ("apps" | "windows" | "actions").
//!
//! Threading: all zbus traffic is BLOCKING on two background threads. The
//! control thread owns the session lifecycle (create/bind/close, driven by
//! apply() messages); the listener thread blocks on the Activated signal
//! stream for the life of the provider. Trait-free by design — the daemon
//! talks to `Shortcuts`, never to this type directly.
//!
//! Rebinding caveat: portals cannot unbind individual shortcuts portably, so
//! every apply() closes the session and runs CreateSession + BindShortcuts
//! again. Desktops that gate binding behind a dialog (e.g. KDE) may show it
//! again on each re-apply. Every failure is a log::warn and leaves the
//! provider inert — never a crash.

use crate::config::Settings;
use crate::shortcuts::{combo_for, parse, valid, Combo, FireFn, MODES};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

/// App id we register with the host Registry portal. GlobalShortcuts refuses
/// callers without an app id; reverse-DNS of the repository home.
const APP_ID: &str = "io.github.Osyna.RadiAll";

/// Cheap availability probe for detect_provider(): a property read that only
/// succeeds when the session bus exposes the GlobalShortcuts portal.
pub fn available() -> bool {
    fn probe() -> zbus::Result<u32> {
        let conn = Connection::session()?;
        Proxy::new(&conn, DEST, PATH, IFACE)?.get_property::<u32>("version")
    }
    match probe() {
        Ok(v) => {
            log::debug!("portal: GlobalShortcuts version {v}");
            true
        }
        Err(e) => {
            log::debug!("portal: GlobalShortcuts unavailable: {e}");
            false
        }
    }
}

/// Host (non-sandboxed) apps have no .flatpak-info, so xdg-desktop-portal
/// can't derive an app id — and GlobalShortcuts rejects the flow outright
/// ("An app id is required"). The Registry host portal (xdg-desktop-portal
/// >= 1.18) exists for exactly this and MUST be the first call on the
/// connection. Best-effort: sandboxed apps already carry an id and the
/// portal refuses the call — that's fine.
fn register_host_app(conn: &Connection) {
    let done = Proxy::new(conn, DEST, PATH, "org.freedesktop.host.portal.Registry")
        .and_then(|p| p.call_method("Register", &(APP_ID, HashMap::<&str, Value>::new())));
    if let Err(e) = done {
        log::debug!("portal: host app registration declined: {e}");
    }
}

/// Combo -> portal trigger syntax: "super+shift+r" -> "LOGO+SHIFT+r".
fn trigger(c: &Combo) -> String {
    let mut out = String::new();
    for m in &c.mods {
        let name = match m.as_str() {
            "super" | "meta" => "LOGO",
            "ctrl" | "control" => "CTRL",
            "alt" => "ALT",
            "shift" => "SHIFT",
            other => {
                log::warn!("portal: unknown modifier {other:?} dropped from trigger");
                continue;
            }
        };
        out.push_str(name);
        out.push('+');
    }
    out.push_str(&c.key);
    out
}

/// (mode, trigger) pairs to bind for these settings; empty when the master
/// toggle is off.
fn bindings(settings: &Settings) -> Vec<(&'static str, String)> {
    if !settings.shortcuts_enabled {
        return Vec::new();
    }
    MODES
        .iter()
        .filter(|mode| valid(combo_for(settings, mode)))
        .map(|mode| (*mode, trigger(&parse(combo_for(settings, mode)))))
        .collect()
}

/// Per-process token counter: request handle tokens must be unique.
static TOKEN: AtomicU32 = AtomicU32::new(0);

fn next_token() -> String {
    format!("radiall_{}", TOKEN.fetch_add(1, Ordering::Relaxed))
}

/// Predicted request-object path per the portal spec: unique name ":1.42" +
/// token "t" -> /org/freedesktop/portal/desktop/request/1_42/t.
fn request_path(conn: &Connection, token: &str) -> Result<String, String> {
    let unique = conn.unique_name().ok_or("no unique bus name")?;
    let sender = unique.trim_start_matches(':').replace('.', "_");
    Ok(format!("/org/freedesktop/portal/desktop/request/{sender}/{token}"))
}

type PortalResults = HashMap<String, OwnedValue>;

/// Call a portal request method and wait for its Response signal. Subscribes
/// on the spec-predicted request path first (no signal race), falling back to
/// the server-returned path for ancient (pre-0.9) portals.
fn portal_request<B>(
    conn: &Connection,
    portal: &Proxy<'_>,
    method: &str,
    body: &B,
    token: &str,
) -> Result<PortalResults, String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let predicted = request_path(conn, token)?;
    let request_proxy = |path: &str| {
        Proxy::new(conn, DEST, path.to_owned(), "org.freedesktop.portal.Request")
            .map_err(|e| format!("request proxy: {e}"))
    };
    let proxy = request_proxy(&predicted)?;
    let mut signals = proxy
        .receive_signal("Response")
        .map_err(|e| format!("Response subscribe: {e}"))?;

    let reply = portal
        .call_method(method, body)
        .map_err(|e| format!("{method}: {e}"))?;
    let (handle,): (OwnedObjectPath,) = reply
        .body()
        .deserialize()
        .map_err(|e| format!("{method} reply: {e}"))?;

    // Old portals ignore handle_token and mint their own path. Re-subscribe
    // there; the tiny window where a response could slip by just leaves the
    // provider inert, which is the contract anyway.
    let proxy2;
    if handle.as_str() != predicted {
        proxy2 = request_proxy(handle.as_str())?;
        signals = proxy2
            .receive_signal("Response")
            .map_err(|e| format!("Response re-subscribe: {e}"))?;
    }

    let msg = signals.next().ok_or_else(|| format!("{method}: bus closed"))?;
    let (code, results): (u32, PortalResults) = msg
        .body()
        .deserialize()
        .map_err(|e| format!("{method} response: {e}"))?;
    if code != 0 {
        // 1 = user cancelled, 2 = other error.
        return Err(format!("{method}: portal response code {code}"));
    }
    Ok(results)
}

/// CreateSession + BindShortcuts for `binds`. Returns the session path.
fn bind_session(
    conn: &Connection,
    binds: &[(&'static str, String)],
) -> Result<OwnedObjectPath, String> {
    let portal = Proxy::new(conn, DEST, PATH, IFACE).map_err(|e| format!("portal proxy: {e}"))?;

    let token = next_token();
    let options: HashMap<&str, Value> = HashMap::from([
        ("handle_token", Value::from(token.as_str())),
        ("session_handle_token", Value::from("radiall")),
    ]);
    let results = portal_request(conn, &portal, "CreateSession", &(options,), &token)?;
    let session = results
        .get("session_handle")
        .ok_or("CreateSession: no session_handle in response")?;
    // The spec says string; some backends have shipped it as an object path.
    let session: OwnedObjectPath = String::try_from(session.clone())
        .map_err(|e| format!("session_handle: {e}"))
        .and_then(|s| s.try_into().map_err(|e| format!("session_handle: {e}")))
        .or_else(|_: String| {
            OwnedObjectPath::try_from(session.clone()).map_err(|e| format!("session_handle: {e}"))
        })?;

    let shortcuts: Vec<(&str, HashMap<&str, Value>)> = binds
        .iter()
        .map(|(mode, trig)| {
            (
                *mode,
                HashMap::from([
                    ("description", Value::from(format!("RadiAll {mode} ring"))),
                    ("preferred_trigger", Value::from(trig.as_str())),
                ]),
            )
        })
        .collect();
    let token = next_token();
    let options: HashMap<&str, Value> = HashMap::from([("handle_token", Value::from(token.as_str()))]);
    portal_request(
        conn,
        &portal,
        "BindShortcuts",
        &(&session, shortcuts, "", options),
        &token,
    )?;
    Ok(session)
}

/// Close the portal session object; best-effort.
fn close_session(conn: &Connection, path: &OwnedObjectPath) {
    let closed = Proxy::new(conn, DEST, path.as_str(), "org.freedesktop.portal.Session")
        .and_then(|p| p.call_method("Close", &()));
    if let Err(e) = closed {
        log::warn!("portal: closing session {path} failed: {e}");
    }
}

/// One full rebind round: drop the old session, then CreateSession +
/// BindShortcuts when anything is enabled. None = inert (already warned).
fn rebind(
    conn: &Connection,
    old: Option<OwnedObjectPath>,
    binds: &[(&'static str, String)],
) -> Option<OwnedObjectPath> {
    if let Some(path) = old {
        close_session(conn, &path);
    }
    if binds.is_empty() {
        return None;
    }
    match bind_session(conn, binds) {
        Ok(path) => {
            log::debug!("portal: bound {} shortcut(s) on session {path}", binds.len());
            Some(path)
        }
        Err(e) => {
            log::warn!("portal: shortcut binding failed, shortcuts inert: {e}");
            None
        }
    }
}

pub struct PortalProvider {
    apply_tx: Sender<Vec<(&'static str, String)>>,
}

impl PortalProvider {
    /// Connect to the session bus and spawn the two provider threads. None
    /// only when the bus itself is unreachable; binding failures happen on
    /// the control thread later and merely leave the provider inert.
    pub fn start(settings: &Settings, fire: FireFn) -> Option<PortalProvider> {
        let conn = match Connection::session() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("portal: session bus unavailable: {e}");
                return None;
            }
        };
        register_host_app(&conn); // must precede any other call on this conn
        // Current session path, so the listener drops Activated signals from
        // a session we already closed.
        let session: Arc<Mutex<Option<OwnedObjectPath>>> = Arc::new(Mutex::new(None));
        let (apply_tx, apply_rx) = channel::<Vec<(&'static str, String)>>();

        // Listener: blocks on the Activated stream for the provider's life.
        // The subscription is on the interface, so it survives rebinds.
        {
            let conn = conn.clone();
            let session = Arc::clone(&session);
            let spawned = std::thread::Builder::new()
                .name("portal-activated".into())
                .spawn(move || {
                    let signals = Proxy::new(&conn, DEST, PATH, IFACE)
                        .and_then(|p| p.receive_signal("Activated"));
                    let signals = match signals {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!("portal: Activated subscribe failed, shortcuts inert: {e}");
                            return;
                        }
                    };
                    for msg in signals {
                        let parsed: zbus::Result<(OwnedObjectPath, String, u64, PortalResults)> =
                            msg.body().deserialize().map_err(Into::into);
                        let Ok((sig_session, id, _ts, _opts)) = parsed else {
                            log::warn!("portal: malformed Activated signal ignored");
                            continue;
                        };
                        let current = session.lock().unwrap_or_else(|p| p.into_inner());
                        if current.as_ref() != Some(&sig_session) {
                            continue; // stale or foreign session
                        }
                        drop(current);
                        match MODES.iter().find(|m| **m == id) {
                            Some(mode) => fire(mode),
                            None => log::debug!("portal: unknown shortcut id {id:?} ignored"),
                        }
                    }
                    log::warn!("portal: Activated stream ended; shortcuts inert");
                });
            if let Err(e) = spawned {
                log::warn!("portal: failed to spawn listener thread: {e}");
                return None;
            }
        }

        // Control: initial bind, then one rebind per apply() message.
        {
            let initial = bindings(settings);
            let session = Arc::clone(&session);
            let spawned = std::thread::Builder::new()
                .name("portal-shortcuts".into())
                .spawn(move || {
                    let mut current = rebind(&conn, None, &initial);
                    *session.lock().unwrap_or_else(|p| p.into_inner()) = current.clone();
                    while let Ok(binds) = apply_rx.recv() {
                        current = rebind(&conn, current.take(), &binds);
                        *session.lock().unwrap_or_else(|p| p.into_inner()) = current.clone();
                    }
                });
            if let Err(e) = spawned {
                log::warn!("portal: failed to spawn control thread: {e}");
                return None;
            }
        }

        Some(PortalProvider { apply_tx })
    }

    /// Re-sync: recreate the session with the combos now in settings.
    pub fn apply(&self, settings: &Settings) {
        if self.apply_tx.send(bindings(settings)).is_err() {
            log::warn!("portal: control thread gone; shortcut apply dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_mapping() {
        assert_eq!(trigger(&parse("super+a")), "LOGO+a");
        assert_eq!(trigger(&parse("ctrl+shift+r")), "CTRL+SHIFT+r");
        assert_eq!(trigger(&parse("meta+alt+F5")), "LOGO+ALT+F5");
        assert_eq!(trigger(&parse("control+space")), "CTRL+space");
        assert_eq!(trigger(&parse("f11")), "f11");
        // unknown modifiers are dropped, not passed through
        assert_eq!(trigger(&parse("hyper+x")), "x");
    }

    #[test]
    fn bindings_respect_master_toggle_and_validity() {
        let mut settings = Settings::default();
        settings.shortcuts.apps = "super+a".into();
        settings.shortcuts.windows = "ctrl+shift".into(); // invalid: lone mods
        settings.shortcuts.actions = "super+d".into();
        let binds = bindings(&settings);
        assert_eq!(
            binds,
            vec![("apps", "LOGO+a".to_owned()), ("actions", "LOGO+d".to_owned())]
        );
        settings.shortcuts_enabled = false;
        assert!(bindings(&settings).is_empty());
    }

    /// Live probe against THIS session's portal (xdg-desktop-portal-hyprland
    /// on Hyprland). Run manually:
    ///   cargo test shortcuts_portal -- --ignored --nocapture
    ///
    /// Host prerequisite: an installed `io.github.Osyna.RadiAll.desktop`
    /// whose Exec resolves, or the Registry call is declined and the portal
    /// rejects CreateSession with "An app id is required" (the provider then
    /// warns and stays inert — the daemon never crashes).
    ///
    /// While a session is open, `hyprctl globalshortcuts` lists the three
    /// ids; the check below is best-effort so the test also passes on
    /// non-Hyprland portals.
    #[test]
    #[ignore = "talks to the real session bus portal"]
    fn live_create_and_bind() {
        fn assert_listed() {
            let Ok(out) = std::process::Command::new("hyprctl").arg("globalshortcuts").output()
            else {
                return; // not Hyprland; the portal calls succeeding is enough
            };
            let listing = String::from_utf8_lossy(&out.stdout);
            println!("hyprctl globalshortcuts:\n{listing}");
            for id in ["apps", "windows", "actions"] {
                assert!(listing.contains(id), "{id} missing from hyprctl globalshortcuts");
            }
        }

        // Raw flow first: failures surface as Err instead of a warn log.
        let conn = Connection::session().expect("session bus");
        register_host_app(&conn);
        assert!(available(), "GlobalShortcuts portal not on the bus");
        let binds = vec![
            ("apps", "LOGO+a".to_owned()),
            ("windows", "LOGO+w".to_owned()),
            ("actions", "LOGO+d".to_owned()),
        ];
        let session = bind_session(&conn, &binds).expect("CreateSession+BindShortcuts");
        println!("portal session: {session}");
        assert_listed();
        close_session(&conn, &session);

        // Full provider path, as the daemon runs it: forced kind, dummy fire.
        std::env::set_var("RADIALL_SHORTCUTS", "portal");
        assert_eq!(
            crate::shortcuts::detect_provider(),
            crate::shortcuts::ProviderKind::Portal
        );
        let settings = Settings::default(); // super+a / super+w / super+d
        let provider = PortalProvider::start(&settings, Arc::new(|mode| println!("fired {mode}")))
            .expect("provider start");
        std::thread::sleep(std::time::Duration::from_secs(2)); // control thread binds
        assert_listed();
        provider.apply(&settings); // rebind round-trip (close + recreate)
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert_listed();
    }
}
