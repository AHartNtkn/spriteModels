use std::path::PathBuf;

use desktop_app::{DepthSpriteApp, layout::minimum_window_size};

fn main() -> eframe::Result {
    let Some(startup_path) = startup_path() else {
        print_help();
        return Ok(());
    };
    run(startup_path)
}

fn startup_path() -> Option<Option<PathBuf>> {
    match std::env::args_os().nth(1) {
        Some(argument) if argument == "--help" || argument == "-h" => None,
        Some(path) => Some(Some(PathBuf::from(path))),
        None => Some(None),
    }
}

fn print_help() {
    println!(
        "DepthSprite model editor\n\nUsage: depthsprite [MODEL]\n\nArguments:\n  [MODEL]  optional .depthsprite model path\n\nOptions:\n  -h, --help  Print help"
    );
}

fn run(startup_path: Option<PathBuf>) -> eframe::Result {
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
