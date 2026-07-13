use num_rational::Ratio;
use relief_core::{Bounds, Chart};
use thiserror::Error;

use crate::diagnostic::normalize_diagnostics;
use crate::{
    CameraBasis, FrameBuffer, RenderDiagnostic, RenderError, RenderRequest, TargetView,
    render_model,
};

const MAX_RELIEF_EIGHTHS: u32 = 254;
const RELIEF_MARGIN_PIXELS: u32 = MAX_RELIEF_EIGHTHS.div_ceil(8);

// Version 1 advances clockwise around Y-down from Front (+Z) toward Right (-X).
// Each pair is (sin, cos) over the fixed denominator 256.
const HORIZONTAL_V1: [(i64, i64); 16] = [
    (0, 256),
    (98, 237),
    (181, 181),
    (237, 98),
    (256, 0),
    (237, -98),
    (181, -181),
    (98, -237),
    (0, -256),
    (-98, -237),
    (-181, -181),
    (-237, -98),
    (-256, 0),
    (-237, 98),
    (-181, 181),
    (-98, 237),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectionCount {
    Eight,
    Sixteen,
}

impl DirectionCount {
    pub const fn frame_count(self) -> usize {
        match self {
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetRequest {
    direction_count: DirectionCount,
    integer_scale: u32,
    padding: u32,
    elevation_index: u8,
}

impl SheetRequest {
    pub fn new(
        direction_count: DirectionCount,
        integer_scale: u32,
        padding: u32,
        elevation_index: u8,
    ) -> Result<Self, SheetError> {
        if integer_scale == 0 {
            return Err(SheetError::ZeroScale);
        }
        if elevation_index != 1 {
            return Err(SheetError::UnsupportedElevation(elevation_index));
        }
        Ok(Self {
            direction_count,
            integer_scale,
            padding,
            elevation_index,
        })
    }

    pub fn direction_count(&self) -> DirectionCount {
        self.direction_count
    }

    pub fn integer_scale(&self) -> u32 {
        self.integer_scale
    }

    pub fn padding(&self) -> u32 {
        self.padding
    }

    pub fn elevation_index(&self) -> u8 {
        self.elevation_index
    }

    pub fn target_view(&self, index: usize) -> Option<TargetView> {
        if index >= self.direction_count.frame_count() {
            return None;
        }
        let table_index = match self.direction_count {
            DirectionCount::Eight => index * 2,
            DirectionCount::Sixteen => index,
        };
        Some(target_v1(HORIZONTAL_V1[table_index]))
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SheetError {
    #[error("sheet scale must be a positive integer")]
    ZeroScale,
    #[error("unsupported directional elevation preset {0}; version 1 requires elevation 1")]
    UnsupportedElevation(u8),
    #[error("chart bounds {actual:?} do not match sheet bounds {expected:?}")]
    MixedBounds { expected: Bounds, actual: Bounds },
    #[error("sheet dimensions overflow addressable storage")]
    FrameBufferTooLarge,
    #[error(transparent)]
    Render(#[from] RenderError),
}

pub fn render_sheet(
    charts: &[Chart],
    bounds: Bounds,
    request: &SheetRequest,
) -> Result<FrameBuffer, SheetError> {
    for chart in charts {
        if chart.bounds() != bounds {
            return Err(SheetError::MixedBounds {
                expected: bounds,
                actual: chart.bounds(),
            });
        }
    }

    let frame_side = bounds
        .width()
        .max(bounds.height())
        .max(bounds.depth())
        .checked_add(2 * RELIEF_MARGIN_PIXELS)
        .ok_or(SheetError::FrameBufferTooLarge)?;
    let scaled_side = frame_side
        .checked_mul(request.integer_scale)
        .ok_or(SheetError::FrameBufferTooLarge)?;
    let cell_side = scaled_side
        .checked_add(
            request
                .padding
                .checked_mul(2)
                .ok_or(SheetError::FrameBufferTooLarge)?,
        )
        .ok_or(SheetError::FrameBufferTooLarge)?;
    let sheet_width = cell_side
        .checked_mul(request.direction_count.frame_count() as u32)
        .ok_or(SheetError::FrameBufferTooLarge)?;
    let pixel_count = (sheet_width as usize)
        .checked_mul(cell_side as usize)
        .ok_or(SheetError::FrameBufferTooLarge)?;
    if pixel_count > isize::MAX as usize {
        return Err(SheetError::FrameBufferTooLarge);
    }

    let mut sheet = FrameBuffer::transparent(sheet_width, cell_side);
    for index in 0..request.direction_count.frame_count() {
        let target = request
            .target_view(index)
            .expect("validated direction index has a version-1 target");
        let frame = render_model(charts, &RenderRequest::new(frame_side, frame_side, target))?;
        blit_nearest(
            &frame,
            &mut sheet,
            index as u32 * cell_side + request.padding,
            request.padding,
            request.integer_scale,
        );
        append_diagnostics(
            frame.diagnostics(),
            &mut sheet.diagnostics,
            index as u32 * cell_side + request.padding,
            request.padding,
            request.integer_scale,
        );
    }
    normalize_diagnostics(&mut sheet.diagnostics);
    Ok(sheet)
}

fn append_diagnostics(
    source: &[RenderDiagnostic],
    destination: &mut Vec<RenderDiagnostic>,
    destination_x: u32,
    destination_y: u32,
    scale: u32,
) {
    for diagnostic in source {
        if let RenderDiagnostic::EqualDepthColorConflict {
            x,
            y,
            first,
            second,
        } = diagnostic
        {
            for scale_y in 0..scale {
                for scale_x in 0..scale {
                    destination.push(RenderDiagnostic::EqualDepthColorConflict {
                        x: destination_x + x * scale + scale_x,
                        y: destination_y + y * scale + scale_y,
                        first: *first,
                        second: *second,
                    });
                }
            }
        } else {
            destination.push(diagnostic.clone());
        }
    }
}

fn target_v1((sin_numerator, cos_numerator): (i64, i64)) -> TargetView {
    let sin = Ratio::new(sin_numerator, 256);
    let cos = Ratio::new(cos_numerator, 256);

    // These fixed scales make direction index 2 exactly TargetView::isometric_v1,
    // while all other indices retain that preset's elevation and projection scale.
    let screen_scale = Ratio::new(128, 181);
    let down_horizontal_scale = Ratio::new(64, 181);
    let depth_horizontal_scale = Ratio::new(256, 543);
    TargetView::from_camera(CameraBasis::new(
        [
            cos * screen_scale,
            Ratio::from_integer(0),
            sin * screen_scale,
        ],
        [
            sin * down_horizontal_scale,
            Ratio::new(1, 2),
            -cos * down_horizontal_scale,
        ],
        [
            -sin * depth_horizontal_scale,
            Ratio::new(1, 3),
            cos * depth_horizontal_scale,
        ],
    ))
}

fn blit_nearest(
    source: &FrameBuffer,
    destination: &mut FrameBuffer,
    destination_x: u32,
    destination_y: u32,
    scale: u32,
) {
    for source_y in 0..source.height() {
        for source_x in 0..source.width() {
            let source_index = (source_y * source.width() + source_x) as usize;
            for scale_y in 0..scale {
                for scale_x in 0..scale {
                    let x = destination_x + source_x * scale + scale_x;
                    let y = destination_y + source_y * scale + scale_y;
                    let destination_index = (y * destination.width() + x) as usize;
                    destination.rgba[destination_index] = source.rgba[source_index];
                    destination.keys[destination_index] = source.keys[source_index].clone();
                }
            }
        }
    }
}
