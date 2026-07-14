use std::path::{Path, PathBuf};

use editor_core::{EditorDocument, EditorError};
use eframe::egui;
use relief_core::{
    CanonicalView, ChartEdge, DiscardPolicy, ImageEdge, ModelError, ReassignMode, ResizeDelta,
    ResizeRequest,
};

use crate::{
    canvas::CanvasPairState,
    layout::{CANONICAL_SOURCE_ORDER, SourceCardLayout},
};

const PNG_FILTER: &[&str] = &["png"];
pub const SOURCE_DISPLAY_ORDER: [CanonicalView; 6] = CANONICAL_SOURCE_ORDER;

pub fn add_source(document: &mut EditorDocument, view: CanonicalView) -> Result<(), EditorError> {
    document.add_source(view)
}

pub fn replace_source_from_png(
    document: &mut EditorDocument,
    view: CanonicalView,
    path: impl AsRef<Path>,
) -> Result<(), EditorError> {
    document.import_source_png(view, path)
}

pub fn remove_source(
    document: &mut EditorDocument,
    view: CanonicalView,
) -> Result<(), EditorError> {
    document.remove_source(view)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CardHeader {
    pub label: &'static str,
}

pub fn card_header(document: &EditorDocument, view: CanonicalView) -> Option<CardHeader> {
    document.source(view)?;
    Some(CardHeader {
        label: view_label(view),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingSourceAction {
    Recreate {
        from: CanonicalView,
        to: CanonicalView,
    },
    DiscardResize {
        request: ResizeRequest,
        edges: Vec<ChartEdge>,
    },
}

pub struct SourceGridState {
    cards: [CanvasPairState; 6],
    error: Option<String>,
    pending: Option<PendingSourceAction>,
}

pub(crate) struct SourceGridOutput {
    #[cfg(test)]
    pub observation: SourceGridObservation,
}

#[cfg(test)]
pub(crate) struct SourceGridObservation {
    pub cards: Vec<SourceCardObservation>,
    pub add_button: Option<egui::Rect>,
    pub add_popover: Option<SidePopoverObservation>,
    pub confirmation: Option<ConfirmationObservation>,
    pub error: Option<String>,
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct SideTargetObservation {
    pub view: CanonicalView,
    pub rect: egui::Rect,
    pub enabled: bool,
}

#[cfg(test)]
pub(crate) struct SidePopoverObservation {
    pub rect: egui::Rect,
    pub targets: Vec<SideTargetObservation>,
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct ResizeActionObservation {
    pub request: ResizeRequest,
    pub rect: egui::Rect,
}

#[cfg(test)]
pub(crate) struct ResizePopoverObservation {
    pub rect: egui::Rect,
    pub center: egui::Rect,
    pub dimensions: (u32, u32),
    pub actions: Vec<ResizeActionObservation>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfirmationKind {
    Recreate {
        from: CanonicalView,
        to: CanonicalView,
    },
    DiscardResize(ResizeRequest),
}

#[cfg(test)]
pub(crate) struct ConfirmationObservation {
    pub kind: ConfirmationKind,
    pub rect: egui::Rect,
    pub confirm: egui::Rect,
    pub cancel: egui::Rect,
    pub affected: Vec<ChartEdge>,
}

#[cfg(test)]
pub(crate) struct SourceCardObservation {
    pub view: CanonicalView,
    pub column: usize,
    pub row: usize,
    pub card: egui::Rect,
    pub color: egui::Rect,
    pub depth: egui::Rect,
    pub header: egui::Rect,
    pub side_selector: egui::Rect,
    pub side_popover: Option<SidePopoverObservation>,
    pub resize_button: Option<egui::Rect>,
    pub resize_popover: Option<ResizePopoverObservation>,
    pub overflow_menu: egui::Rect,
}

struct AuthoredCardOutput {
    #[cfg(test)]
    canvas: crate::canvas::CanvasPairOutput,
    #[cfg(test)]
    header: egui::Rect,
    #[cfg(test)]
    side_selector: egui::Rect,
    #[cfg(test)]
    side_popover: Option<SidePopoverObservation>,
    #[cfg(test)]
    resize_button: Option<egui::Rect>,
    #[cfg(test)]
    resize_popover: Option<ResizePopoverObservation>,
    #[cfg(test)]
    overflow_menu: egui::Rect,
}

impl Default for SourceGridState {
    fn default() -> Self {
        Self {
            cards: std::array::from_fn(|_| CanvasPairState::default()),
            error: None,
            pending: None,
        }
    }
}

impl SourceGridState {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut EditorDocument,
        layouts: &[SourceCardLayout],
        add_button: Option<crate::layout::Rect>,
        origin: egui::Pos2,
    ) -> SourceGridOutput {
        #[cfg(test)]
        let mut observed_cards = Vec::new();
        let authored_views = SOURCE_DISPLAY_ORDER
            .into_iter()
            .filter(|view| document.source(*view).is_some())
            .collect::<Vec<_>>();
        assert_eq!(authored_views.len(), layouts.len());
        for (layout, view) in layouts.iter().zip(authored_views) {
            let _card_rect = to_egui(layout.card, origin);
            let state_index = SOURCE_DISPLAY_ORDER
                .iter()
                .position(|candidate| *candidate == view)
                .expect("authored views come from canonical order");
            let authored = self.show_authored_card(ui, document, state_index, view, layout, origin);
            #[cfg(test)]
            observed_cards.push(SourceCardObservation {
                view,
                column: layout.column,
                row: layout.row,
                card: _card_rect,
                color: authored.canvas.observation.color,
                depth: authored.canvas.observation.depth,
                header: authored.header,
                side_selector: authored.side_selector,
                side_popover: authored.side_popover,
                resize_button: authored.resize_button,
                resize_popover: authored.resize_popover,
                overflow_menu: authored.overflow_menu,
            });
            #[cfg(not(test))]
            {
                let _ = authored;
            }
        }

        let add_rect = add_button.map(|rect| to_egui(rect, origin));
        #[cfg(test)]
        let mut add_popover = None;
        let mut add_choice = None;
        let _add_button = add_rect.map(|rect| {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                let menu = egui::containers::menu::MenuButton::from_button(
                    egui::Button::new("Add Sprite").min_size(rect.size()),
                )
                .ui(ui, |ui| {
                    #[cfg(test)]
                    let mut targets = Vec::new();
                    for view in SOURCE_DISPLAY_ORDER {
                        let enabled = document.source(view).is_none();
                        let response = ui.add_enabled(enabled, egui::Button::new(view_label(view)));
                        #[cfg(test)]
                        targets.push(SideTargetObservation {
                            view,
                            rect: response.rect,
                            enabled,
                        });
                        if response.clicked() {
                            add_choice = Some(view);
                            ui.close();
                        }
                    }
                    #[cfg(test)]
                    {
                        add_popover = Some(SidePopoverObservation {
                            rect: ui.min_rect(),
                            targets,
                        });
                    }
                });
                menu.0.rect
            })
            .inner
        });
        if let Some(view) = add_choice {
            self.capture(add_source(document, view));
        }

        #[cfg(test)]
        let confirmation = self.show_pending_confirmation(ui.ctx(), document);
        #[cfg(not(test))]
        self.show_pending_confirmation(ui.ctx(), document);

        if let Some(error) = &self.error {
            ui.painter().text(
                origin + egui::vec2(layouts[0].card.left(), layouts[0].card.bottom() + 2.0),
                egui::Align2::LEFT_TOP,
                error,
                egui::FontId::monospace(10.0),
                egui::Color32::LIGHT_RED,
            );
        }
        SourceGridOutput {
            #[cfg(test)]
            observation: SourceGridObservation {
                cards: observed_cards,
                add_button: _add_button,
                add_popover,
                confirmation,
                error: self.error.clone(),
            },
        }
    }

    fn show_authored_card(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut EditorDocument,
        index: usize,
        view: CanonicalView,
        layout: &SourceCardLayout,
        origin: egui::Pos2,
    ) -> AuthoredCardOutput {
        let card_rect = to_egui(layout.card, origin);
        ui.painter()
            .rect_filled(card_rect, 4.0, egui::Color32::from_gray(36));
        let header = card_header(document, view).expect("authored sources always have headers");
        let color_rect = to_egui(layout.color, origin);
        let header_rect = egui::Rect::from_min_size(
            card_rect.min
                + egui::vec2(
                    crate::layout::SOURCE_CARD_PADDING,
                    crate::layout::SOURCE_CARD_PADDING,
                ),
            egui::vec2(
                card_rect.width() - crate::layout::SOURCE_CARD_PADDING * 2.0,
                crate::layout::SOURCE_HEADER_HEIGHT,
            ),
        );
        let overflow_rect = egui::Rect::from_min_size(
            egui::pos2(header_rect.right() - 24.0, header_rect.top()),
            egui::vec2(24.0, header_rect.height()),
        );
        let selected = document.selected_view() == view;
        let resize_rect = selected.then(|| {
            egui::Rect::from_min_size(
                egui::pos2(overflow_rect.left() - 50.0, header_rect.top()),
                egui::vec2(48.0, header_rect.height()),
            )
        });
        let side_rect = egui::Rect::from_min_max(
            header_rect.min,
            egui::pos2(
                resize_rect.map_or(overflow_rect.left(), |rect| rect.left()) - 2.0,
                header_rect.bottom(),
            ),
        );

        let mut assignment = None;
        #[cfg(test)]
        let mut side_popover = None;
        let _side_selector = ui
            .scope_builder(egui::UiBuilder::new().max_rect(side_rect), |ui| {
                let menu = egui::containers::menu::MenuButton::from_button(
                    egui::Button::new(header.label).min_size(side_rect.size()),
                )
                .ui(ui, |ui| {
                    #[cfg(test)]
                    let mut targets = Vec::new();
                    for target in SOURCE_DISPLAY_ORDER {
                        let enabled = target != view && document.source(target).is_none();
                        let response =
                            ui.add_enabled(enabled, egui::Button::new(view_label(target)));
                        #[cfg(test)]
                        targets.push(SideTargetObservation {
                            view: target,
                            rect: response.rect,
                            enabled,
                        });
                        if response.clicked() {
                            assignment = Some(target);
                            ui.close();
                        }
                    }
                    #[cfg(test)]
                    {
                        side_popover = Some(SidePopoverObservation {
                            rect: ui.min_rect(),
                            targets,
                        });
                    }
                });
                menu.0.rect
            })
            .inner;

        let mut resize_request = None;
        #[cfg(test)]
        let mut resize_popover = None;
        let _resize_button = resize_rect.map(|rect| {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                let menu = egui::containers::menu::MenuButton::from_button(
                    egui::Button::new("Resize").min_size(rect.size()),
                )
                .ui(ui, |ui| {
                    let output =
                        show_resize_popover(ui, view, document.source(view).unwrap().dimensions());
                    resize_request = output.chosen;
                    #[cfg(test)]
                    {
                        resize_popover = Some(ResizePopoverObservation {
                            rect: output.rect,
                            center: output.center,
                            dimensions: output.dimensions,
                            actions: output
                                .actions
                                .into_iter()
                                .map(|action| ResizeActionObservation {
                                    request: action.request,
                                    rect: action.rect,
                                })
                                .collect(),
                        });
                    }
                });
                menu.0.rect
            })
            .inner
        });

        enum OverflowAction {
            Import,
            Remove,
        }
        let mut overflow_action = None;
        let _overflow_menu = ui
            .scope_builder(egui::UiBuilder::new().max_rect(overflow_rect), |ui| {
                let menu = egui::containers::menu::MenuButton::from_button(
                    egui::Button::new("⋮").min_size(overflow_rect.size()),
                )
                .ui(ui, |ui| {
                    if ui.button("Import PNG…").clicked() {
                        overflow_action = Some(OverflowAction::Import);
                        ui.close();
                    }
                    if ui.button("Remove").clicked() {
                        overflow_action = Some(OverflowAction::Remove);
                        ui.close();
                    }
                });
                menu.0.rect
            })
            .inner;

        if let Some(target) = assignment {
            self.assign_source(document, view, target);
        }
        if let Some(request) = resize_request {
            self.request_resize(document, request);
        }
        match overflow_action {
            Some(OverflowAction::Import) => {
                if let Some(path) = pick_source_png() {
                    self.capture(replace_source_from_png(document, view, path));
                }
            }
            Some(OverflowAction::Remove) => self.capture(remove_source(document, view)),
            None => {}
        }

        let depth_rect = to_egui(layout.depth, origin);
        let _canvas = self.cards[index].show_pair(ui, document, view, color_rect, depth_rect);
        AuthoredCardOutput {
            #[cfg(test)]
            canvas: _canvas,
            #[cfg(test)]
            header: header_rect,
            #[cfg(test)]
            side_selector: _side_selector,
            #[cfg(test)]
            side_popover,
            #[cfg(test)]
            resize_button: _resize_button,
            #[cfg(test)]
            resize_popover,
            #[cfg(test)]
            overflow_menu: _overflow_menu,
        }
    }

    fn assign_source(
        &mut self,
        document: &mut EditorDocument,
        from: CanonicalView,
        to: CanonicalView,
    ) {
        let dimensions_match =
            document.source(from).unwrap().dimensions() == to.dimensions(document.bounds());
        if dimensions_match {
            self.capture(document.reassign_source(from, to, ReassignMode::Preserve));
        } else {
            self.error = None;
            self.pending = Some(PendingSourceAction::Recreate { from, to });
        }
    }

    fn request_resize(&mut self, document: &mut EditorDocument, request: ResizeRequest) {
        match document.resize_source(request, DiscardPolicy::Reject) {
            Ok(()) => self.error = None,
            Err(EditorError::Model(ModelError::ResizeWouldDiscard { edges })) => {
                self.error = None;
                self.pending = Some(PendingSourceAction::DiscardResize { request, edges });
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn show_pending_confirmation(
        &mut self,
        context: &egui::Context,
        document: &mut EditorDocument,
    ) -> ConfirmationReturn {
        let Some(pending) = self.pending.clone() else {
            return confirmation_none();
        };
        let mut confirm = false;
        let mut cancel = false;
        #[cfg(test)]
        let mut observation = None;
        egui::Modal::new("source-action-confirmation".into()).show(context, |ui| {
            let (heading, detail, confirm_label) = match &pending {
                PendingSourceAction::Recreate { from, to } => (
                    "Recreate sprite?",
                    format!(
                        "{} and {} use different dimensions. The replacement will be empty.",
                        view_label(*from),
                        view_label(*to)
                    ),
                    "Recreate Empty",
                ),
                PendingSourceAction::DiscardResize { .. } => (
                    "Remove authored pixels?",
                    "The following authored edges contain pixels:".to_owned(),
                    "Remove Pixels",
                ),
            };
            ui.heading(heading);
            ui.label(detail);
            if let PendingSourceAction::DiscardResize { edges, .. } = &pending {
                for edge in edges {
                    ui.label(format!(
                        "{} {}",
                        view_label(edge.view),
                        edge_label(edge.edge)
                    ));
                }
            }
            let _buttons = ui.horizontal(|ui| {
                let confirm_response = ui.button(confirm_label);
                let cancel_response = ui.button("Cancel");
                if confirm_response.clicked() {
                    confirm = true;
                }
                if cancel_response.clicked() {
                    cancel = true;
                }
                (confirm_response.rect, cancel_response.rect)
            });
            #[cfg(test)]
            {
                let kind = match &pending {
                    PendingSourceAction::Recreate { from, to } => ConfirmationKind::Recreate {
                        from: *from,
                        to: *to,
                    },
                    PendingSourceAction::DiscardResize { request, .. } => {
                        ConfirmationKind::DiscardResize(*request)
                    }
                };
                let affected = match &pending {
                    PendingSourceAction::DiscardResize { edges, .. } => edges.clone(),
                    PendingSourceAction::Recreate { .. } => Vec::new(),
                };
                observation = Some(ConfirmationObservation {
                    kind,
                    rect: ui.min_rect(),
                    confirm: _buttons.inner.0,
                    cancel: _buttons.inner.1,
                    affected,
                });
            }
        });

        if cancel {
            self.pending = None;
        } else if confirm {
            self.pending = None;
            match pending {
                PendingSourceAction::Recreate { from, to } => {
                    self.capture(document.reassign_source(from, to, ReassignMode::RecreateEmpty))
                }
                PendingSourceAction::DiscardResize { request, .. } => {
                    self.capture(document.resize_source(request, DiscardPolicy::Allow));
                }
            }
        }
        #[cfg(test)]
        {
            observation
        }
        #[cfg(not(test))]
        {}
    }

    fn capture(&mut self, result: Result<(), EditorError>) {
        match result {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

#[cfg(test)]
type ConfirmationReturn = Option<ConfirmationObservation>;
#[cfg(not(test))]
type ConfirmationReturn = ();

#[cfg(test)]
fn confirmation_none() -> ConfirmationReturn {
    None
}
#[cfg(not(test))]
fn confirmation_none() -> ConfirmationReturn {}

#[cfg(test)]
struct ResizeActionUi {
    request: ResizeRequest,
    rect: egui::Rect,
}

struct ResizePopoverUi {
    #[cfg(test)]
    rect: egui::Rect,
    #[cfg(test)]
    center: egui::Rect,
    #[cfg(test)]
    dimensions: (u32, u32),
    #[cfg(test)]
    actions: Vec<ResizeActionUi>,
    chosen: Option<ResizeRequest>,
}

fn show_resize_popover(
    ui: &mut egui::Ui,
    view: CanonicalView,
    dimensions: (u32, u32),
) -> ResizePopoverUi {
    ui.set_min_width(210.0);
    #[cfg(test)]
    let mut actions = Vec::new();
    let mut chosen = None;
    let mut action_button = |ui: &mut egui::Ui, edge, delta, label| {
        let request = ResizeRequest { view, edge, delta };
        let response = ui.small_button(label);
        if response.clicked() {
            chosen = Some(request);
            ui.close();
        }
        #[cfg(test)]
        actions.push(ResizeActionUi {
            request,
            rect: response.rect,
        });
    };
    ui.vertical_centered(|ui| {
        ui.horizontal(|ui| {
            action_button(ui, ImageEdge::Top, ResizeDelta::Add, "+ Top");
            action_button(ui, ImageEdge::Top, ResizeDelta::Remove, "− Top");
        });
    });
    let _center = ui
        .horizontal(|ui| {
            ui.vertical(|ui| {
                action_button(ui, ImageEdge::Left, ResizeDelta::Add, "+ Left");
                action_button(ui, ImageEdge::Left, ResizeDelta::Remove, "− Left");
            });
            let center = ui.add_sized(
                [72.0, 42.0],
                egui::Label::new(format!("{} × {}", dimensions.0, dimensions.1)),
            );
            ui.vertical(|ui| {
                action_button(ui, ImageEdge::Right, ResizeDelta::Add, "+ Right");
                action_button(ui, ImageEdge::Right, ResizeDelta::Remove, "− Right");
            });
            center.rect
        })
        .inner;
    ui.vertical_centered(|ui| {
        ui.horizontal(|ui| {
            action_button(ui, ImageEdge::Bottom, ResizeDelta::Add, "+ Bottom");
            action_button(ui, ImageEdge::Bottom, ResizeDelta::Remove, "− Bottom");
        });
    });
    ResizePopoverUi {
        #[cfg(test)]
        rect: ui.min_rect(),
        #[cfg(test)]
        center: _center,
        #[cfg(test)]
        dimensions,
        #[cfg(test)]
        actions,
        chosen,
    }
}

const fn edge_label(edge: ImageEdge) -> &'static str {
    match edge {
        ImageEdge::Left => "Left",
        ImageEdge::Right => "Right",
        ImageEdge::Top => "Top",
        ImageEdge::Bottom => "Bottom",
    }
}

fn pick_source_png() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("PNG image", PNG_FILTER)
        .pick_file()
}

fn to_egui(rect: crate::layout::Rect, origin: egui::Pos2) -> egui::Rect {
    egui::Rect::from_min_max(
        origin + egui::vec2(rect.left(), rect.top()),
        origin + egui::vec2(rect.right(), rect.bottom()),
    )
}

pub const fn view_label(view: CanonicalView) -> &'static str {
    match view {
        CanonicalView::Front => "Front",
        CanonicalView::Right => "Right",
        CanonicalView::Top => "Top",
        CanonicalView::Back => "Back",
        CanonicalView::Left => "Left",
        CanonicalView::Bottom => "Bottom",
    }
}

#[cfg(test)]
mod tests {
    use editor_core::EditorDocument;
    use relief_core::{
        AuthoredModel, Bounds, CanonicalView, Chart, ChartEdge, EMPTY_RGBA, ImageEdge, ResizeDelta,
        ResizeRequest,
    };

    use super::*;
    use crate::layout::{Size, calculate_layout};

    fn run_frame(
        context: &egui::Context,
        grid: &mut SourceGridState,
        document: &mut EditorDocument,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, SourceGridObservation) {
        let layout = calculate_layout(Size::new(1600.0, 1000.0), document.sources().len()).unwrap();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            events,
            ..Default::default()
        };
        let mut observation = None;
        let output = context.run_ui(input, |ui| {
            observation = Some(
                grid.show(
                    ui,
                    document,
                    &layout.source_cards,
                    layout.add_button,
                    egui::Pos2::ZERO,
                )
                .observation,
            );
        });
        (output, observation.unwrap())
    }

    fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click(
        context: &egui::Context,
        grid: &mut SourceGridState,
        document: &mut EditorDocument,
        position: egui::Pos2,
    ) -> SourceGridObservation {
        let (_, clicked) = run_frame(
            context,
            grid,
            document,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, true),
                pointer_button(position, false),
            ],
        );
        let (_, settled) = run_frame(context, grid, document, Vec::new());
        if settled.add_popover.is_some()
            || settled.confirmation.is_some()
            || settled
                .cards
                .iter()
                .any(|card| card.side_popover.is_some() || card.resize_popover.is_some())
        {
            settled
        } else {
            clicked
        }
    }

    fn document_from_charts(bounds: Bounds, charts: Vec<Chart>) -> EditorDocument {
        EditorDocument::from_model(AuthoredModel::new(bounds, charts).unwrap(), None)
    }

    fn side_target(popover: &SidePopoverObservation, view: CanonicalView) -> SideTargetObservation {
        popover
            .targets
            .iter()
            .find(|target| target.view == view)
            .copied()
            .unwrap_or_else(|| panic!("popover did not contain {view:?}"))
    }

    fn resize_action(
        popover: &ResizePopoverObservation,
        edge: ImageEdge,
        delta: ResizeDelta,
    ) -> ResizeActionObservation {
        popover
            .actions
            .iter()
            .find(|action| action.request.edge == edge && action.request.delta == delta)
            .copied()
            .unwrap_or_else(|| panic!("popover did not contain {delta:?} {edge:?}"))
    }

    fn assert_rect_close(actual: egui::Rect, expected: egui::Rect) {
        for delta in [
            actual.min.x - expected.min.x,
            actual.min.y - expected.min.y,
            actual.max.x - expected.max.x,
            actual.max.y - expected.max.y,
        ] {
            assert!(
                delta.abs() < 0.1,
                "actual {actual:?} differs from expected {expected:?}"
            );
        }
    }

    #[test]
    fn explicit_add_menu_can_choose_back_second_in_display_order() {
        let context = egui::Context::default();
        let mut document =
            EditorDocument::new(Bounds::new(32, 32, 32).unwrap(), CanonicalView::Front);
        let mut grid = SourceGridState::default();

        let (_, initial) = run_frame(&context, &mut grid, &mut document, Vec::new());
        let opened = click(
            &context,
            &mut grid,
            &mut document,
            initial.add_button.unwrap().center(),
        );
        let chooser = opened
            .add_popover
            .expect("Add Sprite opens an explicit chooser");
        assert_eq!(
            chooser
                .targets
                .iter()
                .map(|target| target.view)
                .collect::<Vec<_>>(),
            SOURCE_DISPLAY_ORDER
        );
        assert!(
            chooser
                .targets
                .iter()
                .all(|target| chooser.rect.contains_rect(target.rect))
        );
        assert!(!side_target(&chooser, CanonicalView::Front).enabled);
        assert!(side_target(&chooser, CanonicalView::Back).enabled);

        click(
            &context,
            &mut grid,
            &mut document,
            side_target(&chooser, CanonicalView::Back).rect.center(),
        );
        assert!(document.source(CanonicalView::Back).is_some());
        assert!(document.source(CanonicalView::Right).is_none());
    }

    #[test]
    fn source_cards_pack_in_display_order_with_header_controls_outside_the_canvases() {
        let context = egui::Context::default();
        let mut document =
            EditorDocument::new(Bounds::new(32, 32, 32).unwrap(), CanonicalView::Front);
        for view in [
            CanonicalView::Bottom,
            CanonicalView::Back,
            CanonicalView::Top,
            CanonicalView::Left,
            CanonicalView::Right,
        ] {
            document.add_source(view).unwrap();
        }
        document.select_source(CanonicalView::Front).unwrap();
        let mut grid = SourceGridState::default();
        let (_, observation) = run_frame(&context, &mut grid, &mut document, Vec::new());
        let layout = calculate_layout(Size::new(1600.0, 1000.0), 6).unwrap();

        assert_eq!(
            observation
                .cards
                .iter()
                .map(|card| card.view)
                .collect::<Vec<_>>(),
            SOURCE_DISPLAY_ORDER
        );
        assert!(observation.add_button.is_none());
        assert!(observation.error.is_none());
        for (card, expected) in observation.cards.iter().zip(&layout.source_cards) {
            assert_eq!((card.column, card.row), (expected.column, expected.row));
            assert_eq!(card.header.height(), crate::layout::SOURCE_HEADER_HEIGHT);
            assert!(
                card.header.expand(0.1).contains_rect(card.side_selector),
                "header {:?} does not contain side selector {:?} for {:?}",
                card.header,
                card.side_selector,
                card.view
            );
            assert!(
                card.header.expand(0.1).contains_rect(card.overflow_menu),
                "header {:?} does not contain overflow {:?} for {:?}",
                card.header,
                card.overflow_menu,
                card.view
            );
            if card.view == CanonicalView::Front {
                assert!(
                    card.header
                        .expand(0.1)
                        .contains_rect(card.resize_button.unwrap())
                );
            } else {
                assert!(card.resize_button.is_none());
            }
            assert_rect_close(card.color, to_egui(expected.color, egui::Pos2::ZERO));
            assert_rect_close(card.depth, to_egui(expected.depth, egui::Pos2::ZERO));
            assert!(card.header.bottom() <= card.color.top() + 0.1);
            assert!(card.header.bottom() < card.depth.top());
        }
    }

    #[test]
    fn assignment_disables_occupied_targets_and_preserves_pixels_when_dimensions_match() {
        let context = egui::Context::default();
        let bounds = Bounds::new(2, 1, 2).unwrap();
        let pixels = vec![[11, 22, 33, 251], EMPTY_RGBA];
        let mut document = document_from_charts(
            bounds,
            vec![
                Chart::from_rgba(CanonicalView::Front, 2, 1, pixels.clone()).unwrap(),
                Chart::from_rgba(CanonicalView::Back, 2, 1, vec![EMPTY_RGBA; 2]).unwrap(),
            ],
        );
        let before_revision = document.revision();
        let mut grid = SourceGridState::default();

        let (_, initial) = run_frame(&context, &mut grid, &mut document, Vec::new());
        let front = initial
            .cards
            .iter()
            .find(|card| card.view == CanonicalView::Front)
            .unwrap();
        let opened = click(
            &context,
            &mut grid,
            &mut document,
            front.side_selector.center(),
        );
        let chooser = opened
            .cards
            .iter()
            .find(|card| card.view == CanonicalView::Front)
            .unwrap()
            .side_popover
            .as_ref()
            .expect("side name opens assignment chooser");
        assert!(!side_target(chooser, CanonicalView::Front).enabled);
        assert!(!side_target(chooser, CanonicalView::Back).enabled);
        assert!(side_target(chooser, CanonicalView::Right).enabled);

        click(
            &context,
            &mut grid,
            &mut document,
            side_target(chooser, CanonicalView::Right).rect.center(),
        );
        assert_eq!(document.revision(), before_revision + 1);
        assert!(document.source(CanonicalView::Front).is_none());
        assert_eq!(
            document.source(CanonicalView::Right).unwrap().rgba(),
            pixels
        );
        assert!(document.undo());
        assert_eq!(
            document.source(CanonicalView::Front).unwrap().rgba(),
            pixels
        );
        assert!(
            !document.undo(),
            "reassignment creates one undo entry after prior history is cleared by the test boundary"
        );
    }

    #[test]
    fn mismatched_assignment_requires_recreate_and_cancel_does_not_mutate() {
        let context = egui::Context::default();
        let mut document = EditorDocument::new(Bounds::new(2, 1, 3).unwrap(), CanonicalView::Front);
        let before_revision = document.revision();
        let mut grid = SourceGridState::default();

        let (_, initial) = run_frame(&context, &mut grid, &mut document, Vec::new());
        let opened = click(
            &context,
            &mut grid,
            &mut document,
            initial.cards[0].side_selector.center(),
        );
        let chooser = opened.cards[0].side_popover.as_ref().unwrap();
        let pending = click(
            &context,
            &mut grid,
            &mut document,
            side_target(chooser, CanonicalView::Right).rect.center(),
        );
        let confirmation = pending
            .confirmation
            .expect("mismatched dimensions require explicit recreation");
        assert_eq!(
            confirmation.kind,
            ConfirmationKind::Recreate {
                from: CanonicalView::Front,
                to: CanonicalView::Right,
            }
        );
        assert!(confirmation.rect.contains_rect(confirmation.confirm));
        assert!(confirmation.rect.contains_rect(confirmation.cancel));
        assert_eq!(document.revision(), before_revision);

        click(
            &context,
            &mut grid,
            &mut document,
            confirmation.cancel.center(),
        );
        assert_eq!(document.revision(), before_revision);
        assert!(document.source(CanonicalView::Front).is_some());
        assert!(document.source(CanonicalView::Right).is_none());

        let (_, initial) = run_frame(&context, &mut grid, &mut document, Vec::new());
        let opened = click(
            &context,
            &mut grid,
            &mut document,
            initial.cards[0].side_selector.center(),
        );
        let chooser = opened.cards[0].side_popover.as_ref().unwrap();
        let pending = click(
            &context,
            &mut grid,
            &mut document,
            side_target(chooser, CanonicalView::Right).rect.center(),
        );
        let confirmation = pending.confirmation.unwrap();
        click(
            &context,
            &mut grid,
            &mut document,
            confirmation.confirm.center(),
        );
        assert_eq!(document.revision(), before_revision + 1);
        assert!(document.source(CanonicalView::Front).is_none());
        assert_eq!(
            document.source(CanonicalView::Right).unwrap().rgba(),
            &[EMPTY_RGBA; 3]
        );
        assert!(document.undo());
        assert!(document.source(CanonicalView::Front).is_some());
        assert!(!document.undo());
    }

    #[test]
    fn destructive_resize_names_edges_and_confirmation_is_one_undoable_change() {
        let context = egui::Context::default();
        let bounds = Bounds::new(2, 1, 2).unwrap();
        let mut document = document_from_charts(
            bounds,
            vec![
                Chart::from_rgba(CanonicalView::Front, 2, 1, vec![[9, 8, 7, 0], EMPTY_RGBA])
                    .unwrap(),
            ],
        );
        let mut grid = SourceGridState::default();

        let (_, initial) = run_frame(&context, &mut grid, &mut document, Vec::new());
        let original_color = initial.cards[0].color;
        let original_depth = initial.cards[0].depth;
        let opened = click(
            &context,
            &mut grid,
            &mut document,
            initial.cards[0].resize_button.unwrap().center(),
        );
        let popover = opened.cards[0]
            .resize_popover
            .as_ref()
            .expect("Resize opens its compact popover");
        assert_eq!(popover.dimensions, (2, 1));
        assert_eq!(popover.actions.len(), 8);
        assert!(popover.rect.contains_rect(popover.center));
        for action in &popover.actions {
            assert!(popover.rect.contains_rect(action.rect));
        }
        assert_eq!(opened.cards[0].color, original_color);
        assert_eq!(opened.cards[0].depth, original_depth);

        let request = ResizeRequest {
            view: CanonicalView::Front,
            edge: ImageEdge::Left,
            delta: ResizeDelta::Remove,
        };
        let before_revision = document.revision();
        let pending = click(
            &context,
            &mut grid,
            &mut document,
            resize_action(popover, ImageEdge::Left, ResizeDelta::Remove)
                .rect
                .center(),
        );
        let confirmation = pending
            .confirmation
            .expect("authored edge requires destructive confirmation");
        assert_eq!(confirmation.kind, ConfirmationKind::DiscardResize(request));
        assert!(confirmation.rect.contains_rect(confirmation.confirm));
        assert!(confirmation.rect.contains_rect(confirmation.cancel));
        assert_eq!(
            confirmation.affected,
            [ChartEdge {
                view: CanonicalView::Front,
                edge: ImageEdge::Left,
            }]
        );
        assert_eq!(document.revision(), before_revision);

        click(
            &context,
            &mut grid,
            &mut document,
            confirmation.cancel.center(),
        );
        assert_eq!(document.revision(), before_revision);
        assert_eq!(
            document.source(CanonicalView::Front).unwrap().dimensions(),
            (2, 1)
        );

        let (_, initial) = run_frame(&context, &mut grid, &mut document, Vec::new());
        let opened = click(
            &context,
            &mut grid,
            &mut document,
            initial.cards[0].resize_button.unwrap().center(),
        );
        let popover = opened.cards[0].resize_popover.as_ref().unwrap();
        let pending = click(
            &context,
            &mut grid,
            &mut document,
            resize_action(popover, ImageEdge::Left, ResizeDelta::Remove)
                .rect
                .center(),
        );
        let confirmation = pending.confirmation.unwrap();
        click(
            &context,
            &mut grid,
            &mut document,
            confirmation.confirm.center(),
        );
        assert_eq!(document.revision(), before_revision + 1);
        assert_eq!(
            document.source(CanonicalView::Front).unwrap().dimensions(),
            (1, 1)
        );
        assert!(document.undo());
        assert_eq!(
            document.source(CanonicalView::Front).unwrap().dimensions(),
            (2, 1)
        );
        assert!(!document.undo());
    }
}
