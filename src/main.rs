//! RadiAll — a radial app launcher, window switcher, and per-window action
//! menu. Standalone Rust + Slint build: one binary is both the daemon
//! (`radiall --daemon`, spawned by `--start`) and the control CLI that pokes
//! it over a unix socket (`radiall --apps` …), so any compositor or DE can
//! bind keys to it.

mod apps;
mod compositor;
mod config;
mod icons;
mod ipc;
mod ring;
mod shortcuts;
mod shortcuts_portal;
mod shortcuts_x11;
mod theme;
mod ui;

slint::include_modules!();

use ring::Mode;
use std::cell::RefCell;
use std::rc::Rc;

const BUNDLED_THEMES: [(&str, &str); 11] = [
    ("default", include_str!("../themes/default.json")),
    ("radiall", include_str!("../themes/radiall.json")),
    ("catppuccin", include_str!("../themes/catppuccin.json")),
    ("light", include_str!("../themes/light.json")),
    ("nord", include_str!("../themes/nord.json")),
    ("paper", include_str!("../themes/paper.json")),
    ("dracula", include_str!("../themes/dracula.json")),
    ("gruvbox", include_str!("../themes/gruvbox.json")),
    ("tokyo-night", include_str!("../themes/tokyo-night.json")),
    ("rose-pine", include_str!("../themes/rose-pine.json")),
    ("matrix", include_str!("../themes/matrix.json")),
];

thread_local! {
    static UI: RefCell<Option<Rc<ui::Ui>>> = const { RefCell::new(None) };
}

fn with_ui(f: impl FnOnce(&Rc<ui::Ui>)) {
    UI.with(|u| {
        if let Some(ui) = &*u.borrow() {
            f(ui);
        }
    });
}

fn dispatch(cmd: ipc::Command) {
    let _ = slint::invoke_from_event_loop(move || match cmd {
        ipc::Command::Apps => with_ui(|ui| ui.toggle(Mode::Apps)),
        ipc::Command::Windows => with_ui(|ui| ui.toggle(Mode::Windows)),
        ipc::Command::Actions => with_ui(|ui| ui.toggle(Mode::Actions)),
        ipc::Command::Settings => with_ui(|ui| ui.open_settings()),
        ipc::Command::Quit => {
            let _ = slint::quit_event_loop();
        }
    });
}

fn run_daemon() -> Result<(), Box<dyn std::error::Error>> {
    // fail fast when another daemon owns the socket
    let listener = ipc::bind()?;

    // initialize the Slint backend up front: everything below queues work
    // onto the event loop before any window exists.
    // RADIALL_RENDERER=software swaps the GPU renderer for CPU rendering:
    // slower frames, but no GL/EGL driver stack in memory (tens of MB on
    // some drivers) — for RAM-tight or GPU-less setups. Default: femtovg.
    let selector = slint::BackendSelector::new();
    let selector = match std::env::var("RADIALL_RENDERER").ok().as_deref() {
        Some(name) if !name.is_empty() => {
            log::info!("renderer override: {name}");
            selector.renderer_name(name.into())
        }
        _ => selector,
    };
    selector.select()?;

    config::seed_themes(&BUNDLED_THEMES);
    let settings = config::load_settings();
    let app_list = config::load_apps();
    let index = apps::AppIndex::scan();

    // Pre-warm the icon cache off-thread: the settings editor decodes one
    // 32px icon per installed app (SVGs are costly), and the ring needs the
    // configured apps' icons at wheel size. First open stays snappy.
    {
        let installed: Vec<String> = index.installed().iter().map(|d| d.icon.clone()).collect();
        let ring_icons: Vec<String> = app_list.iter().map(|a| a.icon.clone()).collect();
        let px = (settings.icon_size * 1.1).round() as u32; // approx wheel size
        std::thread::spawn(move || {
            for icon in ring_icons {
                apps::load_icon_pixels(&icon, px.max(32));
            }
            for icon in installed {
                apps::load_icon_pixels(&icon, 32);
            }
        });
    }

    let mut comp = compositor::detect();
    log::info!("compositor backend: {}", comp.backend());

    // Global shortcuts: hyprctl binds exec the CLI; portal/X11 providers fire
    // in-process through the same dispatch the socket and tray use.
    let sc = shortcuts::Shortcuts::start(shortcuts::detect_provider(), &settings, |mode| {
        if let Some(cmd) = ipc::Command::parse(mode) {
            dispatch(cmd);
        }
    });
    let overlay_ready = comp.setup_overlay();

    // compositor events -> UI thread
    let (tx, rx) = std::sync::mpsc::channel();
    comp.watch(tx);
    std::thread::spawn(move || {
        for ev in rx {
            let _ = slint::invoke_from_event_loop(move || match ev {
                compositor::CompositorEvent::Windows(ws) => {
                    with_ui(|ui| ui.on_windows_changed(ws.clone()))
                }
                compositor::CompositorEvent::Active(a) => {
                    with_ui(|ui| ui.on_active_changed(a.clone()))
                }
                compositor::CompositorEvent::ConfigReloaded => with_ui(|ui| {
                    let core = ui.core.borrow();
                    if let Some(sc) = &core.shortcuts {
                        sc.apply(&core.settings);
                    }
                }),
            });
        }
    });

    // Window components MUST be created from inside the running event loop:
    // with the winit backend, a window constructed before the loop starts and
    // first shown afterwards never gets its native window mapped. Queue the
    // whole UI construction; it runs as the loop's first user event.
    slint::invoke_from_event_loop(move || {
        let mut core = ring::Core::new(settings, app_list, index, comp);
        core.overlay_ready = overlay_ready;
        core.shortcuts = Some(sc);
        let ui = match ui::Ui::new(core) {
            Ok(ui) => ui,
            Err(e) => {
                log::error!("UI init failed: {e}");
                let _ = slint::quit_event_loop();
                return;
            }
        };
        UI.with(|u| *u.borrow_mut() = Some(ui));

        // tray (optional: fails harmlessly without a StatusNotifier host)
        if std::env::var_os("RADIALL_NO_TRAY").is_none() {
            match Tray::new() {
                Ok(tray) => {
                    tray.on_command(|cmd| {
                        if let Some(c) = ipc::Command::parse(&cmd) {
                            dispatch(c);
                        }
                    });
                    tray.show().ok();
                    UI.with(|u| {
                        if let Some(ui) = &*u.borrow() {
                            ui.keep_tray(tray);
                        }
                    });
                }
                Err(e) => log::info!("no tray: {e}"),
            }
        }
    })?;

    // control socket
    ipc::serve(listener, dispatch);

    slint::run_event_loop_until_quit()?;

    // drop the socket file on clean exit
    std::fs::remove_file(ipc::socket_path()).ok();
    Ok(())
}

