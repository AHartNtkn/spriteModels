use desktop_app::layout::{
    CANONICAL_SOURCE_ORDER, CANVAS_GAP, CANVAS_HEIGHT, CANVAS_WIDTH, CANVASES_PER_SOURCE,
    LayoutError, MENU_HEIGHT, MODEL_TO_CANVAS_RATIO, PANEL_GAP, SOURCE_CARD_GAP,
    SOURCE_CARD_HEIGHT, SOURCE_CARD_PADDING, SOURCE_CARD_WIDTH, SOURCE_COLUMNS,
    SOURCE_HEADER_HEIGHT, SOURCE_ROWS, SOURCE_SLOT_COUNT, Size, TOOL_COLUMN_WIDTH,
    WORKSPACE_PADDING, calculate_layout, minimum_window_size,
};

fn independently_derived_card_size() -> Size {
    Size::new(
        CANVAS_WIDTH + SOURCE_CARD_PADDING * 2.0,
        SOURCE_CARD_PADDING * 2.0
            + SOURCE_HEADER_HEIGHT
            + CANVAS_HEIGHT * CANVASES_PER_SOURCE as f32
            + CANVAS_GAP,
    )
}

fn independently_derived_source_size() -> Size {
    let card = independently_derived_card_size();
    Size::new(
        SOURCE_COLUMNS as f32 * card.width + (SOURCE_COLUMNS - 1) as f32 * SOURCE_CARD_GAP,
        SOURCE_ROWS as f32 * card.height + (SOURCE_ROWS - 1) as f32 * SOURCE_CARD_GAP,
    )
}

fn independently_derived_model_minimum() -> Size {
    Size::new(
        MODEL_TO_CANVAS_RATIO * CANVAS_WIDTH,
        MODEL_TO_CANVAS_RATIO * CANVAS_HEIGHT,
    )
}

fn independently_derived_minimum() -> Size {
    let sources = independently_derived_source_size();
    let model = independently_derived_model_minimum();

    Size::new(
        WORKSPACE_PADDING * 2.0 + TOOL_COLUMN_WIDTH + PANEL_GAP * 2.0 + model.width + sources.width,
        MENU_HEIGHT + WORKSPACE_PADDING * 2.0 + sources.height.max(model.height),
    )
}

fn assert_dominates_canvases(layout: &desktop_app::layout::WorkspaceLayout) {
    for card in &layout.source_cards {
        for canvas in [card.color, card.depth] {
            assert!(
                layout.model.width() >= canvas.width() * MODEL_TO_CANVAS_RATIO,
                "model width {} does not dominate canvas width {}",
                layout.model.width(),
                canvas.width()
            );
            assert!(
                layout.model.height() >= canvas.height() * MODEL_TO_CANVAS_RATIO,
                "model height {} does not dominate canvas height {}",
                layout.model.height(),
                canvas.height()
            );
        }
    }
}

#[test]
fn minimum_native_size_is_derived_from_every_layout_relationship() {
    let expected_card = independently_derived_card_size();
    let expected_sources = independently_derived_source_size();
    let expected_model = independently_derived_model_minimum();
    let expected = independently_derived_minimum();
    assert_eq!(SOURCE_CARD_WIDTH, expected_card.width);
    assert_eq!(SOURCE_CARD_HEIGHT, expected_card.height);
    assert_eq!(minimum_window_size(), expected);

    let layout = calculate_layout(expected).unwrap();
    assert_eq!(layout.window, layout.menu.union(layout.workspace));
    assert_eq!(layout.menu.height(), MENU_HEIGHT);
    assert_eq!(layout.tools.width(), TOOL_COLUMN_WIDTH);
    assert_eq!(
        layout.tools.height(),
        expected_sources.height.max(expected_model.height)
    );
    assert_eq!(layout.model.width(), expected_model.width);
    assert_eq!(layout.model.height(), layout.tools.height());
    assert!(layout.model.height() >= expected_model.height);
    assert_eq!(layout.sources.size(), expected_sources);
    assert_eq!(layout.model.left() - layout.tools.right(), PANEL_GAP);
    assert_eq!(layout.sources.left() - layout.model.right(), PANEL_GAP);
    assert_eq!(layout.tools.left(), WORKSPACE_PADDING);
    assert_eq!(
        layout.window.right() - layout.sources.right(),
        WORKSPACE_PADDING
    );
    assert_eq!(
        layout.sources.top() - layout.menu.bottom(),
        WORKSPACE_PADDING
    );
    assert_eq!(
        layout.window.bottom() - layout.model.bottom(),
        WORKSPACE_PADDING
    );
    for card in &layout.source_cards {
        assert_eq!(card.card.size(), expected_card);
        assert_eq!(card.color.size(), Size::new(CANVAS_WIDTH, CANVAS_HEIGHT));
        assert_eq!(card.depth.size(), Size::new(CANVAS_WIDTH, CANVAS_HEIGHT));
        assert_eq!(card.color.left() - card.card.left(), SOURCE_CARD_PADDING);
        assert_eq!(
            card.color.top() - card.card.top(),
            SOURCE_CARD_PADDING + SOURCE_HEADER_HEIGHT
        );
        assert_eq!(card.depth.top() - card.color.bottom(), CANVAS_GAP);
        assert_eq!(
            card.card.bottom() - card.depth.bottom(),
            SOURCE_CARD_PADDING
        );
    }
    for row in 0..SOURCE_ROWS {
        for column in 1..SOURCE_COLUMNS {
            let previous = layout.source_cards[row * SOURCE_COLUMNS + column - 1];
            let current = layout.source_cards[row * SOURCE_COLUMNS + column];
            assert_eq!(current.card.left() - previous.card.right(), SOURCE_CARD_GAP);
        }
    }
    for row in 1..SOURCE_ROWS {
        for column in 0..SOURCE_COLUMNS {
            let previous = layout.source_cards[(row - 1) * SOURCE_COLUMNS + column];
            let current = layout.source_cards[row * SOURCE_COLUMNS + column];
            assert_eq!(current.card.top() - previous.card.bottom(), SOURCE_CARD_GAP);
        }
    }
    assert_dominates_canvases(&layout);
}

