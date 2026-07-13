use std::{fs, path::PathBuf};

use desktop_app::document::Document;
use relief_render::{DirectionCount, SheetRequest, encode_png, render_sheet};
use tempfile::tempdir;

fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/examples")
        .join(name)
}

fn export_request() -> SheetRequest {
    SheetRequest::new(DirectionCount::Eight, 1, 1, 1).unwrap()
}

#[test]
fn failed_replacement_retains_model_path_and_hash() {
    let initial = asset("block.depthsprite");
    let mut document = Document::open(&initial).unwrap();
    let original_hash = document.model_hash();
    let original_path = document.path().map(ToOwned::to_owned);

    assert!(
        document
            .replace_from_path(asset("missing.depthsprite"))
            .is_err()
    );

    assert_eq!(document.model_hash(), original_hash);
    assert_eq!(document.path(), original_path.as_deref());
    assert_eq!(document.display_name(), "block.depthsprite");
}

#[test]
fn canonical_hash_is_stable_after_save_and_reopen() {
    let temp = tempdir().unwrap();
    let saved = temp.path().join("copy.depthsprite");
    let mut document = Document::open(asset("bowl.depthsprite")).unwrap();
    let original_hash = document.model_hash();

    document.save_as(&saved).unwrap();
    let reopened = Document::open(&saved).unwrap();

    assert_eq!(document.model_hash(), original_hash);
    assert_eq!(reopened.model_hash(), original_hash);
    assert_eq!(document.path(), Some(saved.as_path()));
    assert_eq!(document.display_name(), "copy.depthsprite");
}

#[test]
fn export_sheet_persists_the_authoritative_render_and_png_bytes() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("sheet.png");
    let document = Document::open(asset("block.depthsprite")).unwrap();
    let request = export_request();
    let expected = encode_png(
        &render_sheet(
            document.model().charts(),
            document.model().bounds(),
            &request,
        )
        .unwrap(),
    )
    .unwrap();

    document.export_sheet(&destination, &request).unwrap();

    assert_eq!(fs::read(destination).unwrap(), expected);
}

#[test]
fn failed_png_export_preserves_existing_destination() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("sheet.png");
    let temporary = temp.path().join("sheet.png.tmp");
    let original = b"existing png remains";
    fs::write(&destination, original).unwrap();
    fs::write(&temporary, b"foreign temp").unwrap();
    let document = Document::open(asset("block.depthsprite")).unwrap();

    assert!(
        document
            .export_sheet(&destination, &export_request())
            .is_err()
    );

    assert_eq!(fs::read(destination).unwrap(), original);
    assert_eq!(fs::read(temporary).unwrap(), b"foreign temp");
}
