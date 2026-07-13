use std::io::{Cursor, Read};

use flate2::read::ZlibDecoder;
use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, Chart, SourcePoint};
use relief_render::{
    DirectionCount, RenderDiagnostic, RenderRequest, SheetError, SheetRequest, encode_png,
    render_model, render_sheet,
};

fn flat_chart(bounds: Bounds, view: CanonicalView, rgb: [u8; 3]) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(
        bounds,
        view,
        width,
        height,
        vec![[rgb[0], rgb[1], rgb[2], 255]; (width * height) as usize],
    )
    .unwrap()
}

fn reference_charts(bounds: Bounds) -> Vec<Chart> {
    vec![
        flat_chart(bounds, CanonicalView::Front, [220, 40, 40]),
        flat_chart(bounds, CanonicalView::Right, [40, 220, 40]),
        flat_chart(bounds, CanonicalView::Top, [40, 40, 220]),
    ]
}

#[test]
fn request_rejects_zero_scale_and_every_unsupported_elevation() {
    assert_eq!(
        SheetRequest::new(DirectionCount::Eight, 0, 1, 1),
        Err(SheetError::ZeroScale)
    );
    assert_eq!(
        SheetRequest::new(DirectionCount::Eight, 1, 1, 0),
        Err(SheetError::UnsupportedElevation(0))
    );
    assert_eq!(
        SheetRequest::new(DirectionCount::Sixteen, 1, 1, 2),
        Err(SheetError::UnsupportedElevation(2))
    );
}

#[test]
fn sixteen_v1_targets_use_the_exact_clockwise_horizontal_table() {
    let request = SheetRequest::new(DirectionCount::Sixteen, 1, 0, 1).unwrap();
    let expected = [
        (0, 256),
        (98, 237),
        (181, 181),
        (237, 98),
        (256, 0),
        (237, -98),
        (181, -181),
        (98, -237),
        (0, -256),
        (-98, -237),
        (-181, -181),
        (-237, -98),
        (-256, 0),
        (-237, 98),
        (-181, 181),
        (-98, 237),
    ];
    let bounds = Bounds::new(512, 1, 512).unwrap();

    for (index, (sin, cos)) in expected.into_iter().enumerate() {
        let target = request.target_view(index).unwrap();
        let warp = target
            .warp_coefficients(CanonicalView::Top, bounds)
            .unwrap();
        let x_axis = warp.apply(
            SourcePoint::new(Ratio::from_integer(362), Ratio::from_integer(0)),
            Ratio::from_integer(0),
        );
        let z_axis = warp.apply(
            SourcePoint::new(Ratio::from_integer(0), Ratio::from_integer(362)),
            Ratio::from_integer(0),
        );

        assert_eq!(x_axis.screen_x, Ratio::from_integer(cos), "index {index}");
        assert_eq!(z_axis.screen_x, Ratio::from_integer(sin), "index {index}");
    }
    assert!(request.target_view(16).is_none());
}

#[test]
fn clockwise_cardinals_are_front_right_back_left_and_keep_top_visible() {
    let request = SheetRequest::new(DirectionCount::Sixteen, 1, 0, 1).unwrap();
    let cardinals = [
        (0, CanonicalView::Front),
        (4, CanonicalView::Right),
        (8, CanonicalView::Back),
        (12, CanonicalView::Left),
    ];

    for (index, horizontal_view) in cardinals {
        let target = request.target_view(index).unwrap();
        assert!(target.is_front_facing(horizontal_view));
        assert!(target.is_front_facing(CanonicalView::Top));
    }
    assert_eq!(
        request.target_view(2),
        Some(relief_render::TargetView::isometric_v1())
    );
}

#[test]
fn eight_targets_are_exactly_every_second_sixteen_target() {
    let eight = SheetRequest::new(DirectionCount::Eight, 1, 0, 1).unwrap();
    let sixteen = SheetRequest::new(DirectionCount::Sixteen, 1, 0, 1).unwrap();

    assert_eq!(eight.direction_count().frame_count(), 8);
    assert_eq!(sixteen.direction_count().frame_count(), 16);
    for index in 0..8 {
        assert_eq!(eight.target_view(index), sixteen.target_view(index * 2));
    }
    assert!(eight.target_view(8).is_none());
}

