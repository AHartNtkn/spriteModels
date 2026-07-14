use desktop_app::layout::{CANONICAL_SOURCE_ORDER, Size, calculate_layout, minimum_window_size};

fn assert_dominates_canvases(layout: &desktop_app::layout::WorkspaceLayout) {
    for card in &layout.source_cards {
        for canvas in [card.color, card.depth] {
            assert!(
                layout.model.width() >= canvas.width() * 3.0,
                "model width {} does not dominate canvas width {}",
                layout.model.width(),
                canvas.width()
            );
            assert!(
                layout.model.height() >= canvas.height() * 3.0,
                "model height {} does not dominate canvas height {}",
                layout.model.height(),
                canvas.height()
            );
        }
    }
}

#[test]
fn minimum_native_size_is_the_approved_workspace_size() {
    assert_eq!(minimum_window_size(), Size::new(1600.0, 1000.0));

    let layout = calculate_layout(minimum_window_size());
    assert_eq!(layout.window, layout.menu.union(layout.workspace));
    assert_dominates_canvases(&layout);
}

#[test]
fn menu_is_a_top_strip_above_one_uninterrupted_workspace() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0));

    assert_eq!(layout.menu.top(), 0.0);
    assert_eq!(layout.menu.left(), 0.0);
    assert_eq!(layout.menu.right(), layout.window.right());
    assert_eq!(layout.workspace.top(), layout.menu.bottom());
    assert_eq!(layout.workspace.bottom(), layout.window.bottom());
}

#[test]
fn tools_are_a_narrow_vertical_column_and_model_is_dominant() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0));

    assert!(layout.tools.height() > layout.tools.width() * 10.0);
    assert!(layout.tools.width() < layout.model.width() / 10.0);
    assert!(layout.model.width() > layout.sources.width());
    assert_dominates_canvases(&layout);
}

#[test]
fn source_cards_form_the_canonical_three_by_two_grid() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0));

    assert_eq!(layout.source_cards.len(), 6);
    assert_eq!(
        layout.source_cards.map(|card| card.view),
        CANONICAL_SOURCE_ORDER
    );
    assert_eq!(
        layout.source_cards.map(|card| (card.column, card.row)),
        [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]
    );

    for row in 0..2 {
        let cards: Vec<_> = layout
            .source_cards
            .iter()
            .filter(|card| card.row == row)
            .collect();
        assert_eq!(cards.len(), 3);
        assert!(cards[0].card.right() < cards[1].card.left());
        assert!(cards[1].card.right() < cards[2].card.left());
    }
    assert!(layout.source_cards[0].card.bottom() < layout.source_cards[3].card.top());
}

#[test]
fn every_card_stacks_equal_color_and_depth_canvases() {
    let layout = calculate_layout(Size::new(1600.0, 1000.0));

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
    let minimum = calculate_layout(minimum_window_size());
    let wider = calculate_layout(Size::new(1800.0, minimum_window_size().height));

    assert_eq!(wider.model.width(), minimum.model.width() + 200.0);
    for (before, after) in minimum.source_cards.iter().zip(&wider.source_cards) {
        assert_eq!(after.color.size(), before.color.size());
        assert_eq!(after.depth.size(), before.depth.size());
    }
    assert_dominates_canvases(&wider);
}
