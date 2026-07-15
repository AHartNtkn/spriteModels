use relief_core::{
    AuthoredModel, AxisSide, Bounds, CanonicalView, Chart, ChartEdge, ChartError, DiscardPolicy,
    EMPTY_RGBA, ImageEdge, ModelError, ReassignMode, ResizeDelta, ResizeRequest, WorldAxis,
    WorldEdge,
};

fn pixel(value: u8) -> [u8; 4] {
    [value, value.wrapping_add(1), value.wrapping_add(2), 255]
}

fn chart(view: CanonicalView, width: u32, height: u32, start: u8) -> Chart {
    Chart::from_rgba(
        view,
        width,
        height,
        (0..(width * height))
            .map(|offset| pixel(start.wrapping_add(offset as u8)))
            .collect(),
    )
    .unwrap()
}

fn empty_chart(bounds: Bounds, view: CanonicalView) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(
        view,
        width,
        height,
        vec![EMPTY_RGBA; (width * height) as usize],
    )
    .unwrap()
}

#[test]
fn every_local_edge_round_trips_through_its_signed_world_edge() {
    let expected = [
        (
            CanonicalView::Front,
            ImageEdge::Left,
            WorldAxis::X,
            AxisSide::Min,
        ),
        (
            CanonicalView::Front,
            ImageEdge::Right,
            WorldAxis::X,
            AxisSide::Max,
        ),
        (
            CanonicalView::Front,
            ImageEdge::Top,
            WorldAxis::Y,
            AxisSide::Min,
        ),
        (
            CanonicalView::Front,
            ImageEdge::Bottom,
            WorldAxis::Y,
            AxisSide::Max,
        ),
        (
            CanonicalView::Back,
            ImageEdge::Left,
            WorldAxis::X,
            AxisSide::Max,
        ),
        (
            CanonicalView::Back,
            ImageEdge::Right,
            WorldAxis::X,
            AxisSide::Min,
        ),
        (
            CanonicalView::Back,
            ImageEdge::Top,
            WorldAxis::Y,
            AxisSide::Min,
        ),
        (
            CanonicalView::Back,
            ImageEdge::Bottom,
            WorldAxis::Y,
            AxisSide::Max,
        ),
        (
            CanonicalView::Left,
            ImageEdge::Left,
            WorldAxis::Z,
            AxisSide::Min,
        ),
        (
            CanonicalView::Left,
            ImageEdge::Right,
            WorldAxis::Z,
            AxisSide::Max,
        ),
        (
            CanonicalView::Left,
            ImageEdge::Top,
            WorldAxis::Y,
            AxisSide::Min,
        ),
        (
            CanonicalView::Left,
            ImageEdge::Bottom,
            WorldAxis::Y,
            AxisSide::Max,
        ),
        (
            CanonicalView::Right,
            ImageEdge::Left,
            WorldAxis::Z,
            AxisSide::Max,
        ),
        (
            CanonicalView::Right,
            ImageEdge::Right,
            WorldAxis::Z,
            AxisSide::Min,
        ),
        (
            CanonicalView::Right,
            ImageEdge::Top,
            WorldAxis::Y,
            AxisSide::Min,
        ),
        (
            CanonicalView::Right,
            ImageEdge::Bottom,
            WorldAxis::Y,
            AxisSide::Max,
        ),
        (
            CanonicalView::Top,
            ImageEdge::Left,
            WorldAxis::X,
            AxisSide::Min,
        ),
        (
            CanonicalView::Top,
            ImageEdge::Right,
            WorldAxis::X,
            AxisSide::Max,
        ),
        (
            CanonicalView::Top,
            ImageEdge::Top,
            WorldAxis::Z,
            AxisSide::Min,
        ),
        (
            CanonicalView::Top,
            ImageEdge::Bottom,
            WorldAxis::Z,
            AxisSide::Max,
        ),
        (
            CanonicalView::Bottom,
            ImageEdge::Left,
            WorldAxis::X,
            AxisSide::Min,
        ),
        (
            CanonicalView::Bottom,
            ImageEdge::Right,
            WorldAxis::X,
            AxisSide::Max,
        ),
        (
            CanonicalView::Bottom,
            ImageEdge::Top,
            WorldAxis::Z,
            AxisSide::Max,
        ),
        (
            CanonicalView::Bottom,
            ImageEdge::Bottom,
            WorldAxis::Z,
            AxisSide::Min,
        ),
    ];

    for (view, image, axis, side) in expected {
        assert_eq!(view.world_edge(image), WorldEdge { axis, side });
        assert_eq!(view.image_edge(WorldEdge { axis, side }), Some(image));
    }
}

