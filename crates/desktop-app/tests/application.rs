use std::process::Command;

#[test]
fn help_reports_the_optional_model_path_without_starting_the_event_loop() {
    let output = Command::new(env!("CARGO_BIN_EXE_depthsprite"))
        .arg("--help")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Usage: depthsprite [MODEL]"));
    assert!(stdout.contains("MODEL"));
    assert!(stdout.contains("optional .depthsprite model path"));
    assert!(output.stderr.is_empty());
}
