use desktop_app::layout::{
    ADD_BUTTON_WIDTH, CANVAS_GAP, CANVAS_HEIGHT, CANVAS_WIDTH, CANVASES_PER_SOURCE, LayoutError,
    MENU_HEIGHT, MODEL_TO_CANVAS_RATIO, PANEL_GAP, SOURCE_ACTION_GAP, SOURCE_ACTION_HEIGHT,
    SOURCE_CARD_GAP, SOURCE_CARD_HEIGHT, SOURCE_CARD_PADDING, SOURCE_CARD_WIDTH, SOURCE_COLUMNS,
    SOURCE_HEADER_HEIGHT, SOURCE_SLOT_COUNT, Size, TOOL_COLUMN_WIDTH, WORKSPACE_PADDING,
    calculate_layout, minimum_window_size, source_grid_size,
};

fn card_size() -> Size {
    Size::new(
        CANVAS_WIDTH + SOURCE_CARD_PADDING * 2.0,
        SOURCE_CARD_PADDING * 2.0
            + SOURCE_HEADER_HEIGHT
            + CANVAS_HEIGHT * CANVASES_PER_SOURCE as f32
            + CANVAS_GAP,
    )
}

fn repeated(count: usize, extent: f32, gap: f32) -> f32 {
    count as f32 * extent + (count - 1) as f32 * gap
}

fn expected_source_size(count: usize) -> Size {
    let columns = count.min(SOURCE_COLUMNS);
    let rows = count.div_ceil(SOURCE_COLUMNS);
    let action = if count < SOURCE_SLOT_COUNT {
        SOURCE_ACTION_HEIGHT + SOURCE_ACTION_GAP
    } else {
        0.0
    };
    Size::new(
        repeated(columns, SOURCE_CARD_WIDTH, SOURCE_CARD_GAP),
        action + repeated(rows, SOURCE_CARD_HEIGHT, SOURCE_CARD_GAP),
    )
}

fn assert_dominates_canvases(layout: &desktop_app::layout::WorkspaceLayout) {
    for card in &layout.source_cards {
        for canvas in [card.color, card.depth] {
            assert!(layout.model.width() >= canvas.width() * MODEL_TO_CANVAS_RATIO);
            assert!(layout.model.height() >= canvas.height() * MODEL_TO_CANVAS_RATIO);
        }
    }
}

#[test]
fn minimum_native_size_is_derived_from_the_six_source_maximum() {
    assert_eq!(SOURCE_CARD_WIDTH, card_size().width);
    assert_eq!(SOURCE_CARD_HEIGHT, card_size().height);
    assert_eq!(source_grid_size(6), expected_source_size(6));

    let sources = expected_source_size(6);
    let model = Size::new(
        MODEL_TO_CANVAS_RATIO * CANVAS_WIDTH,
        MODEL_TO_CANVAS_RATIO * CANVAS_HEIGHT,
    );
    let expected = Size::new(
        WORKSPACE_PADDING * 2.0 + TOOL_COLUMN_WIDTH + PANEL_GAP * 2.0 + model.width + sources.width,
        MENU_HEIGHT + WORKSPACE_PADDING * 2.0 + sources.height.max(model.height),
    );
    assert_eq!(minimum_window_size(), expected);

    let layout = calculate_layout(expected, 6).unwrap();
    assert_eq!(layout.source_cards.len(), 6);
    assert!(layout.add_button.is_none());
    assert_eq!(layout.sources.size(), sources);
    assert_eq!(layout.model.width(), model.width);
    assert_dominates_canvases(&layout);
}

#[test]
fn authored_counts_pack_into_only_the_columns_and_rows_they_need() {
    for (count, columns, rows) in [
        (1, 1, 1),
        (2, 2, 1),
        (3, 3, 1),
        (4, 3, 2),
        (5, 3, 2),
        (6, 3, 2),
    ] {
        let layout = calculate_layout(Size::new(1600.0, 1000.0), count).unwrap();
        assert_eq!(layout.source_cards.len(), count);
        assert_eq!(layout.sources.size(), expected_source_size(count));
        assert_eq!(
            layout
                .source_cards
                .iter()
                .map(|card| card.column)
                .max()
                .unwrap()
                + 1,
            columns
        );
        assert_eq!(
            layout
                .source_cards
                .iter()
                .map(|card| card.row)
                .max()
                .unwrap()
                + 1,
            rows
        );
        assert_eq!(layout.add_button.is_some(), count < 6);
        assert_dominates_canvases(&layout);
    }
}

