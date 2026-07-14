use desktop_app::layout::{
    ADD_BUTTON_WIDTH, CANVAS_GAP, CANVASES_PER_SOURCE, LayoutError, MENU_HEIGHT, MIN_CANVAS_HEIGHT,
    MIN_CANVAS_WIDTH, MODEL_TO_CANVAS_RATIO, PANEL_GAP, SOURCE_ACTION_GAP, SOURCE_ACTION_HEIGHT,
    SOURCE_CARD_GAP, SOURCE_CARD_PADDING, SOURCE_HEADER_HEIGHT, SOURCE_SLOT_COUNT, Size,
    TOOL_COLUMN_WIDTH, WORKSPACE_PADDING, calculate_layout, minimum_model_size,
    minimum_source_card_size, minimum_source_grid_size, minimum_window_size,
};

fn repeated(count: usize, extent: f32, gap: f32) -> f32 {
    count as f32 * extent + (count - 1) as f32 * gap
}

fn expected_minimum_card() -> Size {
    Size::new(
        MIN_CANVAS_WIDTH + SOURCE_CARD_PADDING * 2.0,
        SOURCE_CARD_PADDING * 2.0
            + SOURCE_HEADER_HEIGHT
            + MIN_CANVAS_HEIGHT * CANVASES_PER_SOURCE as f32
            + CANVAS_GAP,
    )
}

fn assert_size_near(actual: Size, expected: Size) {
    assert!((actual.width - expected.width).abs() < 0.01);
    assert!((actual.height - expected.height).abs() < 0.01);
}

fn assert_model_dominates(layout: &desktop_app::layout::WorkspaceLayout) {
    for card in &layout.source_cards {
        assert!(layout.model.width() >= card.color.width() * MODEL_TO_CANVAS_RATIO - 0.01);
        assert!(layout.model.height() >= card.color.height() * MODEL_TO_CANVAS_RATIO - 0.01);
        assert_size_near(card.color.size(), card.depth.size());
        assert_eq!(card.color.left(), card.depth.left());
        assert!(card.color.bottom() < card.depth.top());
        assert!(card.card.contains_rect(card.color));
        assert!(card.card.contains_rect(card.depth));
    }
}

#[test]
fn minimum_size_is_derived_from_the_two_by_three_minimum_grid() {
    let card = expected_minimum_card();
    assert_eq!(minimum_source_card_size(), card);
    let sources = Size::new(
        repeated(2, card.width, SOURCE_CARD_GAP),
        repeated(3, card.height, SOURCE_CARD_GAP),
    );
    assert_eq!(minimum_source_grid_size(6), sources);
    let model = minimum_model_size();
    let expected = Size::new(
        WORKSPACE_PADDING * 2.0 + TOOL_COLUMN_WIDTH + PANEL_GAP * 2.0 + model.width + sources.width,
        MENU_HEIGHT + WORKSPACE_PADDING * 2.0 + sources.height.max(model.height),
    );
    assert_eq!(minimum_window_size(), expected);

    let layout = calculate_layout(expected, 6).unwrap();
    assert_eq!(layout.source_cards.len(), 6);
    assert!(layout.add_button.is_none());
    for card in &layout.source_cards {
        assert_eq!(
            card.color.size(),
            Size::new(MIN_CANVAS_WIDTH, MIN_CANVAS_HEIGHT)
        );
    }
    assert_model_dominates(&layout);
}

#[test]
fn counts_one_through_six_pack_into_two_columns_and_three_rows() {
    for (count, columns, rows) in [
        (1, 1, 1),
        (2, 2, 1),
        (3, 2, 2),
        (4, 2, 2),
        (5, 2, 3),
        (6, 2, 3),
    ] {
        let layout = calculate_layout(Size::new(1600.0, 1000.0), count).unwrap();
        assert_eq!(layout.source_cards.len(), count);
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
        assert_eq!(layout.add_button.is_some(), count < SOURCE_SLOT_COUNT);
        assert_model_dominates(&layout);
    }
}

#[test]
fn two_source_fullscreen_canvases_use_the_available_workspace() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0), 2).unwrap();
    let first = layout.source_cards[0];
    assert!(first.color.width() > MIN_CANVAS_WIDTH * 2.0);
    assert!(first.color.height() > MIN_CANVAS_HEIGHT * 3.0);
    assert!(first.card.height() > layout.model.height() / 2.0);
    assert_eq!(
        layout
            .source_cards
            .iter()
            .map(|card| (card.column, card.row))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 0)]
    );
    assert_model_dominates(&layout);
}

#[test]
fn six_sources_resize_to_fill_three_rows_without_overlap() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0), 6).unwrap();
    assert_eq!(
        layout
            .source_cards
            .iter()
            .map(|card| (card.column, card.row))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)]
    );
    for pair in layout.source_cards.windows(2) {
        if pair[0].row == pair[1].row {
            assert_eq!(pair[1].card.left() - pair[0].card.right(), SOURCE_CARD_GAP);
        }
    }
    assert!(layout.sources.contains_rect(layout.source_cards[5].card));
    assert_model_dominates(&layout);
}

#[test]
fn compact_add_stays_above_and_outside_responsive_cards() {
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
fn window_width_and_height_resize_canvases_and_model_from_the_same_constraints() {
    let base = calculate_layout(Size::new(1600.0, 1000.0), 6).unwrap();
    let wider = calculate_layout(Size::new(1800.0, 1000.0), 6).unwrap();
    let taller = calculate_layout(Size::new(1600.0, 1200.0), 6).unwrap();
    assert!(wider.source_cards[0].color.width() > base.source_cards[0].color.width());
    assert!(wider.model.width() > base.model.width());
    assert_eq!(
        wider.source_cards[0].color.height(),
        base.source_cards[0].color.height()
    );
    assert!(taller.source_cards[0].color.height() > base.source_cards[0].color.height());
    assert_eq!(
        taller.source_cards[0].color.width(),
        base.source_cards[0].color.width()
    );
    assert_model_dominates(&wider);
    assert_model_dominates(&taller);
}

#[test]
fn menu_tools_model_and_sources_remain_nonoverlapping() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0), 2).unwrap();
    assert_eq!(layout.menu.top(), 0.0);
    assert_eq!(layout.workspace.top(), layout.menu.bottom());
    assert_eq!(layout.tools.width(), TOOL_COLUMN_WIDTH);
    assert_eq!(layout.model.left() - layout.tools.right(), PANEL_GAP);
    assert_eq!(layout.sources.left() - layout.model.right(), PANEL_GAP);
    assert!(layout.model.width() > layout.sources.width());
}

#[test]
fn each_axis_below_the_derived_minimum_is_rejected() {
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
