use relief_core::CanonicalView;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RenderDiagnostic {
    ReliefBeyondOpposingPlane {
        view: CanonicalView,
        source_x: u32,
        source_y: u32,
    },
    EqualDepthColorConflict {
        x: u32,
        y: u32,
        first: CanonicalView,
        second: CanonicalView,
    },
    WarpFold {
        view: CanonicalView,
        source_x: u32,
        source_y: u32,
    },
    HeavyChartOverlap {
        covered_pixels: u32,
        conflicting_pixels: u32,
    },
    InsufficientCoverage {
        covered_pixels: u32,
        total_pixels: u32,
    },
}