#[test]
fn world_edges_perpendicular_to_a_view_have_no_local_image_edge() {
    assert_eq!(
        CanonicalView::Front.image_edge(WorldEdge {
            axis: WorldAxis::Z,
            side: AxisSide::Min,
        }),
        None
    );
    assert_eq!(
        CanonicalView::Left.image_edge(WorldEdge {
            axis: WorldAxis::X,
            side: AxisSide::Max,
        }),
        None
    );
    assert_eq!(
        CanonicalView::Top.image_edge(WorldEdge {
            axis: WorldAxis::Y,
            side: AxisSide::Min,
        }),
        None
    );
}

#[test]
fn front_left_addition_rebuilds_every_x_plane_on_the_mirrored_local_edge() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let left = chart(CanonicalView::Left, 2, 2, 30);
    let right = chart(CanonicalView::Right, 2, 2, 40);
    let mut model = AuthoredModel::new(
        bounds,
        vec![
            chart(CanonicalView::Front, 2, 2, 10),
            chart(CanonicalView::Back, 2, 2, 20),
            left.clone(),
            right.clone(),
            chart(CanonicalView::Top, 2, 2, 50),
            chart(CanonicalView::Bottom, 2, 2, 60),
        ],
    )
    .unwrap();

    model
        .resize(
            ResizeRequest {
                view: CanonicalView::Front,
                edge: ImageEdge::Left,
                delta: ResizeDelta::Add,
            },
            DiscardPolicy::Reject,
        )
        .unwrap();

    assert_eq!(model.bounds(), Bounds::new(3, 2, 2).unwrap());
    assert_eq!(
        model.chart(CanonicalView::Front).unwrap().rgba(),
        &[
            EMPTY_RGBA,
            pixel(10),
            pixel(11),
            EMPTY_RGBA,
            pixel(12),
            pixel(13)
        ]
    );
    assert_eq!(
        model.chart(CanonicalView::Back).unwrap().rgba(),
        &[
            pixel(20),
            pixel(21),
            EMPTY_RGBA,
            pixel(22),
            pixel(23),
            EMPTY_RGBA
        ]
    );
    assert_eq!(
        model.chart(CanonicalView::Top).unwrap().rgba(),
        &[
            EMPTY_RGBA,
            pixel(50),
            pixel(51),
            EMPTY_RGBA,
            pixel(52),
            pixel(53)
        ]
    );
    assert_eq!(
        model.chart(CanonicalView::Bottom).unwrap().rgba(),
        &[
            EMPTY_RGBA,
            pixel(60),
            pixel(61),
            EMPTY_RGBA,
            pixel(62),
            pixel(63)
        ]
    );
    assert_eq!(model.chart(CanonicalView::Left).unwrap(), &left);
    assert_eq!(model.chart(CanonicalView::Right).unwrap(), &right);
}

