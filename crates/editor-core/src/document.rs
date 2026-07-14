use std::path::{Path, PathBuf};

use depthsprite_format::DepthSpriteModel;
use relief_core::{Bounds, CanonicalView, Chart};

use crate::{DepthValue, EditorError, ReliefValue, SourceSprite, fallback::resolve_charts};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DocumentState {
    pub(crate) bounds: Bounds,
    pub(crate) sources: Vec<SourceSprite>,
    pub(crate) selection: CanonicalView,
    pub(crate) active_layer: ActiveLayer,
    pub(crate) tool: Tool,
    pub(crate) current_rgb: [u8; 3],
    pub(crate) current_depth: DepthValue,
}

impl DocumentState {
    pub(crate) fn has_same_persistent_content(&self, other: &Self) -> bool {
        self.bounds == other.bounds && self.has_same_authored_sources(other)
    }

    pub(crate) fn has_same_authored_sources(&self, other: &Self) -> bool {
        self.sources == other.sources
    }
}

pub struct EditorDocument {
    pub(crate) state: DocumentState,
    pub(crate) saved_state: DocumentState,
    pub(crate) undo: Vec<DocumentState>,
    pub(crate) redo: Vec<DocumentState>,
    pub(crate) stroke_before: Option<DocumentState>,
    path: Option<PathBuf>,
    pub(crate) revision: u64,
}

impl EditorDocument {
    pub fn new(bounds: Bounds, initial: CanonicalView) -> Self {
        let state = DocumentState {
            bounds,
            sources: vec![SourceSprite::empty(initial, bounds)],
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

    pub fn from_model(model: DepthSpriteModel, path: Option<PathBuf>) -> Result<Self, EditorError> {
        let bounds = model.bounds();
        let sources: Vec<_> = model
            .charts()
            .iter()
            .map(SourceSprite::from_chart)
            .collect();
        for source in &sources {
            Self::validate_source(bounds, source)?;
        }
        let selection = sources[0].view();
        let state = DocumentState {
            bounds,
            sources,
            selection,
            active_layer: ActiveLayer::Color,
            tool: Tool::Pencil,
            current_rgb: [0, 0, 0],
            current_depth: DepthValue::Relief(
                ReliefValue::new(0).expect("zero relief is always valid"),
            ),
        };
        Ok(Self::from_clean_state(state, path))
    }

    pub fn to_model(&self) -> Result<DepthSpriteModel, EditorError> {
        let charts = self
            .state
            .sources
            .iter()
            .map(SourceSprite::to_chart)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(DepthSpriteModel::new(self.state.bounds, charts)?)
    }

    pub fn sources(&self) -> impl ExactSizeIterator<Item = &SourceSprite> {
        self.state.sources.iter()
    }

    pub fn source(&self, view: CanonicalView) -> Option<&SourceSprite> {
        self.state
            .sources
            .iter()
            .find(|source| source.view() == view)
    }

    pub fn add_source(&mut self, view: CanonicalView) -> Result<(), EditorError> {
        self.ensure_no_active_stroke()?;
        if self.state.sources.len() == 6 {
            return Err(EditorError::SourceLimit);
        }
        if self.source(view).is_some() {
            return Err(EditorError::SourceAlreadyExists(view));
        }

        let before = self.state.clone();
        self.state
            .sources
            .push(SourceSprite::empty(view, self.state.bounds));
        self.state
            .sources
            .sort_by_key(|source| source.view().rank());
        self.state.selection = view;
        self.finish_command(before);
        Ok(())
    }

    pub fn replace_source(&mut self, source: SourceSprite) -> Result<(), EditorError> {
        self.ensure_no_active_stroke()?;
        Self::validate_source(self.state.bounds, &source)?;
        let Some(index) = self
            .state
            .sources
            .iter()
            .position(|current| current.view() == source.view())
        else {
            return Err(EditorError::SourceNotFound(source.view()));
        };

        let before = self.state.clone();
        self.state.selection = source.view();
        self.state.sources[index] = source;
        self.finish_command(before);
        Ok(())
    }

    pub fn remove_source(&mut self, view: CanonicalView) -> Result<(), EditorError> {
        self.ensure_no_active_stroke()?;
        let Some(index) = self
            .state
            .sources
            .iter()
            .position(|source| source.view() == view)
        else {
            return Err(EditorError::SourceNotFound(view));
        };
        if self.state.sources.len() == 1 {
            return Err(EditorError::LastSource);
        }

        let before = self.state.clone();
        self.state.sources.remove(index);
        if self.state.selection == view {
            self.state.selection = self.state.sources[0].view();
        }
        self.finish_command(before);
        Ok(())
    }

    pub fn resolved_charts(&self) -> Result<Vec<Chart>, EditorError> {
        resolve_charts(&self.state.sources)
    }

    pub fn bounds(&self) -> Bounds {
        self.state.bounds
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
        }
    }

    fn validate_source(bounds: Bounds, source: &SourceSprite) -> Result<(), EditorError> {
        let expected = source.view().dimensions(bounds);
        let actual = source.dimensions();
        if actual != expected {
            return Err(EditorError::DimensionMismatch {
                view: source.view(),
                expected,
                actual,
            });
        }
        Ok(())
    }
}
