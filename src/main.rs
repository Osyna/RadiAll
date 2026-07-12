#[allow(dead_code)]
mod apps;
#[allow(dead_code)]
mod compositor;
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod ipc;
#[allow(dead_code)]
mod theme;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    env_logger::init();
    let win = RingWindow::new()?;
    win.window().set_fullscreen(true);
    win.set_open(true);
    let weak = win.as_weak();
    win.on_esc_pressed(move || {
        if let Some(w) = weak.upgrade() {
            w.hide().ok();
        }
    });
    win.run()
}
