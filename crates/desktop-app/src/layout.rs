use relief_core::CanonicalView;

pub const MIN_WINDOW_WIDTH: f32 = 1600.0;
pub const MIN_WINDOW_HEIGHT: f32 = 1000.0;
pub const MENU_HEIGHT: f32 = 28.0;
pub const WORKSPACE_PADDING: f32 = 10.0;
pub const PANEL_GAP: f32 = 10.0;
pub const TOOL_COLUMN_WIDTH: f32 = 42.0;
pub const SOURCE_CARD_WIDTH: f32 = 150.0;
pub const SOURCE_CARD_HEIGHT: f32 = 216.0;
pub const SOURCE_CARD_GAP: f32 = 10.0;
pub const SOURCE_CARD_PADDING: f32 = 6.0;
pub const SOURCE_HEADER_HEIGHT: f32 = 18.0;
pub const CANVAS_HEIGHT: f32 = 90.0;
pub const CANVAS_GAP: f32 = 6.0;

pub const CANONICAL_SOURCE_ORDER: [CanonicalView; 6] = [
    CanonicalView::Front,
    CanonicalView::Right,
    CanonicalView::Top,
    CanonicalView::Back,
    CanonicalView::Left,
    CanonicalView::Bottom,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl Rect {
    pub const fn from_min_size(min_x: f32, min_y: f32, size: Size) -> Self {
        Self {
            min_x,
            min_y,
            max_x: min_x + size.width,
            max_y: min_y + size.height,
        }
    }

    pub const fn left(self) -> f32 {
        self.min_x
    }

    pub const fn top(self) -> f32 {
        self.min_y
    }

    pub const fn right(self) -> f32 {
        self.max_x
    }

    pub const fn bottom(self) -> f32 {
        self.max_y
    }

    pub const fn width(self) -> f32 {
        self.max_x - self.min_x
    }

    pub const fn height(self) -> f32 {
        self.max_y - self.min_y
    }

    pub const fn size(self) -> Size {
        Size::new(self.width(), self.height())
    }

    pub fn contains_rect(self, other: Self) -> bool {
        self.left() <= other.left()
            && self.top() <= other.top()
            && self.right() >= other.right()
            && self.bottom() >= other.bottom()
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            min_x: self.left().min(other.left()),
            min_y: self.top().min(other.top()),
            max_x: self.right().max(other.right()),
            max_y: self.bottom().max(other.bottom()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceCardLayout {
    pub view: CanonicalView,
    pub column: usize,
    pub row: usize,
    pub card: Rect,
    pub color: Rect,
    pub depth: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceLayout {
    pub window: Rect,
    pub menu: Rect,
    pub workspace: Rect,
    pub tools: Rect,
    pub model: Rect,
    pub sources: Rect,
    pub source_cards: [SourceCardLayout; 6],
}

pub const fn minimum_window_size() -> Size {
    Size::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
}

pub fn calculate_layout(window_size: Size) -> WorkspaceLayout {
    let minimum = minimum_window_size();
    assert!(window_size.width >= minimum.width);
    assert!(window_size.height >= minimum.height);

    let window = Rect::from_min_size(0.0, 0.0, window_size);
    let menu = Rect::from_min_size(0.0, 0.0, Size::new(window_size.width, MENU_HEIGHT));
    let workspace = Rect::from_min_size(
        0.0,
        MENU_HEIGHT,
        Size::new(window_size.width, window_size.height - MENU_HEIGHT),
    );
    let content_top = workspace.top() + WORKSPACE_PADDING;
    let content_height = workspace.height() - WORKSPACE_PADDING * 2.0;
    let tools = Rect::from_min_size(
        WORKSPACE_PADDING,
        content_top,
        Size::new(TOOL_COLUMN_WIDTH, content_height),
    );

    let sources_size = Size::new(
        SOURCE_CARD_WIDTH * 3.0 + SOURCE_CARD_GAP * 2.0,
        SOURCE_CARD_HEIGHT * 2.0 + SOURCE_CARD_GAP,
    );
    let sources = Rect::from_min_size(
        window.right() - WORKSPACE_PADDING - sources_size.width,
        content_top,
        sources_size,
    );
    let model_left = tools.right() + PANEL_GAP;
    let model = Rect::from_min_size(
        model_left,
        content_top,
        Size::new(sources.left() - PANEL_GAP - model_left, content_height),
    );

    let source_cards = std::array::from_fn(|index| {
        let column = index % 3;
        let row = index / 3;
        let card = Rect::from_min_size(
            sources.left() + column as f32 * (SOURCE_CARD_WIDTH + SOURCE_CARD_GAP),
            sources.top() + row as f32 * (SOURCE_CARD_HEIGHT + SOURCE_CARD_GAP),
            Size::new(SOURCE_CARD_WIDTH, SOURCE_CARD_HEIGHT),
        );
        let canvas_size = Size::new(SOURCE_CARD_WIDTH - SOURCE_CARD_PADDING * 2.0, CANVAS_HEIGHT);
        let color = Rect::from_min_size(
            card.left() + SOURCE_CARD_PADDING,
            card.top() + SOURCE_CARD_PADDING + SOURCE_HEADER_HEIGHT,
            canvas_size,
        );
        let depth = Rect::from_min_size(color.left(), color.bottom() + CANVAS_GAP, canvas_size);
        SourceCardLayout {
            view: CANONICAL_SOURCE_ORDER[index],
            column,
            row,
            card,
            color,
            depth,
        }
    });

    WorkspaceLayout {
        window,
        menu,
        workspace,
        tools,
        model,
        sources,
        source_cards,
    }
}