#[test]
fn each_axis_below_the_derived_minimum_is_explicitly_rejected() {
    let minimum = independently_derived_minimum();
    let too_narrow = Size::new(minimum.width - 1.0, minimum.height);
    let too_short = Size::new(minimum.width, minimum.height - 1.0);

    assert_eq!(
        calculate_layout(too_narrow),
        Err(LayoutError::WindowTooSmall {
            requested: too_narrow,
            minimum,
        })
    );
    assert_eq!(
        calculate_layout(too_short),
        Err(LayoutError::WindowTooSmall {
            requested: too_short,
            minimum,
        })
    );
}

#[test]
fn menu_is_a_top_strip_above_one_uninterrupted_workspace() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0)).unwrap();

    assert_eq!(layout.menu.top(), 0.0);
    assert_eq!(layout.menu.left(), 0.0);
    assert_eq!(layout.menu.right(), layout.window.right());
    assert_eq!(layout.workspace.top(), layout.menu.bottom());
    assert_eq!(layout.workspace.bottom(), layout.window.bottom());
}

#[test]
fn tools_are_a_narrow_vertical_column_and_model_is_dominant() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0)).unwrap();

    assert!(layout.tools.height() > layout.tools.width() * 5.0);
    assert!(layout.tools.width() < SOURCE_CARD_WIDTH);
    assert!(layout.model.width() > layout.sources.width());
    assert_dominates_canvases(&layout);
}

#[test]
fn source_cards_form_the_canonical_three_by_two_grid() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0)).unwrap();

    assert_eq!(layout.source_cards.len(), SOURCE_SLOT_COUNT);
    assert_eq!(
        layout.source_cards.map(|card| card.view),
        CANONICAL_SOURCE_ORDER
    );
    assert_eq!(
        layout.source_cards.map(|card| (card.column, card.row)),
        [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]
    );

    for row in 0..SOURCE_ROWS {
        let cards: Vec<_> = layout
            .source_cards
            .iter()
            .filter(|card| card.row == row)
            .collect();
        assert_eq!(cards.len(), SOURCE_COLUMNS);
        for pair in cards.windows(2) {
            assert!(pair[0].card.right() < pair[1].card.left());
        }
    }
    assert!(layout.source_cards[0].card.bottom() < layout.source_cards[SOURCE_COLUMNS].card.top());
}

#[test]
fn every_card_stacks_equal_color_and_depth_canvases() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0)).unwrap();

    for card in &layout.source_cards {
        assert_eq!(card.color.size(), card.depth.size());
        assert_eq!(card.color.left(), card.depth.left());
        assert!(card.color.bottom() < card.depth.top());
        assert!(card.card.contains_rect(card.color));
        assert!(card.card.contains_rect(card.depth));
    }
}

#[test]
fn extra_width_grows_the_model_before_source_canvases() {
    let minimum = calculate_layout(Size::new(1600.0, 1000.0)).unwrap();
    let wider = calculate_layout(Size::new(1800.0, 1000.0)).unwrap();

    assert_eq!(wider.model.width(), minimum.model.width() + 200.0);
    for (before, after) in minimum.source_cards.iter().zip(&wider.source_cards) {
        assert_eq!(after.color.size(), before.color.size());
        assert_eq!(after.depth.size(), before.depth.size());
    }
    assert_dominates_canvases(&wider);
}
