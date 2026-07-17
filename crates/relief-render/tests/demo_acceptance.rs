use std::{
    collections::{BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

use depthsprite_format::load_path;
use relief_core::{Bounds, CanonicalView, Chart, DecodedTexel};
use relief_render::{FrameBuffer, PreparedModel, RenderRequest, TargetView, render_model};

fn asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/examples")
        .join(name)
}

fn foreground(chart: &Chart, x: i32, y: i32) -> bool {
    let (width, height) = chart.dimensions();
    x >= 0
        && y >= 0
        && x < width as i32
        && y < height as i32
        && matches!(
            chart.texel_at(x as u32, y as u32),
            Some(DecodedTexel::Relief { .. })
        )
}

fn reliefs(chart: &Chart) -> BTreeSet<u8> {
    chart
        .texels()
        .filter_map(|texel| match texel {
            DecodedTexel::Background => None,
            DecodedTexel::Relief { eighths, .. } => Some(eighths),
        })
        .collect()
}

fn rgbs(chart: &Chart) -> BTreeSet<[u8; 3]> {
    chart
        .texels()
        .filter_map(|texel| match texel {
            DecodedTexel::Background => None,
            DecodedTexel::Relief { rgb, .. } => Some(rgb),
        })
        .collect()
}

fn authored_pixel(chart: &Chart, x: u32, y: u32) -> ([u8; 3], u8) {
    match chart.texel_at(x, y) {
        Some(DecodedTexel::Relief { rgb, eighths }) => (rgb, eighths),
        _ => panic!("expected authored foreground at ({x}, {y})"),
    }
}

fn brightness(rgb: [u8; 3]) -> u32 {
    rgb.into_iter().map(u32::from).sum()
}

fn average_brightness(chart: &Chart) -> u32 {
    let total: u32 = chart
        .texels()
        .map(|texel| match texel {
            DecodedTexel::Background => panic!("the block must fully author every face"),
            DecodedTexel::Relief { rgb, .. } => brightness(rgb),
        })
        .sum();
    total / chart.texels().len() as u32
}

fn row_reliefs(chart: &Chart, y: u32) -> BTreeSet<u8> {
    let (width, _) = chart.dimensions();
    (0..width)
        .filter_map(|x| match chart.texel_at(x, y) {
            Some(DecodedTexel::Relief { eighths, .. }) => Some(eighths),
            _ => None,
        })
        .collect()
}

fn column_reliefs(chart: &Chart, x: u32) -> BTreeSet<u8> {
    let (_, height) = chart.dimensions();
    (0..height)
        .filter_map(|y| match chart.texel_at(x, y) {
            Some(DecodedTexel::Relief { eighths, .. }) => Some(eighths),
            _ => None,
        })
        .collect()
}

#[test]
fn foundational_block_authors_six_lit_zero_relief_charts() {
    let model = load_path(asset("block.depthsprite")).unwrap();
    assert_eq!(model.bounds(), Bounds::new(16, 16, 16).unwrap());
    assert_eq!(
        model
            .charts()
            .iter()
            .map(Chart::view)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            CanonicalView::Front,
            CanonicalView::Back,
            CanonicalView::Left,
            CanonicalView::Right,
            CanonicalView::Top,
            CanonicalView::Bottom,
        ])
    );

    for chart in model.charts() {
        assert_eq!(
            chart.texels().len(),
            chart
                .texels()
                .filter(|texel| matches!(texel, DecodedTexel::Relief { eighths: 0, .. }))
                .count(),
            "{:?} must be fully foreground at zero relief",
            chart.view()
        );
        assert!(
            rgbs(chart).len() >= 4,
            "{:?} must carry a visible within-face light gradient",
            chart.view()
        );
        let (bright, dark) = match chart.view() {
            CanonicalView::Back | CanonicalView::Right => ((15, 0), (0, 15)),
            CanonicalView::Bottom => ((0, 15), (15, 0)),
            _ => ((0, 0), (15, 15)),
        };
        let (bright_rgb, bright_relief) = authored_pixel(chart, bright.0, bright.1);
        let (dark_rgb, dark_relief) = authored_pixel(chart, dark.0, dark.1);
        assert_eq!(bright_relief, dark_relief);
        assert!(
            brightness(bright_rgb) > brightness(dark_rgb),
            "{:?} must follow the world-directed bright-to-shadow diagonal",
            chart.view()
        );
    }

    let ordered_faces = [
        CanonicalView::Top,
        CanonicalView::Front,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Back,
        CanonicalView::Bottom,
    ];
    let ordered_brightness =
        ordered_faces.map(|view| average_brightness(model.chart(view).unwrap()));
    assert!(
        ordered_brightness.windows(2).all(|pair| pair[0] > pair[1]),
        "average face brightness must be Top > Front > Left > Right > Back > Bottom: {ordered_brightness:?}"
    );
}

