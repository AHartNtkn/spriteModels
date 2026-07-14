use relief_core::{CanonicalView, Chart};

use crate::{EditorError, SourceSprite};

pub const fn opposite(view: CanonicalView) -> CanonicalView {
    match view {
        CanonicalView::Front => CanonicalView::Back,
        CanonicalView::Back => CanonicalView::Front,
        CanonicalView::Left => CanonicalView::Right,
        CanonicalView::Right => CanonicalView::Left,
        CanonicalView::Top => CanonicalView::Bottom,
        CanonicalView::Bottom => CanonicalView::Top,
    }
}

pub(crate) fn resolve_charts(sources: &[SourceSprite]) -> Result<Vec<Chart>, EditorError> {
    let mut charts = Vec::with_capacity(6);
    for rank in 0..6 {
        let view = CanonicalView::from_rank(rank).expect("all canonical ranks are present");
        let source = sources
            .iter()
            .find(|source| source.view() == view)
            .or_else(|| {
                sources
                    .iter()
                    .find(|source| source.view() == opposite(view))
            });
        if let Some(source) = source {
            charts.push(source.to_chart_for(view)?);
        }
    }
    Ok(charts)
}