fn start_detached() {
    if ipc::ping() {
        println!("radiall: already running");
        return;
    }
    let exe = std::env::current_exe().expect("current_exe");
    use std::os::unix::process::CommandExt;
    let spawned = std::process::Command::new(exe)
        .arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    match spawned {
        Ok(_) => println!("radiall: started"),
        Err(e) => {
            eprintln!("radiall: failed to start — {e}");
            std::process::exit(1);
        }
    }
}

fn send_or_die(cmd: ipc::Command) {
    if let Err(e) = ipc::send(cmd) {
        eprintln!("radiall: {e}");
        std::process::exit(1);
    }
}

fn print_binds() {
    print!(
        "# RadiAll ring shortcuts — paste into ~/.config/hypr/hyprland.conf (or a sourced file).\n\
         # Change the keys to taste; each just opens one ring.\n\
         bind = SUPER, A, exec, radiall --apps       # app ring\n\
         bind = SUPER, W, exec, radiall --windows    # open-windows ring\n\
         bind = SUPER, D, exec, radiall --actions    # focused-window actions ring\n\
         # optional: open settings\n\
         # bind = SUPER, S, exec, radiall --settings\n"
    );
}

fn print_help() {
    print!(
        "RadiAll — a cute radial launcher. Standalone build (Rust + Slint).\n\n\
         Usage: radiall <command>\n\n\
         Rings (bind these to keys in your compositor / DE):\n\
           --apps        open the app ring\n\
           --windows     open the open-windows ring\n\
           --actions     open the focused-window actions ring\n\
           --settings    open the settings panel\n\n\
         Lifecycle:\n\
           --start       start the launcher daemon\n\
           --daemon      run the daemon in the foreground\n\
           --stop        stop the running launcher\n\
           --restart     restart it\n\
           --status      is it running?  (exit 0 = yes, 1 = no)\n\n\
         Setup:\n\
           --binds       print ready-to-paste Hyprland bind lines\n\
           --version     print version\n\
           --help, -h    show this help\n\n\
         Bind example (~/.config/hypr/hyprland.conf):\n\
           bind = SUPER, A, exec, radiall --apps\n\n\
         GNOME: Settings → Keyboard → Custom Shortcuts → command `radiall --apps`.\n\
         KDE:   System Settings → Shortcuts → Add Command.\n"
    );
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let arg = std::env::args().nth(1).unwrap_or_default();
    match arg.trim_start_matches('-') {
        "apps" => send_or_die(ipc::Command::Apps),
        "windows" => send_or_die(ipc::Command::Windows),
        "actions" => send_or_die(ipc::Command::Actions),
        // The desktop-entry entry point: humans click this with no daemon
        // running (fresh login, crashed daemon), so heal instead of dying.
        "settings" => {
            if !ipc::ping() {
                start_detached();
                for _ in 0..30 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if ipc::ping() {
                        break;
                    }
                }
            }
            send_or_die(ipc::Command::Settings);
        }
        "daemon" => {
            if let Err(e) = run_daemon() {
                eprintln!("radiall: {e}");
                std::process::exit(1);
            }
        }
        "start" => start_detached(),
        "stop" => {
            if ipc::ping() {
                send_or_die(ipc::Command::Quit);
                println!("radiall: stopped");
            } else {
                println!("radiall: not running");
            }
        }
        "restart" => {
            if ipc::ping() {
                let _ = ipc::send(ipc::Command::Quit);
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
            start_detached();
        }
        "status" => {
            if ipc::ping() {
                println!("radiall: running");
            } else {
                println!("radiall: not running");
                std::process::exit(1);
            }
        }
        "binds" => print_binds(),
        "version" | "v" => println!("RadiAll {}", env!("CARGO_PKG_VERSION")),
        "help" | "h" | "" => print_help(),
        other => {
            eprintln!("radiall: unknown command '{other}'\ntry: radiall --help");
            std::process::exit(1);
        }
    }
}
