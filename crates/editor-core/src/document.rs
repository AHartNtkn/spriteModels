use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart};

use crate::{DepthValue, EditorError, ReliefValue};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveLayer {
    Color,
    Depth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tool {
    Pencil,
    Eraser,
    Fill,
    Eyedropper,
}

impl Tool {
    pub const fn is_available_on(self, layer: ActiveLayer) -> bool {
        !matches!((self, layer), (Self::Eraser, ActiveLayer::Color))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DocumentState {
    pub(crate) model: AuthoredModel,
    pub(crate) selection: CanonicalView,
    pub(crate) active_layer: ActiveLayer,
    pub(crate) tool: Tool,
    pub(crate) current_rgb: [u8; 3],
    pub(crate) current_depth: DepthValue,
}

impl DocumentState {
    pub(crate) fn has_same_persistent_content(&self, other: &Self) -> bool {
        self.model == other.model
    }

    pub(crate) fn has_same_authored_sources(&self, other: &Self) -> bool {
        self.model.charts() == other.model.charts()
    }
}

pub struct EditorDocument {
    pub(crate) state: DocumentState,
    pub(crate) saved_state: DocumentState,
    pub(crate) undo: Vec<DocumentState>,
    pub(crate) redo: Vec<DocumentState>,
    pub(crate) stroke_before: Option<DocumentState>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) revision: u64,
    pub(crate) render_identity: u64,
}

static NEXT_RENDER_IDENTITY: AtomicU64 = AtomicU64::new(1);

impl EditorDocument {
    pub fn new(bounds: Bounds, initial: CanonicalView) -> Self {
        let model = AuthoredModel::with_empty_chart(bounds, initial)
            .expect("validated bounds always produce a valid empty chart");
        let state = DocumentState {
            model,
            selection: initial,
            active_layer: ActiveLayer::Color,
            tool: Tool::Pencil,
            current_rgb: [0, 0, 0],
            current_depth: DepthValue::Relief(
                ReliefValue::new(0).expect("zero relief is always valid"),
            ),
        };
        Self::from_clean_state(state, None)
    }

    pub fn from_model(model: AuthoredModel, path: Option<PathBuf>) -> Self {
        let selection = model.charts()[0].view();
        let state = DocumentState {
            model,
            selection,
            active_layer: ActiveLayer::Color,
            tool: Tool::Pencil,
            current_rgb: [0, 0, 0],
            current_depth: DepthValue::Relief(
                ReliefValue::new(0).expect("zero relief is always valid"),
            ),
        };
        Self::from_clean_state(state, path)
    }

    pub fn model(&self) -> &AuthoredModel {
        &self.state.model
    }

    pub fn to_model(&self) -> AuthoredModel {
        self.state.model.clone()
    }

    pub fn sources(&self) -> impl ExactSizeIterator<Item = &Chart> {
        self.state.model.charts().iter()
    }

    pub fn source(&self, view: CanonicalView) -> Option<&Chart> {
        self.state.model.chart(view)
    }

    pub fn add_source(&mut self, view: CanonicalView) -> Result<(), EditorError> {
        self.ensure_no_active_stroke()?;
        let before = self.state.clone();
        self.state.model.add_empty_chart(view)?;
        self.state.selection = view;
        self.finish_command(before);
        Ok(())
    }

    pub fn replace_source(&mut self, source: Chart) -> Result<(), EditorError> {
        self.ensure_no_active_stroke()?;
        let before = self.state.clone();
        let view = source.view();
        self.state.model.replace_chart(source)?;
        self.state.selection = view;
        self.finish_command(before);
        Ok(())
    }

    pub fn remove_source(&mut self, view: CanonicalView) -> Result<(), EditorError> {
        self.ensure_no_active_stroke()?;
        let before = self.state.clone();
        self.state.model.remove_chart(view)?;
        if self.state.selection == view {
            self.state.selection = self.state.model.charts()[0].view();
        }
        self.finish_command(before);
        Ok(())
    }

    pub fn bounds(&self) -> Bounds {
        self.state.model.bounds()
    }

    pub fn selected_view(&self) -> CanonicalView {
        self.state.selection
    }

    pub fn active_layer(&self) -> ActiveLayer {
        self.state.active_layer
    }

    pub fn tool(&self) -> Tool {
        self.state.tool
    }

    pub fn current_rgb(&self) -> [u8; 3] {
        self.state.current_rgb
    }

    pub fn current_depth(&self) -> DepthValue {
        self.state.current_depth
    }

    pub fn is_dirty(&self) -> bool {
        !self.state.has_same_persistent_content(&self.saved_state)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn render_key(&self) -> (u64, u64) {
        (self.render_identity, self.revision)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn stroke_active(&self) -> bool {
        self.stroke_before.is_some()
    }

    fn from_clean_state(state: DocumentState, path: Option<PathBuf>) -> Self {
        Self {
            saved_state: state.clone(),
            state,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke_before: None,
            path,
            revision: 0,
            render_identity: NEXT_RENDER_IDENTITY.fetch_add(1, Ordering::Relaxed),
        }
    }
}