#[test]
fn foundational_bowl_has_shallow_basin_narrowing_exterior_and_lighting() {
    let model = load_path(asset("bowl.depthsprite")).unwrap();
    assert_eq!(model.bounds(), Bounds::new(32, 12, 32).unwrap());
    assert_eq!(
        model.charts().iter().map(Chart::view).collect::<Vec<_>>(),
        vec![CanonicalView::Front, CanonicalView::Top]
    );
    let front = model.chart(CanonicalView::Front).unwrap();
    let top = model.chart(CanonicalView::Top).unwrap();
    assert!(front.supplies_opposite());
    assert!(front.mirrors_opposite());
    assert!(!top.supplies_opposite());
    assert!(!top.mirrors_opposite());
    assert_eq!(front.dimensions(), (32, 12));
    assert_eq!(top.dimensions(), (32, 32));

    let row_counts = (0..12)
        .map(|y| (0..32).filter(|&x| foreground(front, x, y)).count())
        .collect::<Vec<_>>();
    assert_eq!(row_counts[0], 32, "the complete near rim must occupy row 0");
    assert!(
        (1..=2).contains(&row_counts[11]),
        "the bottom row must retain one or two center texels"
    );
    assert!(
        row_counts.windows(2).all(|pair| pair[0] >= pair[1]),
        "the exterior half-width must decrease monotonically"
    );
    assert!(
        (0..12).filter(|&y| row_reliefs(front, y).len() > 1).count() >= 4,
        "representative Front rows must curve in relief"
    );
    assert!(
        (0..32)
            .filter(|&x| column_reliefs(front, x).len() > 1)
            .count()
            >= 4,
        "representative Front columns must change relief vertically"
    );
    assert!(
        reliefs(top).iter().copied().max().unwrap() <= 48,
        "the basin must remain shallow"
    );
    assert!(
        reliefs(front).iter().copied().max().unwrap() <= 128,
        "the exterior must stay inside its authored half-depth"
    );
    assert!(reliefs(top).len() >= 4);
    assert!(reliefs(front).len() >= 4);
    assert!(rgbs(top).len() >= 4, "Top must be directionally lit");
    assert!(rgbs(front).len() >= 4, "Front must be directionally lit");

    let (top_upper_left, top_upper_left_relief) = authored_pixel(top, 8, 8);
    let (top_lower_right, top_lower_right_relief) = authored_pixel(top, 23, 23);
    assert_eq!(
        top_upper_left_relief, top_lower_right_relief,
        "symmetric Top lighting samples must control for relief darkening"
    );
    assert!(
        brightness(top_upper_left) > brightness(top_lower_right),
        "the relief-matched Top upper-left must be brighter than lower-right"
    );

    let (front_left, front_left_relief) = authored_pixel(front, 5, 4);
    let (front_right, front_right_relief) = authored_pixel(front, 26, 4);
    assert_eq!(
        front_left_relief, front_right_relief,
        "same-row symmetric Front samples must control for relief darkening"
    );
    assert!(
        brightness(front_left) > brightness(front_right),
        "the relief-matched Front left must be brighter than right"
    );
}

fn classified_mask(chart: &Chart, is_land: impl Fn([u8; 3]) -> bool) -> BTreeSet<(u32, u32)> {
    let (width, height) = chart.dimensions();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| match chart.texel_at(x, y) {
            Some(DecodedTexel::Relief { rgb, .. }) => is_land(rgb),
            _ => false,
        })
        .collect()
}

