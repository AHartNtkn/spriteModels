use relief_core::CanonicalView;

use crate::{ActiveLayer, EditorDocument, EditorError, SourceSprite};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DepthValue {
    Empty,
    Relief(ReliefValue),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReliefValue(u8);

impl ReliefValue {
    pub const fn new(value: u8) -> Result<Self, EditorError> {
        if value <= 254 {
            Ok(Self(value))
        } else {
            Err(EditorError::InvalidRelief(value))
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for ReliefValue {
    type Error = EditorError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ReliefValue> for u8 {
    fn from(value: ReliefValue) -> Self {
        value.get()
    }
}

impl EditorDocument {
    pub fn set_active_layer(&mut self, layer: ActiveLayer) {
        self.state.active_layer = layer;
    }

    pub fn set_current_rgb(&mut self, rgb: [u8; 3]) {
        self.state.current_rgb = rgb;
    }

    pub fn set_current_depth(&mut self, depth: DepthValue) {
        self.state.current_depth = depth;
    }

    pub fn pencil_pixel(
        &mut self,
        view: CanonicalView,
        x: u32,
        y: u32,
    ) -> Result<bool, EditorError> {
        self.require_active_stroke()?;
        let pixel = self.pixel(view, x, y)?;
        let replacement = match self.state.active_layer {
            ActiveLayer::Color => [
                self.state.current_rgb[0],
                self.state.current_rgb[1],
                self.state.current_rgb[2],
                pixel[3],
            ],
            ActiveLayer::Depth => {
                let DepthValue::Relief(relief) = self.state.current_depth else {
                    return Ok(false);
                };
                [pixel[0], pixel[1], pixel[2], 255 - relief.get()]
            }
        };
        self.write_live_pixel(view, x, y, replacement)
    }

    pub fn erase_pixel(
        &mut self,
        view: CanonicalView,
        x: u32,
        y: u32,
    ) -> Result<bool, EditorError> {
        self.require_active_stroke()?;
        let pixel = self.pixel(view, x, y)?;
        if self.state.active_layer == ActiveLayer::Color {
            return Ok(false);
        }
        self.write_live_pixel(view, x, y, [pixel[0], pixel[1], pixel[2], 0])
    }

    pub fn fill(&mut self, view: CanonicalView, x: u32, y: u32) -> Result<bool, EditorError> {
        self.ensure_no_active_stroke()?;
        let source_index = self.source_index(view)?;
        let source = &self.state.sources[source_index];
        let start = pixel_index(source, view, x, y)?;
        let (width, height) = source.dimensions();
        let mut rgba = source.rgba().to_vec();
        let before = self.state.clone();

        match self.state.active_layer {
            ActiveLayer::Color => {
                let target = [rgba[start][0], rgba[start][1], rgba[start][2]];
                let replacement = self.state.current_rgb;
                if target == replacement {
                    return Ok(false);
                }
                flood_indices(width, height, start, |index| {
                    let pixel = &mut rgba[index];
                    if pixel[..3] != target {
                        return false;
                    }
                    pixel[..3].copy_from_slice(&replacement);
                    true
                });
            }
            ActiveLayer::Depth => {
                let DepthValue::Relief(relief) = self.state.current_depth else {
                    return Ok(false);
                };
                let target = rgba[start][3];
                let replacement = 255 - relief.get();
                if target == replacement {
                    return Ok(false);
                }
                flood_indices(width, height, start, |index| {
                    let pixel = &mut rgba[index];
                    if pixel[3] != target {
                        return false;
                    }
                    pixel[3] = replacement;
                    true
                });
            }
        }

        self.state.sources[source_index] = SourceSprite::from_rgba(view, width, height, rgba)?;
        Ok(self.finish_command(before))
    }

    pub fn eyedrop(&mut self, view: CanonicalView, x: u32, y: u32) -> Result<(), EditorError> {
        let pixel = self.pixel(view, x, y)?;
        match self.state.active_layer {
            ActiveLayer::Color => self.state.current_rgb = [pixel[0], pixel[1], pixel[2]],
            ActiveLayer::Depth => {
                self.state.current_depth = if pixel[3] == 0 {
                    DepthValue::Empty
                } else {
                    DepthValue::Relief(
                        ReliefValue::new(255 - pixel[3])
                            .expect("nonempty alpha always decodes to relief 0..=254"),
                    )
                };
            }
        }
        Ok(())
    }

    fn require_active_stroke(&self) -> Result<(), EditorError> {
        if self.stroke_before.is_some() {
            Ok(())
        } else {
            Err(EditorError::NoActiveStroke)
        }
    }

    fn pixel(&self, view: CanonicalView, x: u32, y: u32) -> Result<[u8; 4], EditorError> {
        let source = &self.state.sources[self.source_index(view)?];
        let index = pixel_index(source, view, x, y)?;
        Ok(source.rgba()[index])
    }

    fn source_index(&self, view: CanonicalView) -> Result<usize, EditorError> {
        self.state
            .sources
            .iter()
            .position(|source| source.view() == view)
            .ok_or(EditorError::SourceNotFound(view))
    }

    fn write_live_pixel(
        &mut self,
        view: CanonicalView,
        x: u32,
        y: u32,
        replacement: [u8; 4],
    ) -> Result<bool, EditorError> {
        let source_index = self.source_index(view)?;
        let source = &self.state.sources[source_index];
        let index = pixel_index(source, view, x, y)?;
        if source.rgba()[index] == replacement {
            return Ok(false);
        }
        let (width, height) = source.dimensions();
        let mut rgba = source.rgba().to_vec();
        rgba[index] = replacement;
        self.state.sources[source_index] = SourceSprite::from_rgba(view, width, height, rgba)?;
        self.advance_revision();
        Ok(true)
    }
}

fn pixel_index(
    source: &SourceSprite,
    view: CanonicalView,
    x: u32,
    y: u32,
) -> Result<usize, EditorError> {
    let (width, height) = source.dimensions();
    if x >= width || y >= height {
        return Err(EditorError::PixelOutOfBounds { view, x, y });
    }
    Ok((y as usize) * (width as usize) + (x as usize))
}

fn flood_indices(
    width: u32,
    height: u32,
    start: usize,
    mut replace_if_target: impl FnMut(usize) -> bool,
) {
    let width = width as usize;
    let height = height as usize;
    let mut stack = vec![start];
    while let Some(index) = stack.pop() {
        if !replace_if_target(index) {
            continue;
        }
        let x = index % width;
        let y = index / width;
        if x > 0 {
            stack.push(index - 1);
        }
        if x + 1 < width {
            stack.push(index + 1);
        }
        if y > 0 {
            stack.push(index - width);
        }
        if y + 1 < height {
            stack.push(index + width);
        }
    }
}