#[test]
fn removal_rejects_all_nonempty_affected_edges_without_mutation() {
    let bounds = Bounds::new(3, 2, 2).unwrap();
    let mut model = AuthoredModel::new(
        bounds,
        vec![
            chart(CanonicalView::Front, 3, 2, 10),
            chart(CanonicalView::Back, 3, 2, 20),
            chart(CanonicalView::Left, 2, 2, 30),
            chart(CanonicalView::Right, 2, 2, 40),
            chart(CanonicalView::Top, 3, 2, 50),
            chart(CanonicalView::Bottom, 3, 2, 60),
        ],
    )
    .unwrap();
    let before = model.clone();

    assert_eq!(
        model.resize(
            ResizeRequest {
                view: CanonicalView::Front,
                edge: ImageEdge::Left,
                delta: ResizeDelta::Remove,
            },
            DiscardPolicy::Reject,
        ),
        Err(ModelError::ResizeWouldDiscard {
            edges: vec![
                ChartEdge {
                    view: CanonicalView::Front,
                    edge: ImageEdge::Left,
                },
                ChartEdge {
                    view: CanonicalView::Back,
                    edge: ImageEdge::Right,
                },
                ChartEdge {
                    view: CanonicalView::Top,
                    edge: ImageEdge::Left,
                },
                ChartEdge {
                    view: CanonicalView::Bottom,
                    edge: ImageEdge::Left,
                },
            ],
        })
    );
    assert_eq!(model, before);
}

#[test]
fn allowed_removal_drops_exactly_the_mirrored_affected_edge_pixels() {
    let bounds = Bounds::new(3, 2, 2).unwrap();
    let left = chart(CanonicalView::Left, 2, 2, 30);
    let right = chart(CanonicalView::Right, 2, 2, 40);
    let mut model = AuthoredModel::new(
        bounds,
        vec![
            chart(CanonicalView::Front, 3, 2, 10),
            chart(CanonicalView::Back, 3, 2, 20),
            left.clone(),
            right.clone(),
            chart(CanonicalView::Top, 3, 2, 50),
            chart(CanonicalView::Bottom, 3, 2, 60),
        ],
    )
    .unwrap();

    model
        .resize(
            ResizeRequest {
                view: CanonicalView::Front,
                edge: ImageEdge::Left,
                delta: ResizeDelta::Remove,
            },
            DiscardPolicy::Allow,
        )
        .unwrap();

    assert_eq!(model.bounds(), Bounds::new(2, 2, 2).unwrap());
    assert_eq!(
        model.chart(CanonicalView::Front).unwrap().rgba(),
        &[pixel(11), pixel(12), pixel(14), pixel(15)]
    );
    assert_eq!(
        model.chart(CanonicalView::Back).unwrap().rgba(),
        &[pixel(20), pixel(21), pixel(23), pixel(24)]
    );
    assert_eq!(
        model.chart(CanonicalView::Top).unwrap().rgba(),
        &[pixel(51), pixel(52), pixel(54), pixel(55)]
    );
    assert_eq!(
        model.chart(CanonicalView::Bottom).unwrap().rgba(),
        &[pixel(61), pixel(62), pixel(64), pixel(65)]
    );
    assert_eq!(model.chart(CanonicalView::Left).unwrap(), &left);
    assert_eq!(model.chart(CanonicalView::Right).unwrap(), &right);
}

#[test]
fn resize_rejects_prospective_perpendicular_relief_without_mutation() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = Chart::from_rgba(CanonicalView::Front, 2, 2, vec![[1, 2, 3, 247]; 4]).unwrap();
    let mut model = AuthoredModel::new(bounds, vec![front]).unwrap();
    let before = model.clone();

    assert!(matches!(
        model.resize(
            ResizeRequest {
                view: CanonicalView::Right,
                edge: ImageEdge::Left,
                delta: ResizeDelta::Remove,
            },
            DiscardPolicy::Reject,
        ),
        Err(ModelError::ReliefBeyondMaximum {
            view: CanonicalView::Front,
            actual: 8,
            maximum: 4,
            ..
        })
    ));
    assert_eq!(model, before);
}