fn foreground_count(chart: &Chart) -> usize {
    chart
        .texels()
        .filter(|texel| matches!(texel, DecodedTexel::Relief { .. }))
        .count()
}

fn definition_floor_sqrt(value: u32) -> u32 {
    let mut root: u32 = 0;
    while (root + 1).pow(2) <= value {
        root += 1;
    }
    root
}

fn globe_circle_contains(x: i32, y: i32) -> bool {
    let dx = 2 * x + 1 - 48;
    let dy = 2 * y + 1 - 48;
    dx * dx + dy * dy <= 48_i32.pow(2)
}

fn globe_circle_boundary(x: i32, y: i32) -> bool {
    globe_circle_contains(x, y)
        && (-1..=1).any(|dy| {
            (-1..=1).any(|dx| (dx != 0 || dy != 0) && !globe_circle_contains(x + dx, y + dy))
        })
}

fn assert_exact_globe_source_profile(chart: &Chart) {
    for y in 0..48_i32 {
        for x in 0..48_i32 {
            let actual = chart.texel_at(x as u32, y as u32);
            if !globe_circle_contains(x, y) {
                assert_eq!(
                    actual,
                    Some(DecodedTexel::Background),
                    "{:?} ({x}, {y}) must be outside the radius-48 circle",
                    chart.view()
                );
                continue;
            }

            let dx = 2 * x + 1 - 48;
            let dy = 2 * y + 1 - 48;
            let remaining = (48_i32.pow(2) - dx * dx - dy * dy) as u32;
            let sphere_relief = (4 * (48 - definition_floor_sqrt(remaining))).min(192) as u8;
            let expected_relief = if globe_circle_boundary(x, y) {
                192
            } else {
                sphere_relief
            };
            let Some(DecodedTexel::Relief { eighths, .. }) = actual else {
                panic!(
                    "{:?} ({x}, {y}) must be foreground inside the radius-48 circle",
                    chart.view()
                )
            };
            assert_eq!(
                eighths,
                expected_relief,
                "{:?} ({x}, {y}) must follow the exact sphere profile",
                chart.view()
            );
        }
    }
}

#[test]
fn foundational_globe_authors_distinct_hemispheres_with_meeting_boundaries() {
    let model = load_path(asset("globe.depthsprite")).unwrap();
    assert_eq!(model.bounds(), Bounds::new(48, 48, 48).unwrap());
    assert_eq!(
        model.charts().iter().map(Chart::view).collect::<Vec<_>>(),
        vec![CanonicalView::Front, CanonicalView::Back]
    );
    let front = model.chart(CanonicalView::Front).unwrap();
    let back = model.chart(CanonicalView::Back).unwrap();
    assert_ne!(front.rgba(), back.rgba(), "hemisphere patterns must differ");

    let front_land = classified_mask(front, |rgb| rgb[1] > rgb[2]);
    let back_land = classified_mask(back, |rgb| rgb[0] > rgb[2]);
    for (view, land, foreground) in [
        (CanonicalView::Front, &front_land, foreground_count(front)),
        (CanonicalView::Back, &back_land, foreground_count(back)),
    ] {
        assert!(!land.is_empty(), "{view:?} must contain classified land");
        assert!(
            land.len() < foreground,
            "{view:?} must contain classified water"
        );
    }
    assert_ne!(
        front_land, back_land,
        "hemisphere land masks must differ independently of palette"
    );

    assert_exact_globe_source_profile(front);
    assert_exact_globe_source_profile(back);
}

fn assert_frame_uses_explicit_chart(frame: &FrameBuffer, chart: &Chart) {
    let mut rendered_colors = BTreeSet::new();
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let Some(owner) = frame.owner_at(x, y) else {
                continue;
            };
            assert_eq!(owner.view, chart.view());
            let DecodedTexel::Relief { rgb, .. } = chart
                .texel_at(owner.source_x, owner.source_y)
                .expect("owner coordinates must belong to the explicit chart")
            else {
                panic!("render owner must identify explicit foreground")
            };
            assert_eq!(frame.rgba_at(x, y), [rgb[0], rgb[1], rgb[2], 255]);
            rendered_colors.insert(rgb);
        }
    }
    assert!(
        rendered_colors.len() >= 4,
        "the render must preserve multiple explicit chart colors"
    );
}

