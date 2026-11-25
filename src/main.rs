#![cfg_attr(windows, windows_subsystem = "windows")]

mod after;
mod app;
mod caps;
mod deps;
mod encode;
mod filters;
mod icon;
mod model;
mod platform;
mod presets;
mod probe;
mod queue;
mod settings;
mod strings;
mod ui;
mod view;

fn main() -> eframe::Result {
    let set = settings::Settings::load();
    // Files given on the command line, as when Windows opens them with this program.
    let open: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(std::path::PathBuf::from).filter(|p| p.is_file()).collect();
    let mut size = app::START_SIZE;
    let mut viewport =
        eframe::egui::ViewportBuilder::default().with_decorations(false).with_app_id("stiva").with_min_inner_size(app::MIN_SIZE);
    if let Some(g) = &set.window {
        size = [g.size[0].max(app::MIN_SIZE[0]), g.size[1].max(app::MIN_SIZE[1])];
        if let Some(p) = g.pos.filter(|p| p[0] > -10000.0 && p[1] > -10000.0 && p[0] < 20000.0 && p[1] < 20000.0) {
            viewport = viewport.with_position(p);
        }
        viewport = viewport.with_maximized(g.maximized);
    }
    viewport = viewport.with_inner_size(size);
    if let Some((rgba, width, height)) = icon::rgba() {
        viewport = viewport.with_icon(eframe::egui::IconData { rgba, width, height });
    }
    // Dropped files reach the window only through X11, so on a Wayland desktop that has XWayland go that way
    // first, and fall back to native Wayland if the X11 window cannot be opened.
    #[cfg(not(windows))]
    if platform::use_x11() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let options = eframe::NativeOptions {
            viewport: viewport.clone(),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_x11();
            })),
            ..Default::default()
        };
        match launch(options, set.clone(), open.clone()) {
            Err(eframe::Error::WinitEventLoop(e)) => {
                eprintln!("Stiva: X11 did not start ({e}), using Wayland");
                // A process gets one try at an event loop, so Wayland needs a fresh start.
                use std::os::unix::process::CommandExt;
                if let Ok(exe) = std::env::current_exe() {
                    let err = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("STIVA_WAYLAND", "1").exec();
                    eprintln!("Stiva: could not start again: {err}");
                }
                return Err(eframe::Error::WinitEventLoop(e));
            }
            done => return done,
        }
    }
    launch(eframe::NativeOptions { viewport, ..Default::default() }, set, open)
}

fn launch(options: eframe::NativeOptions, set: settings::Settings, open: Vec<std::path::PathBuf>) -> eframe::Result {
    eframe::run_native(
        strings::APP_NAME,
        options,
        Box::new(move |cc| {
            ui::fonts(&cc.egui_ctx);
            ui::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc.egui_ctx.clone(), set, open)))
        }),
    )
}