#[test]
fn resize_preserves_the_one_through_sixty_three_bound_limits() {
    for (bounds, delta) in [
        (Bounds::new(1, 2, 2).unwrap(), ResizeDelta::Remove),
        (Bounds::new(63, 2, 2).unwrap(), ResizeDelta::Add),
    ] {
        let mut model =
            AuthoredModel::new(bounds, vec![empty_chart(bounds, CanonicalView::Front)]).unwrap();
        let before = model.clone();

        assert!(matches!(
            model.resize(
                ResizeRequest {
                    view: CanonicalView::Front,
                    edge: ImageEdge::Left,
                    delta,
                },
                DiscardPolicy::Allow,
            ),
            Err(ModelError::Chart(ChartError::BoundsOutOfRange { .. }))
        ));
        assert_eq!(model, before);
    }
}

#[test]
fn reassignment_rejects_an_occupied_target_without_mutation() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let model = AuthoredModel::new(
        bounds,
        vec![
            chart(CanonicalView::Front, 2, 3, 10),
            chart(CanonicalView::Back, 2, 3, 20),
        ],
    )
    .unwrap();

    for mode in [ReassignMode::Preserve, ReassignMode::RecreateEmpty] {
        let mut attempted = model.clone();
        assert_eq!(
            attempted.reassign_chart(CanonicalView::Front, CanonicalView::Back, mode),
            Err(ModelError::DuplicateView(CanonicalView::Back))
        );
        assert_eq!(attempted, model);
    }
}

#[test]
fn preserve_reassignment_retains_exact_rgba_when_dimensions_match() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let pixels = chart(CanonicalView::Front, 2, 3, 10).rgba().to_vec();
    let mut model = AuthoredModel::new(
        bounds,
        vec![
            Chart::from_rgba(CanonicalView::Front, 2, 3, pixels.clone())
                .unwrap()
                .with_opposite_assignment()
                .with_mirrored_opposite(),
        ],
    )
    .unwrap();

    model
        .reassign_chart(
            CanonicalView::Front,
            CanonicalView::Back,
            ReassignMode::Preserve,
        )
        .unwrap();

    assert!(model.chart(CanonicalView::Front).is_none());
    assert_eq!(model.charts().len(), 1);
    let reassigned = model.chart(CanonicalView::Back).unwrap();
    assert_eq!(reassigned.rgba(), pixels);
    assert!(reassigned.supplies_opposite());
    assert!(reassigned.mirrors_opposite());
}

#[test]
fn preserve_reassignment_rejects_dimension_mismatch_without_mutation() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let mut model = AuthoredModel::new(
        bounds,
        vec![
            chart(CanonicalView::Front, 2, 3, 10)
                .with_opposite_assignment()
                .with_mirrored_opposite(),
        ],
    )
    .unwrap();
    let before = model.clone();

    assert_eq!(
        model.reassign_chart(
            CanonicalView::Front,
            CanonicalView::Left,
            ReassignMode::Preserve,
        ),
        Err(ModelError::DimensionMismatch {
            view: CanonicalView::Left,
            expected: (4, 3),
            actual: (2, 3),
        })
    );
    assert_eq!(model, before);
}

#[test]
fn recreate_reassignment_replaces_the_source_with_one_correctly_sized_empty_target() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let mut model = AuthoredModel::new(
        bounds,
        vec![
            chart(CanonicalView::Front, 2, 3, 10)
                .with_opposite_assignment()
                .with_mirrored_opposite(),
        ],
    )
    .unwrap();

    model
        .reassign_chart(
            CanonicalView::Front,
            CanonicalView::Left,
            ReassignMode::RecreateEmpty,
        )
        .unwrap();

    assert!(model.chart(CanonicalView::Front).is_none());
    assert_eq!(model.charts().len(), 1);
    let target = model.chart(CanonicalView::Left).unwrap();
    assert_eq!(target.dimensions(), (4, 3));
    assert_eq!(target.rgba(), &[EMPTY_RGBA; 12]);
    assert!(target.supplies_opposite());
    assert!(target.mirrors_opposite());
}