fn opaque_is_connected(frame: &FrameBuffer) -> bool {
    let occupied = (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.rgba_at(x, y)[3] != 0)
        .collect::<BTreeSet<_>>();
    let Some(&start) = occupied.iter().next() else {
        return false;
    };
    let mut reached = BTreeSet::from([start]);
    let mut pending = VecDeque::from([start]);
    while let Some((x, y)) = pending.pop_front() {
        for dy in -1..=1_i32 {
            for dx in -1..=1_i32 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let neighbor = (x as i32 + dx, y as i32 + dy);
                if neighbor.0 >= 0 && neighbor.1 >= 0 {
                    let neighbor = (neighbor.0 as u32, neighbor.1 as u32);
                    if occupied.contains(&neighbor) && reached.insert(neighbor) {
                        pending.push_back(neighbor);
                    }
                }
            }
        }
    }
    reached == occupied
}

#[test]
fn foundational_globe_combines_explicit_front_back_into_a_connected_oblique_silhouette() {
    let model = load_path(asset("globe.depthsprite")).unwrap();
    let resolved = model.resolve();
    let prepared = PreparedModel::new(&resolved);
    let front = render_model(&prepared, &RenderRequest::new(96, 96, TargetView::front())).unwrap();
    let back = render_model(&prepared, &RenderRequest::new(96, 96, TargetView::back())).unwrap();
    let oblique = render_model(
        &prepared,
        &RenderRequest::new(96, 96, TargetView::isometric()),
    )
    .unwrap();

    assert_frame_uses_explicit_chart(&front, model.chart(CanonicalView::Front).unwrap());
    assert_frame_uses_explicit_chart(&back, model.chart(CanonicalView::Back).unwrap());
    let mut oblique_views = BTreeSet::new();
    for y in 0..oblique.height() {
        for x in 0..oblique.width() {
            if let Some(owner) = oblique.owner_at(x, y) {
                oblique_views.insert(owner.view);
            }
        }
    }
    assert!(oblique_views.contains(&CanonicalView::Front));
    assert!(oblique_views.contains(&CanonicalView::Back));
    assert!(
        opaque_is_connected(&oblique),
        "the oblique globe silhouette must be one eight-connected region"
    );
}

fn views(model: &relief_core::AuthoredModel) -> Vec<CanonicalView> {
    model.charts().iter().map(Chart::view).collect()
}

#[derive(Clone, Copy)]
struct GyroscopeProjection {
    radius_u: i32,
    radius_v: i32,
    direction_u: i32,
    direction_v: i32,
    thickness: i32,
    phase: i32,
    draw_order: i32,
}

