use relief_core::CanonicalView;

pub const MENU_HEIGHT: f32 = 28.0;
pub const WORKSPACE_PADDING: f32 = 10.0;
pub const PANEL_GAP: f32 = 10.0;
pub const TOOL_COLUMN_WIDTH: f32 = 100.0;
pub const SOURCE_COLUMNS: usize = 2;
pub const SOURCE_ROWS: usize = 3;
pub const SOURCE_SLOT_COUNT: usize = SOURCE_COLUMNS * SOURCE_ROWS;
pub const CANVASES_PER_SOURCE: usize = 2;
pub const SOURCE_CARD_GAP: f32 = 10.0;
pub const SOURCE_CARD_PADDING: f32 = 6.0;
pub const SOURCE_HEADER_HEIGHT: f32 = 18.0;
pub const SOURCE_ACTION_HEIGHT: f32 = 28.0;
pub const SOURCE_ACTION_GAP: f32 = 6.0;
pub const ADD_BUTTON_WIDTH: f32 = 100.0;
pub const MIN_CANVAS_WIDTH: f32 = 138.0;
pub const MIN_CANVAS_HEIGHT: f32 = 90.0;
pub const CANVAS_GAP: f32 = 6.0;
pub const MODEL_TO_CANVAS_RATIO: f32 = 3.0;

const SOURCE_CARD_WIDTH_CHROME: f32 = SOURCE_CARD_PADDING * 2.0;
const SOURCE_CARD_HEIGHT_CHROME: f32 =
    SOURCE_CARD_PADDING * 2.0 + SOURCE_HEADER_HEIGHT + CANVAS_GAP;

