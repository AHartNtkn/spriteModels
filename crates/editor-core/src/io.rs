use std::path::Path;

use depthsprite_format::{load_path, load_rgba_png, save_path_atomic};
use relief_core::{CanonicalView, Chart, ModelError};

use crate::{EditorDocument, EditorError};

impl EditorDocument {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EditorError> {
        let path = path.as_ref().to_owned();
        let model = load_path(&path)?;
        Ok(Self::from_model(model, Some(path)))
    }

    pub fn import_source_png(
        &mut self,
        view: CanonicalView,
        path: impl AsRef<Path>,
    ) -> Result<(), EditorError> {
        let image = load_rgba_png(path)?;
        let source = Chart::from_rgba(view, image.width, image.height, image.pixels)
            .map_err(ModelError::from)?;
        self.replace_source(source)
    }

    pub fn save(&mut self) -> Result<(), EditorError> {
        let path = self.path.clone().ok_or(EditorError::MissingPath)?;
        self.save_as(path)
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), EditorError> {
        self.ensure_no_active_stroke()?;
        let path = path.as_ref().to_owned();
        let model = self.to_model();
        save_path_atomic(&model, &path)?;
        self.path = Some(path);
        self.saved_state = self.state.clone();
        Ok(())
    }
}