#[test]
fn add_sprite_is_a_compact_action_outside_every_source_card() {
    for count in 1..6 {
        let layout = calculate_layout(Size::new(1600.0, 1000.0), count).unwrap();
        let add = layout.add_button.unwrap();
        assert_eq!(
            add.size(),
            Size::new(ADD_BUTTON_WIDTH, SOURCE_ACTION_HEIGHT)
        );
        assert!(layout.sources.contains_rect(add));
        for card in &layout.source_cards {
            assert!(!add.intersects(card.card));
            assert!(add.bottom() + SOURCE_ACTION_GAP <= card.card.top());
        }
    }
}

#[test]
fn source_cards_pack_consecutively_with_color_above_depth() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0), 5).unwrap();
    assert_eq!(
        layout
            .source_cards
            .iter()
            .map(|card| (card.column, card.row))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1)]
    );
    for card in &layout.source_cards {
        assert_eq!(card.card.size(), card_size());
        assert_eq!(card.color.size(), Size::new(CANVAS_WIDTH, CANVAS_HEIGHT));
        assert_eq!(card.color.size(), card.depth.size());
        assert_eq!(card.color.left(), card.depth.left());
        assert!(card.color.bottom() < card.depth.top());
        assert!(card.card.contains_rect(card.color));
        assert!(card.card.contains_rect(card.depth));
    }
}

#[test]
fn fewer_source_columns_return_the_space_to_the_model() {
    let one = calculate_layout(Size::new(1600.0, 1000.0), 1).unwrap();
    let two = calculate_layout(Size::new(1600.0, 1000.0), 2).unwrap();
    let three = calculate_layout(Size::new(1600.0, 1000.0), 3).unwrap();
    assert!(one.model.width() > two.model.width());
    assert!(two.model.width() > three.model.width());
    assert_eq!(
        one.model.width() - two.model.width(),
        SOURCE_CARD_WIDTH + SOURCE_CARD_GAP
    );
    assert_eq!(
        two.model.width() - three.model.width(),
        SOURCE_CARD_WIDTH + SOURCE_CARD_GAP
    );
}

#[test]
fn menu_tools_model_and_sources_remain_one_nonoverlapping_workspace() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0), 2).unwrap();
    assert_eq!(layout.menu.top(), 0.0);
    assert_eq!(layout.menu.right(), layout.window.right());
    assert_eq!(layout.workspace.top(), layout.menu.bottom());
    assert_eq!(layout.tools.width(), TOOL_COLUMN_WIDTH);
    assert_eq!(layout.model.left() - layout.tools.right(), PANEL_GAP);
    assert_eq!(layout.sources.left() - layout.model.right(), PANEL_GAP);
    assert!(layout.model.width() > layout.sources.width());
    assert_dominates_canvases(&layout);
}

#[test]
fn each_axis_below_the_six_source_minimum_is_rejected() {
    let minimum = minimum_window_size();
    for requested in [
        Size::new(minimum.width - 1.0, minimum.height),
        Size::new(minimum.width, minimum.height - 1.0),
    ] {
        assert_eq!(
            calculate_layout(requested, 6),
            Err(LayoutError::WindowTooSmall { requested, minimum })
        );
    }
}

#[test]
fn extra_window_width_grows_only_the_model() {
    let before = calculate_layout(Size::new(1600.0, 1000.0), 2).unwrap();
    let after = calculate_layout(Size::new(1800.0, 1000.0), 2).unwrap();
    assert_eq!(after.model.width(), before.model.width() + 200.0);
    assert_eq!(after.sources.size(), before.sources.size());
    for (before, after) in before.source_cards.iter().zip(&after.source_cards) {
        assert_eq!(after.color.size(), before.color.size());
        assert_eq!(after.depth.size(), before.depth.size());
    }
}