pub const CANONICAL_SOURCE_ORDER: [CanonicalView; SOURCE_SLOT_COUNT] = [
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

    pub fn intersects(self, other: Self) -> bool {
        self.left() < other.right()
            && self.right() > other.left()
            && self.top() < other.bottom()
            && self.bottom() > other.top()
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
    pub source_cards: Vec<SourceCardLayout>,
    pub add_button: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutError {
    WindowTooSmall { requested: Size, minimum: Size },
}

pub const fn minimum_source_grid_size(authored_count: usize) -> Size {
    let columns = minimum(authored_count, SOURCE_COLUMNS);
    let rows = authored_count.div_ceil(SOURCE_COLUMNS);
    let action = if authored_count < SOURCE_SLOT_COUNT {
        SOURCE_ACTION_HEIGHT + SOURCE_ACTION_GAP
    } else {
        0.0
    };
    let card = minimum_source_card_size();
    Size::new(
        repeated_extent(columns, card.width, SOURCE_CARD_GAP),
        action + repeated_extent(rows, card.height, SOURCE_CARD_GAP),
    )
}

pub const fn minimum_source_card_size() -> Size {
    Size::new(
        MIN_CANVAS_WIDTH + SOURCE_CARD_WIDTH_CHROME,
        MIN_CANVAS_HEIGHT * CANVASES_PER_SOURCE as f32 + SOURCE_CARD_HEIGHT_CHROME,
    )
}

pub const fn minimum_model_size() -> Size {
    Size::new(
        MIN_CANVAS_WIDTH * MODEL_TO_CANVAS_RATIO,
        MIN_CANVAS_HEIGHT * MODEL_TO_CANVAS_RATIO,
    )
}

pub const fn minimum_window_size() -> Size {
    let sources = minimum_source_grid_size(SOURCE_SLOT_COUNT);
    let model = minimum_model_size();
    Size::new(
        WORKSPACE_PADDING * 2.0 + TOOL_COLUMN_WIDTH + PANEL_GAP * 2.0 + model.width + sources.width,
        MENU_HEIGHT + WORKSPACE_PADDING * 2.0 + maximum(model.height, sources.height),
    )
}

pub fn calculate_layout(
    window_size: Size,
    authored_count: usize,
) -> Result<WorkspaceLayout, LayoutError> {
    assert!((1..=SOURCE_SLOT_COUNT).contains(&authored_count));
    let minimum_size = minimum_window_size();
    if !(window_size.width >= minimum_size.width && window_size.height >= minimum_size.height) {
        return Err(LayoutError::WindowTooSmall {
            requested: window_size,
            minimum: minimum_size,
        });
    }

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

    let columns = minimum(authored_count, SOURCE_COLUMNS);
    let rows = authored_count.div_ceil(SOURCE_COLUMNS);
    let action_height = if authored_count < SOURCE_SLOT_COUNT {
        SOURCE_ACTION_HEIGHT + SOURCE_ACTION_GAP
    } else {
        0.0
    };
    let available_width =
        window_size.width - WORKSPACE_PADDING * 2.0 - TOOL_COLUMN_WIDTH - PANEL_GAP * 2.0;
    let fixed_source_width =
        columns as f32 * SOURCE_CARD_WIDTH_CHROME + (columns - 1) as f32 * SOURCE_CARD_GAP;
    let canvas_width = ((available_width - fixed_source_width)
        / (columns as f32 + MODEL_TO_CANVAS_RATIO))
        .max(MIN_CANVAS_WIDTH);
    let card_width = canvas_width + SOURCE_CARD_WIDTH_CHROME;
    let sources_width = repeated_extent(columns, card_width, SOURCE_CARD_GAP);
    let model_width = available_width - sources_width;

    let grid_height = content_height - action_height;
    let fixed_grid_height =
        rows as f32 * SOURCE_CARD_HEIGHT_CHROME + (rows - 1) as f32 * SOURCE_CARD_GAP;
    let canvas_height_for_rows =
        (grid_height - fixed_grid_height) / (rows * CANVASES_PER_SOURCE) as f32;
    let canvas_height = canvas_height_for_rows
        .min(content_height / MODEL_TO_CANVAS_RATIO)
        .max(MIN_CANVAS_HEIGHT);
    let card_height = canvas_height * CANVASES_PER_SOURCE as f32 + SOURCE_CARD_HEIGHT_CHROME;
    let sources_height = action_height + repeated_extent(rows, card_height, SOURCE_CARD_GAP);
    let sources_size = Size::new(sources_width, sources_height);
    let sources = Rect::from_min_size(
        window.right() - WORKSPACE_PADDING - sources_size.width,
        content_top,
        sources_size,
    );
    let model_left = tools.right() + PANEL_GAP;
    let model = Rect::from_min_size(
        model_left,
        content_top,
        Size::new(model_width, content_height),
    );

    let card_top = sources.top()
        + if authored_count < SOURCE_SLOT_COUNT {
            SOURCE_ACTION_HEIGHT + SOURCE_ACTION_GAP
        } else {
            0.0
        };
    let source_cards = (0..authored_count)
        .map(|index| {
            let column = index % SOURCE_COLUMNS;
            let row = index / SOURCE_COLUMNS;
            let card = Rect::from_min_size(
                sources.left() + column as f32 * (card_width + SOURCE_CARD_GAP),
                card_top + row as f32 * (card_height + SOURCE_CARD_GAP),
                Size::new(card_width, card_height),
            );
            let canvas_size = Size::new(canvas_width, canvas_height);
            let color = Rect::from_min_size(
                card.left() + SOURCE_CARD_PADDING,
                card.top() + SOURCE_CARD_PADDING + SOURCE_HEADER_HEIGHT,
                canvas_size,
            );
            let depth = Rect::from_min_size(color.left(), color.bottom() + CANVAS_GAP, canvas_size);
            SourceCardLayout {
                column,
                row,
                card,
                color,
                depth,
            }
        })
        .collect();
    let add_button = (authored_count < SOURCE_SLOT_COUNT).then(|| {
        Rect::from_min_size(
            sources.right() - ADD_BUTTON_WIDTH,
            sources.top(),
            Size::new(ADD_BUTTON_WIDTH, SOURCE_ACTION_HEIGHT),
        )
    });

    Ok(WorkspaceLayout {
        window,
        menu,
        workspace,
        tools,
        model,
        sources,
        source_cards,
        add_button,
    })
}

const fn repeated_extent(count: usize, item_extent: f32, gap: f32) -> f32 {
    count as f32 * item_extent + (count - 1) as f32 * gap
}

const fn maximum(left: f32, right: f32) -> f32 {
    if left >= right { left } else { right }
}

const fn minimum(left: usize, right: usize) -> usize {
    if left <= right { left } else { right }
}
