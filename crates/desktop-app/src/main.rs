mod app;
mod export_ui;
mod jobs;
mod viewport;

fn main() -> eframe::Result {
    let bundled_bowl = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/examples/bowl.depthsprite");
    let initial_path = app::select_initial_path(std::env::args_os().nth(1), &bundled_bowl);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "DepthSprite",
        options,
        Box::new(move |_creation| Ok(Box::new(app::DepthSpriteApp::new(initial_path)))),
    )
}
