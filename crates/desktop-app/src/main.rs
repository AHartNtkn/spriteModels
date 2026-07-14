use std::path::PathBuf;

use desktop_app::{DepthSpriteApp, layout::minimum_window_size};

fn main() -> eframe::Result {
    let startup_path = std::env::args_os().nth(1).map(PathBuf::from);
    let minimum = minimum_window_size();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([minimum.width, minimum.height])
            .with_min_inner_size([minimum.width, minimum.height]),
        ..Default::default()
    };
    eframe::run_native(
        "DepthSprite",
        options,
        Box::new(move |_creation| Ok(Box::new(DepthSpriteApp::from_startup_path(startup_path)))),
    )
}
