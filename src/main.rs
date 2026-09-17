#![windows_subsystem = "windows"]

mod app;
mod ui_text;
mod app_state;
mod backends;
mod config;
mod es_dash;
mod es_view;
mod icons;
mod kind;
mod models;
mod visual;
mod worker;

use app::DblensApp;

fn load_app_icon() -> egui::IconData {
    let bytes = include_bytes!("../assets/dblens.png");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            }
        }
        Err(_) => egui::IconData::default(),
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_title("DbLens — 万能查看器")
            .with_icon(load_app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "dblens",
        options,
        Box::new(|cc| Ok(Box::new(DblensApp::new(cc)))),
    )
}
