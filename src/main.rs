#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod caps;
mod deps;
mod encode;
mod icon;
mod model;
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
    let mut viewport = eframe::egui::ViewportBuilder::default().with_decorations(false).with_min_inner_size(app::MIN_SIZE);
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
    let options = eframe::NativeOptions { viewport, ..Default::default() };
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
