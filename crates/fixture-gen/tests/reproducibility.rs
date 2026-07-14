use std::path::{Path, PathBuf};

use depthsprite_format::load_path;
use fixture_gen::generate_examples;

const ASSETS: [&str; 6] = [
    "block.depthsprite",
    "bowl.depthsprite",
    "globe.depthsprite",
    "gyroscope.depthsprite",
    "tent.depthsprite",
    "dome.depthsprite",
];

fn committed_asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/examples")
        .join(name)
}

#[test]
fn one_generation_operation_is_reproducible_and_matches_committed_assets() {
    let temporary = tempfile::tempdir().unwrap();
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");

    generate_examples(&first).unwrap();
    generate_examples(&second).unwrap();

    for name in ASSETS {
        let first_path = first.join(name);
        let second_path = second.join(name);
        let first_bytes = std::fs::read(&first_path)
            .unwrap_or_else(|error| panic!("generated {name} must exist: {error}"));
        let second_bytes = std::fs::read(&second_path)
            .unwrap_or_else(|error| panic!("second generated {name} must exist: {error}"));
        assert_eq!(
            first_bytes, second_bytes,
            "independent generations of {name} must match"
        );
        assert_eq!(
            first_bytes,
            std::fs::read(committed_asset(name))
                .unwrap_or_else(|error| panic!("committed {name} must exist: {error}")),
            "committed {name} must come from generate_examples"
        );
        load_path(first_path).unwrap();
        load_path(second_path).unwrap();
    }
}