const GYROSCOPE_PROJECTIONS: [(CanonicalView, [GyroscopeProjection; 3]); 6] = [
    (
        CanonicalView::Front,
        [
            GyroscopeProjection {
                radius_u: 21,
                radius_v: 15,
                direction_u: 4,
                direction_v: 1,
                thickness: 2,
                phase: 1,
                draw_order: 0,
            },
            GyroscopeProjection {
                radius_u: 16,
                radius_v: 10,
                direction_u: 3,
                direction_v: -2,
                thickness: 2,
                phase: 5,
                draw_order: 2,
            },
            GyroscopeProjection {
                radius_u: 10,
                radius_v: 6,
                direction_u: 1,
                direction_v: 0,
                thickness: 2,
                phase: 9,
                draw_order: 1,
            },
        ],
    ),
    (
        CanonicalView::Back,
        [
            GyroscopeProjection {
                radius_u: 20,
                radius_v: 14,
                direction_u: 3,
                direction_v: -2,
                thickness: 2,
                phase: 7,
                draw_order: 2,
            },
            GyroscopeProjection {
                radius_u: 16,
                radius_v: 11,
                direction_u: 4,
                direction_v: 1,
                thickness: 2,
                phase: 2,
                draw_order: 0,
            },
            GyroscopeProjection {
                radius_u: 9,
                radius_v: 6,
                direction_u: 1,
                direction_v: 1,
                thickness: 2,
                phase: 11,
                draw_order: 1,
            },
        ],
    ),
    (
        CanonicalView::Left,
        [
            GyroscopeProjection {
                radius_u: 21,
                radius_v: 13,
                direction_u: 2,
                direction_v: 3,
                thickness: 2,
                phase: 4,
                draw_order: 1,
            },
            GyroscopeProjection {
                radius_u: 15,
                radius_v: 11,
                direction_u: 1,
                direction_v: -3,
                thickness: 2,
                phase: 8,
                draw_order: 0,
            },
            GyroscopeProjection {
                radius_u: 10,
                radius_v: 5,
                direction_u: 1,
                direction_v: 0,
                thickness: 2,
                phase: 12,
                draw_order: 2,
            },
        ],
    ),
    (
        CanonicalView::Right,
        [
            GyroscopeProjection {
                radius_u: 20,
                radius_v: 15,
                direction_u: 1,
                direction_v: -3,
                thickness: 2,
                phase: 10,
                draw_order: 0,
            },
            GyroscopeProjection {
                radius_u: 16,
                radius_v: 9,
                direction_u: 2,
                direction_v: 3,
                thickness: 2,
                phase: 3,
                draw_order: 2,
            },
            GyroscopeProjection {
                radius_u: 9,
                radius_v: 6,
                direction_u: 0,
                direction_v: 1,
                thickness: 2,
                phase: 14,
                draw_order: 1,
            },
        ],
    ),
    (
        CanonicalView::Top,
        [
            GyroscopeProjection {
                radius_u: 19,
                radius_v: 16,
                direction_u: 3,
                direction_v: 1,
                thickness: 2,
                phase: 6,
                draw_order: 2,
            },
            GyroscopeProjection {
                radius_u: 16,
                radius_v: 10,
                direction_u: 1,
                direction_v: 3,
                thickness: 2,
                phase: 12,
                draw_order: 1,
            },
            GyroscopeProjection {
                radius_u: 10,
                radius_v: 6,
                direction_u: 2,
                direction_v: -1,
                thickness: 2,
                phase: 1,
                draw_order: 0,
            },
        ],
    ),
    (
        CanonicalView::Bottom,
        [
            GyroscopeProjection {
                radius_u: 21,
                radius_v: 14,
                direction_u: 1,
                direction_v: 3,
                thickness: 2,
                phase: 13,
                draw_order: 1,
            },
            GyroscopeProjection {
                radius_u: 15,
                radius_v: 11,
                direction_u: 3,
                direction_v: -1,
                thickness: 2,
                phase: 4,
                draw_order: 0,
            },
            GyroscopeProjection {
                radius_u: 9,
                radius_v: 5,
                direction_u: 1,
                direction_v: 2,
                thickness: 2,
                phase: 7,
                draw_order: 2,
            },
        ],
    ),
];

fn gyroscope_ring_at(x: i32, y: i32, projections: &[GyroscopeProjection; 3]) -> Option<usize> {
    let centered_x = 2 * x + 1 - 48;
    let centered_y = 2 * y + 1 - 48;
    projections
        .iter()
        .enumerate()
        .filter(|(_, projection)| {
            let u = projection.direction_u * centered_x + projection.direction_v * centered_y;
            let v = -projection.direction_v * centered_x + projection.direction_u * centered_y;
            let norm = projection.direction_u.pow(2) + projection.direction_v.pow(2);
            let radius_u = 2 * projection.radius_u;
            let radius_v = 2 * projection.radius_v;
            let metric = i64::from(u).pow(2) * 4096 / i64::from(norm * radius_u.pow(2))
                + i64::from(v).pow(2) * 4096 / i64::from(norm * radius_v.pow(2));
            let band = 8192 * i64::from(projection.thickness)
                / i64::from(projection.radius_u.min(projection.radius_v));
            (metric - 4096).abs() <= band
        })
        .max_by_key(|(_, projection)| projection.draw_order)
        .map(|(ring, _)| ring)
}

fn gyroscope_family(rgb: [u8; 3]) -> usize {
    if rgb[0] > rgb[1] && rgb[0] > rgb[2] {
        0
    } else if rgb[1] > rgb[0] && rgb[1] > rgb[2] {
        1
    } else {
        assert!(
            rgb[2] > rgb[0] && rgb[2] > rgb[1],
            "invalid ring color {rgb:?}"
        );
        2
    }
}