#[test]
fn sheet_aggregates_diagnostics_and_maps_pixel_coordinates_without_changing_pixels() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let charts = vec![
        flat_chart(bounds, CanonicalView::Front, [220, 40, 40]),
        flat_chart(bounds, CanonicalView::Front, [40, 220, 40]),
    ];
    let request = SheetRequest::new(DirectionCount::Eight, 2, 3, 1).unwrap();
    let frame_side = 65;
    let cell_side = frame_side * request.integer_scale() + 2 * request.padding();
    let sheet = render_sheet(&charts, bounds, &request).unwrap();
    let mut expected_diagnostics = Vec::new();

    for direction in 0..request.direction_count().frame_count() {
        let frame = render_model(
            &charts,
            &RenderRequest::new(
                frame_side,
                frame_side,
                request.target_view(direction).unwrap(),
            ),
        )
        .unwrap();
        let cell_x = direction as u32 * cell_side + request.padding();

        for diagnostic in frame.diagnostics() {
            match diagnostic {
                RenderDiagnostic::EqualDepthColorConflict {
                    x,
                    y,
                    first,
                    second,
                } => {
                    for scale_y in 0..request.integer_scale() {
                        for scale_x in 0..request.integer_scale() {
                            expected_diagnostics.push(RenderDiagnostic::EqualDepthColorConflict {
                                x: cell_x + x * request.integer_scale() + scale_x,
                                y: request.padding() + y * request.integer_scale() + scale_y,
                                first: *first,
                                second: *second,
                            });
                        }
                    }
                }
                other => expected_diagnostics.push(other.clone()),
            }
        }

        for source_y in 0..frame_side {
            for source_x in 0..frame_side {
                for scale_y in 0..request.integer_scale() {
                    for scale_x in 0..request.integer_scale() {
                        assert_eq!(
                            sheet.rgba_at(
                                cell_x + source_x * request.integer_scale() + scale_x,
                                request.padding() + source_y * request.integer_scale() + scale_y,
                            ),
                            frame.rgba_at(source_x, source_y),
                        );
                    }
                }
            }
        }
    }

    expected_diagnostics.sort();
    expected_diagnostics.dedup();
    assert!(
        expected_diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            RenderDiagnostic::EqualDepthColorConflict { .. }
        ))
    );
    assert!(
        expected_diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic, RenderDiagnostic::InsufficientCoverage { .. }))
    );
    assert_eq!(sheet.diagnostics(), expected_diagnostics);
}

#[test]
fn layout_uses_registered_bounds_global_relief_margin_and_transparent_cell_padding() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let charts = reference_charts(bounds);
    let base_request = SheetRequest::new(DirectionCount::Eight, 1, 0, 1).unwrap();
    let scaled_request = SheetRequest::new(DirectionCount::Eight, 2, 1, 1).unwrap();
    let base = render_sheet(&charts, bounds, &base_request).unwrap();
    let scaled = render_sheet(&charts, bounds, &scaled_request).unwrap();

    // ceil(254 / 8) is reserved on both sides of max(Bounds), independent of sparse content.
    assert_eq!((base.width(), base.height()), (8 * 65, 65));
    assert_eq!((scaled.width(), scaled.height()), (8 * 132, 132));

    for pixel in (0..scaled.width()).map(|x| scaled.rgba_at(x, 0)) {
        assert_eq!(pixel, [0, 0, 0, 0]);
    }
    for direction in 0..8_u32 {
        let cell_x = direction * 132;
        for y in 0..132 {
            assert_eq!(scaled.rgba_at(cell_x, y), [0, 0, 0, 0]);
            assert_eq!(scaled.rgba_at(cell_x + 131, y), [0, 0, 0, 0]);
        }
    }

    for direction in 0..8_u32 {
        for y in 0..65_u32 {
            for x in 0..65_u32 {
                let expected = base.rgba_at(direction * 65 + x, y);
                for dy in 0..2 {
                    for dx in 0..2 {
                        assert_eq!(
                            scaled.rgba_at(direction * 132 + 1 + x * 2 + dx, 1 + y * 2 + dy),
                            expected
                        );
                    }
                }
            }
        }
    }
}

fn png_chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut chunks = Vec::new();
    let mut offset = 8;
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[offset + 4..offset + 8].try_into().unwrap();
        let data = bytes[offset + 8..offset + 8 + length].to_vec();
        chunks.push((kind, data));
        offset += 12 + length;
    }
    assert_eq!(offset, bytes.len());
    chunks
}

#[test]
fn png_is_repeatedly_identical_rgba8_with_only_critical_chunks_and_filter_zero() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let request = SheetRequest::new(DirectionCount::Eight, 1, 1, 1).unwrap();
    let charts = reference_charts(bounds);
    let first_frame = render_sheet(&charts, bounds, &request).unwrap();
    let second_frame = render_sheet(&charts, bounds, &request).unwrap();

    let first = encode_png(&first_frame).unwrap();
    let second = encode_png(&second_frame).unwrap();
    assert_eq!(first, second);

    let chunks = png_chunks(&first);
    assert_eq!(chunks.first().unwrap().0, *b"IHDR");
    assert_eq!(chunks.last().unwrap().0, *b"IEND");
    assert!(
        chunks
            .iter()
            .all(|(kind, _)| matches!(kind, b"IHDR" | b"IDAT" | b"IEND"))
    );
    let ihdr = &chunks[0].1;
    assert_eq!(ihdr[8], 8);
    assert_eq!(ihdr[9], 6);

    let compressed: Vec<u8> = chunks
        .iter()
        .filter(|(kind, _)| kind == b"IDAT")
        .flat_map(|(_, data)| data.iter().copied())
        .collect();
    let mut scanlines = Vec::new();
    ZlibDecoder::new(Cursor::new(compressed))
        .read_to_end(&mut scanlines)
        .unwrap();
    let stride = 1 + first_frame.width() as usize * 4;
    assert_eq!(scanlines.len(), stride * first_frame.height() as usize);
    assert!(scanlines.chunks_exact(stride).all(|row| row[0] == 0));
}
