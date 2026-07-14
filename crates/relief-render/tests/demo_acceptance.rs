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
    }
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
    assert!(
        opaque_is_connected(&oblique),
        "the oblique globe silhouette must be one eight-connected region"
    );
}