fn target(view: CanonicalView) -> TargetView {
    match view {
        CanonicalView::Front => TargetView::front(),
        CanonicalView::Right => TargetView::right(),
        CanonicalView::Back => TargetView::back(),
        CanonicalView::Left => TargetView::left(),
        CanonicalView::Top => TargetView::top(),
        CanonicalView::Bottom => TargetView::bottom(),
    }
}

fn owner_color_distribution(
    frame: &FrameBuffer,
) -> std::collections::BTreeMap<(CanonicalView, usize), usize> {
    let mut distribution = std::collections::BTreeMap::new();
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let Some(owner) = frame.owner_at(x, y) else {
                continue;
            };
            let rgb = frame.rgba_at(x, y);
            *distribution
                .entry((owner.view, gyroscope_family([rgb[0], rgb[1], rgb[2]])))
                .or_default() += 1;
        }
    }
    distribution
}

fn owner_views_touch(frame: &FrameBuffer, first: CanonicalView, second: CanonicalView) -> bool {
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            if frame.owner_at(x, y).map(|owner| owner.view) != Some(first) {
                continue;
            }
            for dy in -1..=1_i32 {
                for dx in -1..=1_i32 {
                    let neighbor_x = x as i32 + dx;
                    let neighbor_y = y as i32 + dy;
                    if neighbor_x >= 0
                        && neighbor_y >= 0
                        && frame
                            .owner_at(neighbor_x as u32, neighbor_y as u32)
                            .map(|owner| owner.view)
                            == Some(second)
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn has_two_axis_relief(chart: &Chart) -> bool {
    let (width, height) = chart.dimensions();
    (0..height)
        .filter(|&y| row_reliefs(chart, y).len() >= 3)
        .count()
        >= 3
        && (0..width)
            .filter(|&x| column_reliefs(chart, x).len() >= 3)
            .count()
            >= 3
}

#[test]
fn ambitious_gyroscope_authors_exact_asymmetric_ring_observations() {
    let gyroscope = load_path(asset("gyroscope.depthsprite")).unwrap();
    assert_eq!(gyroscope.bounds(), Bounds::new(48, 48, 48).unwrap());
    assert_eq!(
        views(&gyroscope),
        vec![
            CanonicalView::Front,
            CanonicalView::Right,
            CanonicalView::Back,
            CanonicalView::Left,
            CanonicalView::Top,
            CanonicalView::Bottom,
        ]
    );
    for (a, b) in [
        (CanonicalView::Front, CanonicalView::Back),
        (CanonicalView::Left, CanonicalView::Right),
        (CanonicalView::Top, CanonicalView::Bottom),
    ] {
        assert_ne!(
            gyroscope.chart(a).unwrap().rgba(),
            gyroscope.chart(b).unwrap().rgba()
        );
    }

    for (view, projections) in GYROSCOPE_PROJECTIONS {
        let chart = gyroscope.chart(view).unwrap();
        let mut families = BTreeSet::new();
        let mut empty = 0;
        assert!(
            projections
                .iter()
                .all(|projection| (0..16).contains(&projection.phase))
        );
        for y in 0..48_i32 {
            for x in 0..48_i32 {
                let actual = chart.texel_at(x as u32, y as u32).unwrap();
                match (gyroscope_ring_at(x, y, &projections), actual) {
                    (None, DecodedTexel::Background) => empty += 1,
                    (Some(expected), DecodedTexel::Relief { rgb, eighths }) => {
                        assert_eq!(
                            gyroscope_family(rgb),
                            expected,
                            "{view:?} ring at ({x}, {y})"
                        );
                        assert!(eighths <= 192);
                        families.insert(expected);
                    }
                    (expected, actual) => panic!(
                        "{view:?} source topology at ({x}, {y}) expected {expected:?}, got {actual:?}"
                    ),
                }
            }
        }
        assert_eq!(families, BTreeSet::from([0, 1, 2]));
        assert!(empty > 0, "{view:?} must preserve alpha-empty gaps");
        assert!(reliefs(chart).len() >= 8, "{view:?} must vary ring relief");
    }
}

#[test]
fn ambitious_gyroscope_opposite_renders_preserve_distinct_ownership_and_color() {
    let gyroscope = load_path(asset("gyroscope.depthsprite")).unwrap();
    let resolved = gyroscope.resolve();
    let prepared = PreparedModel::new(&resolved);
    for (a, b) in [
        (CanonicalView::Front, CanonicalView::Back),
        (CanonicalView::Left, CanonicalView::Right),
        (CanonicalView::Top, CanonicalView::Bottom),
    ] {
        let first = render_model(&prepared, &RenderRequest::new(96, 96, target(a))).unwrap();
        let second = render_model(&prepared, &RenderRequest::new(96, 96, target(b))).unwrap();
        let first_distribution = owner_color_distribution(&first);
        let second_distribution = owner_color_distribution(&second);
        assert!(first_distribution.keys().any(|(view, _)| *view == a));
        assert!(second_distribution.keys().any(|(view, _)| *view == b));
        assert!(
            first_distribution.keys().all(|(view, _)| *view != b),
            "the opposite-facing {b:?} image must not bleed into the {a:?} target"
        );
        assert!(
            second_distribution.keys().all(|(view, _)| *view != a),
            "the opposite-facing {a:?} image must not bleed into the {b:?} target"
        );
        assert_ne!(first_distribution, second_distribution);
    }
}

#[test]
fn ambitious_tent_authors_entrance_curvature_and_connected_landmarks() {
    let tent = load_path(asset("tent.depthsprite")).unwrap();
    assert_eq!(tent.bounds(), Bounds::new(48, 28, 36).unwrap());
    assert_eq!(
        views(&tent),
        vec![
            CanonicalView::Front,
            CanonicalView::Right,
            CanonicalView::Top
        ]
    );
    let front = tent.chart(CanonicalView::Front).unwrap();
    assert!(front.supplies_opposite());
    assert!(
        tent.chart(CanonicalView::Right)
            .unwrap()
            .supplies_opposite()
    );
    assert!(!tent.chart(CanonicalView::Top).unwrap().supplies_opposite());
    assert!(tent.resolve().chart(CanonicalView::Back).is_some());
    assert!(tent.resolve().chart(CanonicalView::Left).is_some());
    assert!(tent.resolve().chart(CanonicalView::Bottom).is_none());
    assert!(
        !foreground(front, 24, 24),
        "the entrance center must be alpha-empty"
    );
    assert!(
        foreground(front, 24, 14),
        "fabric must surround the entrance above"
    );
    assert!(
        foreground(front, 17, 24),
        "fabric must surround the entrance on the left"
    );
    assert!(
        foreground(front, 31, 24),
        "fabric must surround the entrance on the right"
    );
    assert!(
        foreground(front, 20, 22),
        "the foreground flap must remain separate from the entrance void"
    );
    for view in [CanonicalView::Right, CanonicalView::Top] {
        assert!(
            has_two_axis_relief(tent.chart(view).unwrap()),
            "{view:?} must vary relief along both axes"
        );
    }

    let oblique = render_model(
        &PreparedModel::new(&tent.resolve()),
        &RenderRequest::new(128, 96, TargetView::isometric()),
    )
    .unwrap();
    assert!(opaque_is_connected(&oblique));
    assert!(
        owner_views_touch(&oblique, CanonicalView::Front, CanonicalView::Top),
        "Front eave must meet the Top roof"
    );
    assert!(
        owner_views_touch(&oblique, CanonicalView::Right, CanonicalView::Top),
        "Right eave must meet the Top roof"
    );
}

fn dome_rib(rgb: [u8; 3]) -> bool {
    rgb[0] > rgb[1] && rgb[1] > rgb[2].saturating_add(20)
}

fn dome_window(rgb: [u8; 3]) -> bool {
    rgb[1].saturating_sub(rgb[0]) >= 14 && rgb[2].saturating_sub(rgb[1]) >= 14
}

#[test]
fn ambitious_dome_authors_ribs_relief_and_connected_crown_drum() {
    let dome = load_path(asset("dome.depthsprite")).unwrap();
    assert_eq!(dome.bounds(), Bounds::new(48, 32, 48).unwrap());
    assert_eq!(
        views(&dome),
        vec![
            CanonicalView::Front,
            CanonicalView::Right,
            CanonicalView::Top
        ]
    );
    assert!(
        dome.chart(CanonicalView::Front)
            .unwrap()
            .supplies_opposite()
    );
    assert!(
        dome.chart(CanonicalView::Right)
            .unwrap()
            .supplies_opposite()
    );
    assert!(!dome.chart(CanonicalView::Top).unwrap().supplies_opposite());
    assert!(dome.resolve().chart(CanonicalView::Back).is_some());
    assert!(dome.resolve().chart(CanonicalView::Left).is_some());
    assert!(dome.resolve().chart(CanonicalView::Bottom).is_none());
    for view in [
        CanonicalView::Front,
        CanonicalView::Right,
        CanonicalView::Top,
    ] {
        let chart = dome.chart(view).unwrap();
        assert!(
            has_two_axis_relief(chart),
            "{view:?} must vary relief along both axes"
        );
        let rib_pixels = chart
            .texels()
            .filter(|texel| matches!(texel, DecodedTexel::Relief { rgb, .. } if dome_rib(*rgb)))
            .count();
        assert!(
            rib_pixels >= 24,
            "{view:?} must repeat the architectural rib palette"
        );
    }
    assert!(
        !dome
            .chart(CanonicalView::Top)
            .unwrap()
            .texels()
            .any(|texel| matches!(texel, DecodedTexel::Relief { rgb, .. } if dome_window(rgb))),
        "windows must not appear on the shell Top"
    );
    for view in [CanonicalView::Front, CanonicalView::Right] {
        let chart = dome.chart(view).unwrap();
        let window_rows = (0..32)
            .flat_map(|y| {
                (0..48).filter_map(move |x| match chart.texel_at(x, y) {
                    Some(DecodedTexel::Relief { rgb, .. }) if dome_window(rgb) => Some(y),
                    _ => None,
                })
            })
            .collect::<BTreeSet<_>>();
        assert!(
            !window_rows.is_empty(),
            "{view:?} drum must contain windows"
        );
        assert!(
            window_rows.iter().all(|&y| y >= 20),
            "{view:?} windows must remain on the drum"
        );
    }
    assert!(
        reliefs(dome.chart(CanonicalView::Top).unwrap())
            .iter()
            .all(|&value| value <= 128)
    );
    for view in [CanonicalView::Front, CanonicalView::Right] {
        assert!(
            reliefs(dome.chart(view).unwrap())
                .iter()
                .all(|&value| value <= 192)
        );
    }

    let oblique = render_model(
        &PreparedModel::new(&dome.resolve()),
        &RenderRequest::new(128, 112, TargetView::isometric()),
    )
    .unwrap();
    let owners = (0..oblique.height())
        .flat_map(|y| {
            (0..oblique.width()).filter_map({
                let oblique = &oblique;
                move |x| oblique.owner_at(x, y)
            })
        })
        .collect::<Vec<_>>();
    assert!(!owners.is_empty());
    assert!(
        owners
            .iter()
            .all(|owner| owner.view != CanonicalView::Bottom),
        "the Top-only dome must never invent a Bottom observation"
    );
    for view in [
        CanonicalView::Front,
        CanonicalView::Right,
        CanonicalView::Top,
    ] {
        assert!(
            owners.iter().any(|owner| owner.view == view),
            "oblique render must retain {view:?} ownership"
        );
    }
    for view in [CanonicalView::Front, CanonicalView::Right] {
        assert!(
            owners
                .iter()
                .any(|owner| owner.view == view && owner.source_y < 20),
            "{view:?} must own crown pixels"
        );
        assert!(
            owners
                .iter()
                .any(|owner| owner.view == view && owner.source_y >= 20),
            "{view:?} must own drum pixels"
        );
        assert!(
            owner_views_touch(&oblique, view, CanonicalView::Top),
            "{view:?} crown must connect to Top shell ownership"
        );
    }
    assert!(
        opaque_is_connected(&oblique),
        "crown and drum must form one connected rendered silhouette"
    );
}
