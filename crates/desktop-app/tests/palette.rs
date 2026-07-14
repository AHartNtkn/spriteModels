use desktop_app::palette::{
    apply_hex_rgb, format_rgb_hex, relief_labels, select_tool, set_rgb_channel, tool_entries,
};
use editor_core::{ActiveLayer, DepthValue, EditorDocument, ReliefValue, Tool};
use relief_core::{Bounds, CanonicalView};

const VIEW: CanonicalView = CanonicalView::Front;

fn document(width: u32) -> EditorDocument {
    EditorDocument::new(Bounds::new(width, 1, 1).unwrap(), VIEW)
}

fn pixels(document: &EditorDocument) -> Vec<[u8; 4]> {
    document.source(VIEW).unwrap().rgba().to_vec()
}

#[test]
fn tools_are_presented_in_the_required_vertical_order() {
    assert_eq!(
        tool_entries()
            .map(|entry| (entry.tool, entry.label))
            .collect::<Vec<_>>(),
        [
            (Tool::Pencil, "Pencil"),
            (Tool::Eraser, "Eraser"),
            (Tool::Fill, "Fill"),
            (Tool::Eyedropper, "Eyedropper"),
        ]
    );
}

#[test]
fn selecting_a_palette_tool_updates_the_document_selection() {
    let mut document = document(1);
    select_tool(&mut document, Tool::Fill);
    assert_eq!(document.tool(), Tool::Fill);
    select_tool(&mut document, Tool::Eyedropper);
    assert_eq!(document.tool(), Tool::Eyedropper);
}

#[test]
fn direct_channels_and_six_digit_hex_update_the_document_picker() {
    let mut document = document(1);

    set_rgb_channel(&mut document, 0, 0x12).unwrap();
    set_rgb_channel(&mut document, 1, 0x34).unwrap();
    set_rgb_channel(&mut document, 2, 0x56).unwrap();
    assert_eq!(document.current_rgb(), [0x12, 0x34, 0x56]);
    assert_eq!(format_rgb_hex(document.current_rgb()), "123456");

    apply_hex_rgb(&mut document, "aBcDeF").unwrap();
    assert_eq!(document.current_rgb(), [0xab, 0xcd, 0xef]);
    assert_eq!(format_rgb_hex(document.current_rgb()), "ABCDEF");

    assert!(apply_hex_rgb(&mut document, "#123456").is_err());
    assert!(apply_hex_rgb(&mut document, "12345").is_err());
    assert!(apply_hex_rgb(&mut document, "GG0000").is_err());
    assert_eq!(document.current_rgb(), [0xab, 0xcd, 0xef]);
}

#[test]
fn palette_selected_color_is_consumed_by_pencil_and_fill_commands() {
    let mut pencil = document(1);
    pencil.set_active_layer(ActiveLayer::Color);
    apply_hex_rgb(&mut pencil, "C86432").unwrap();
    pencil.begin_stroke().unwrap();
    pencil.pencil_pixel(VIEW, 0, 0).unwrap();
    pencil.finish_stroke().unwrap();
    assert_eq!(pixels(&pencil), [[200, 100, 50, 0]]);

    let mut fill = document(2);
    fill.set_active_layer(ActiveLayer::Color);
    set_rgb_channel(&mut fill, 0, 9).unwrap();
    set_rgb_channel(&mut fill, 1, 8).unwrap();
    set_rgb_channel(&mut fill, 2, 7).unwrap();
    fill.fill(VIEW, 0, 0).unwrap();
    assert_eq!(pixels(&fill), [[9, 8, 7, 0], [9, 8, 7, 0]]);
}

#[test]
fn relief_labels_show_eighth_pixel_units_and_model_pixels() {
    let labels = relief_labels(DepthValue::Relief(ReliefValue::new(42).unwrap()));
    assert_eq!(labels.units, "42 eighth-pixel units");
    assert_eq!(labels.model_pixels, "5.25 model pixels");

    let empty = relief_labels(DepthValue::Empty);
    assert_eq!(empty.units, "Empty");
    assert_eq!(empty.model_pixels, "No model surface");
}
