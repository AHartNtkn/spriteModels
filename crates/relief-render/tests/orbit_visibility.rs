use std::{collections::VecDeque, path::Path};

use depthsprite_format::load_path;
use relief_core::CanonicalView;
use relief_render::{FrameBuffer, RenderRequest, TargetView, render_model};

fn globe() -> relief_core::AuthoredModel {
    load_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("assets/examples/globe.depthsprite"),
    )
    .expect("the checked-in globe must load")
}

fn opaque_count(frame: &FrameBuffer) -> usize {
    frame.pixels().iter().filter(|pixel| pixel[3] != 0).count()
}

fn owner_count(frame: &FrameBuffer, view: CanonicalView) -> usize {
    (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.owner_at(x, y).is_some_and(|owner| owner.view == view))
        .count()
}

fn opaque_is_connected(frame: &FrameBuffer) -> bool {
    let Some(start) = (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .find(|&(x, y)| frame.rgba_at(x, y)[3] != 0)
    else {
        return false;
    };
    let mut visited = vec![false; (frame.width() * frame.height()) as usize];
    let mut pending = VecDeque::from([start]);
    visited[(start.1 * frame.width() + start.0) as usize] = true;
    let mut reached = 0;
    while let Some((x, y)) = pending.pop_front() {
        reached += 1;
        for dy in -1..=1_i32 {
            for dx in -1..=1_i32 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (next_x, next_y) = (x as i32 + dx, y as i32 + dy);
                if next_x < 0 || next_y < 0 {
                    continue;
                }
                let (next_x, next_y) = (next_x as u32, next_y as u32);
                if next_x >= frame.width() || next_y >= frame.height() {
                    continue;
                }
                let index = (next_y * frame.width() + next_x) as usize;
                if !visited[index] && frame.rgba_at(next_x, next_y)[3] != 0 {
                    visited[index] = true;
                    pending.push_back((next_x, next_y));
                }
            }
        }
    }
    reached == opaque_count(frame)
}

#[test]
fn exact_side_globe_is_a_complete_connected_disc_from_both_sprites() {
    let model = globe();
    let resolved = model.resolve();
    let front = render_model(&resolved, &RenderRequest::new(96, 96, TargetView::front())).unwrap();
    let side = render_model(&resolved, &RenderRequest::new(96, 96, TargetView::right())).unwrap();

    assert!(
        opaque_count(&side) >= opaque_count(&front) * 9 / 10,
        "an exact side view must retain the full globe rather than collapse or disappear"
    );
    assert!(opaque_is_connected(&side));
    assert!(owner_count(&side, CanonicalView::Front) > 0);
    assert!(owner_count(&side, CanonicalView::Back) > 0);
}

#[test]
fn oblique_globe_combines_locally_visible_regions_of_both_sprites() {
    let model = globe();
    let frame = render_model(
        &model.resolve(),
        &RenderRequest::new(96, 96, TargetView::isometric()),
    )
    .unwrap();

    assert!(opaque_is_connected(&frame));
    assert!(owner_count(&frame, CanonicalView::Front) > 0);
    assert!(owner_count(&frame, CanonicalView::Back) > 0);
}
