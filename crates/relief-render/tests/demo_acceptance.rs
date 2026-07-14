use std::{
    collections::{BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

use depthsprite_format::load_path;
use relief_core::{Bounds, CanonicalView, Chart, DecodedTexel};
use relief_render::{FrameBuffer, RenderRequest, TargetView, render_model};

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
        let (upper_left, upper_left_relief) = authored_pixel(chart, 0, 0);
        let (lower_right, lower_right_relief) = authored_pixel(chart, 15, 15);
        assert_eq!(upper_left_relief, lower_right_relief);
        assert!(
            brightness(upper_left) > brightness(lower_right),
            "{:?} must be brighter at upper-left than lower-right",
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

fn assert_center_out_relief_is_monotonic(chart: &Chart, coordinates: Vec<(u32, u32)>) {
    let relief = coordinates
        .into_iter()
        .map(|(x, y)| authored_pixel(chart, x, y).1)
        .collect::<Vec<_>>();
    assert!(relief.len() >= 20);
    assert!(
        relief.windows(2).all(|pair| pair[0] <= pair[1]),
        "{:?} center-out relief must be nondecreasing: {relief:?}",
        chart.view()
    );
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

    for chart in [front, back] {
        let mut boundary_count = 0;
        for y in 0..48_i32 {
            for x in 0..48_i32 {
                if foreground(chart, x, y)
                    && (-1..=1).any(|dy| {
                        (-1..=1)
                            .any(|dx| (dx != 0 || dy != 0) && !foreground(chart, x + dx, y + dy))
                    })
                {
                    boundary_count += 1;
                    let Some(DecodedTexel::Relief { eighths, .. }) =
                        chart.texel_at(x as u32, y as u32)
                    else {
                        unreachable!("the boundary predicate selected foreground")
                    };
                    assert_eq!(
                        eighths, 192,
                        "every silhouette boundary texel must meet at relief 192"
                    );
                }
            }
        }
        assert!(boundary_count > 0);

        assert_center_out_relief_is_monotonic(chart, (24..48).map(|x| (x, 23)).collect());
        assert_center_out_relief_is_monotonic(chart, (0..=23).rev().map(|x| (x, 23)).collect());
        assert_center_out_relief_is_monotonic(chart, (24..48).map(|y| (23, y)).collect());
        assert_center_out_relief_is_monotonic(chart, (0..=23).rev().map(|y| (23, y)).collect());
    }
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
fn foundational_globe_renders_explicit_front_back_and_connected_oblique_silhouette() {
    let model = load_path(asset("globe.depthsprite")).unwrap();
    let resolved = model.resolve();
    let front = render_model(&resolved, &RenderRequest::new(96, 96, TargetView::front())).unwrap();
    let back = render_model(&resolved, &RenderRequest::new(96, 96, TargetView::back())).unwrap();
    let oblique = render_model(
        &resolved,
        &RenderRequest::new(96, 96, TargetView::isometric()),
    )
    .unwrap();

    assert_frame_uses_explicit_chart(&front, model.chart(CanonicalView::Front).unwrap());
    assert_frame_uses_explicit_chart(&back, model.chart(CanonicalView::Back).unwrap());
    for y in 0..oblique.height() {
        for x in 0..oblique.width() {
            if oblique.rgba_at(x, y)[3] == 0 {
                continue;
            }
            assert_eq!(
                oblique.owner_at(x, y).unwrap().view,
                CanonicalView::Front,
                "the front-right-top globe view must not contain Back ownership"
            );
        }
    }
    assert!(
        opaque_is_connected(&oblique),
        "the oblique globe silhouette must be one eight-connected region"
    );
}
