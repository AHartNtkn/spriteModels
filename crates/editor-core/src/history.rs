use crate::{EditorDocument, EditorError, document::DocumentState};

impl EditorDocument {
    pub fn begin_stroke(&mut self) -> Result<(), EditorError> {
        self.ensure_no_active_stroke()?;
        self.stroke_before = Some(self.state.clone());
        Ok(())
    }

    pub fn finish_stroke(&mut self) -> Result<bool, EditorError> {
        let before = self
            .stroke_before
            .take()
            .ok_or(EditorError::NoActiveStroke)?;
        let changed = self.state != before;
        if changed {
            self.record_undo(before);
        }
        Ok(changed)
    }

    pub fn cancel_stroke(&mut self) {
        let Some(before) = self.stroke_before.take() else {
            return;
        };
        self.state = before;
        self.advance_revision();
    }

    pub fn undo(&mut self) -> bool {
        if self.stroke_before.is_some() {
            return false;
        }
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.state, previous));
        self.advance_revision();
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.stroke_before.is_some() {
            return false;
        }
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.state, next));
        self.advance_revision();
        true
    }

    pub(crate) fn ensure_no_active_stroke(&self) -> Result<(), EditorError> {
        if self.stroke_before.is_some() {
            Err(EditorError::StrokeAlreadyActive)
        } else {
            Ok(())
        }
    }

    pub(crate) fn finish_command(&mut self, before: DocumentState) -> bool {
        if self.state == before {
            return false;
        }
        self.record_undo(before);
        self.advance_revision();
        true
    }

    pub(crate) fn advance_revision(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("document revision must remain monotonically increasing");
    }

    fn record_undo(&mut self, before: DocumentState) {
        self.undo.push(before);
        self.redo.clear();
    }
}
