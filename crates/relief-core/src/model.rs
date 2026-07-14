use thiserror::Error;

use crate::{
    Bounds, CanonicalView, Chart, ChartEdge, ChartError, DiscardPolicy, ReassignMode, ResizeDelta,
    ResizeRequest, WorldAxis,
};

pub const EMPTY_RGBA: [u8; 4] = [255, 0, 255, 0];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredModel {
    bounds: Bounds,
    charts: Vec<Chart>,
}

impl AuthoredModel {
    pub fn new(bounds: Bounds, mut charts: Vec<Chart>) -> Result<Self, ModelError> {
        if !(1..=6).contains(&charts.len()) {
            return Err(ModelError::ChartCount(charts.len()));
        }

        charts.sort_by_key(|chart| chart.view().rank());
        for adjacent in charts.windows(2) {
            if adjacent[0].view() == adjacent[1].view() {
                return Err(ModelError::DuplicateView(adjacent[0].view()));
            }
        }
        for chart in &charts {
            validate_chart(bounds, chart)?;
        }

        Ok(Self { bounds, charts })
    }

    pub fn with_empty_chart(bounds: Bounds, view: CanonicalView) -> Result<Self, ModelError> {
        Self::new(bounds, vec![empty_chart(bounds, view)?])
    }

    pub const fn bounds(&self) -> Bounds {
        self.bounds
    }

    pub fn charts(&self) -> &[Chart] {
        &self.charts
    }

    pub fn chart(&self, view: CanonicalView) -> Option<&Chart> {
        self.charts
            .binary_search_by_key(&view.rank(), |chart| chart.view().rank())
            .ok()
            .map(|index| &self.charts[index])
    }

    pub fn add_chart(&mut self, chart: Chart) -> Result<(), ModelError> {
        let mut charts = self.charts.clone();
        charts.push(chart);
        *self = Self::new(self.bounds, charts)?;
        Ok(())
    }

    pub fn add_empty_chart(&mut self, view: CanonicalView) -> Result<(), ModelError> {
        self.add_chart(empty_chart(self.bounds, view)?)
    }

    pub fn replace_chart(&mut self, chart: Chart) -> Result<(), ModelError> {
        let Some(index) = self
            .charts
            .iter()
            .position(|current| current.view() == chart.view())
        else {
            return Err(ModelError::MissingView(chart.view()));
        };

        let mut charts = self.charts.clone();
        charts[index] = chart;
        *self = Self::new(self.bounds, charts)?;
        Ok(())
    }

    pub fn remove_chart(&mut self, view: CanonicalView) -> Result<(), ModelError> {
        let Some(index) = self.charts.iter().position(|chart| chart.view() == view) else {
            return Err(ModelError::MissingView(view));
        };
        if self.charts.len() == 1 {
            return Err(ModelError::LastChart);
        }

        self.charts.remove(index);
        Ok(())
    }

    pub fn set_rgba(&mut self, view: CanonicalView, rgba: Vec<[u8; 4]>) -> Result<(), ModelError> {
        let Some(chart) = self.chart(view) else {
            return Err(ModelError::MissingView(view));
        };
        let (width, height) = chart.dimensions();
        self.replace_chart(Chart::from_rgba(view, width, height, rgba)?)
    }

    pub fn resolve(&self) -> ResolvedCharts {
        let mut charts = Vec::with_capacity(6);
        for rank in 0..6 {
            let view = CanonicalView::from_rank(rank).expect("canonical ranks 0..6 are defined");
            let Some(source) = self.chart(view).or_else(|| self.chart(view.opposite())) else {
                continue;
            };
            let (width, height) = source.dimensions();
            charts.push(
                Chart::from_rgba(view, width, height, source.rgba().to_vec())
                    .expect("resolved charts preserve validated pixel dimensions"),
            );
        }
        ResolvedCharts {
            bounds: self.bounds,
            charts,
        }
    }

    pub fn resize(
        &mut self,
        request: ResizeRequest,
        policy: DiscardPolicy,
    ) -> Result<(), ModelError> {
        let world_edge = request.view.world_edge(request.edge);
        let change = match request.delta {
            ResizeDelta::Add => 1,
            ResizeDelta::Remove => -1,
        };
        let dimensions = [
            self.bounds.width() as i64,
            self.bounds.height() as i64,
            self.bounds.depth() as i64,
        ];
        let axis = match world_edge.axis {
            WorldAxis::X => 0,
            WorldAxis::Y => 1,
            WorldAxis::Z => 2,
        };
        let mut prospective = dimensions;
        prospective[axis] += change;
        let new_bounds = Bounds::new(
            prospective[0] as u32,
            prospective[1] as u32,
            prospective[2] as u32,
        )?;

        let affected = self
            .charts
            .iter()
            .enumerate()
            .filter_map(|(index, chart)| {
                chart
                    .view()
                    .image_edge(world_edge)
                    .map(|edge| (index, edge))
            })
            .collect::<Vec<_>>();

        if request.delta == ResizeDelta::Remove && policy == DiscardPolicy::Reject {
            let edges = affected
                .iter()
                .filter_map(|&(index, edge)| {
                    self.charts[index]
                        .edge_contains_authored_pixel(edge)
                        .then_some(ChartEdge {
                            view: self.charts[index].view(),
                            edge,
                        })
                })
                .collect::<Vec<_>>();
            if !edges.is_empty() {
                return Err(ModelError::ResizeWouldDiscard { edges });
            }
        }

        let mut charts = self.charts.clone();
        for (index, edge) in affected {
            charts[index] = charts[index].resized(edge, request.delta);
        }
        let replacement = Self::new(new_bounds, charts)?;
        *self = replacement;
        Ok(())
    }

    pub fn reassign_chart(
        &mut self,
        from: CanonicalView,
        to: CanonicalView,
        mode: ReassignMode,
    ) -> Result<(), ModelError> {
        let Some(index) = self.charts.iter().position(|chart| chart.view() == from) else {
            return Err(ModelError::MissingView(from));
        };
        if self.chart(to).is_some() {
            return Err(ModelError::DuplicateView(to));
        }

        let source = &self.charts[index];
        let target = match mode {
            ReassignMode::Preserve => {
                let expected = to.dimensions(self.bounds);
                let actual = source.dimensions();
                if actual != expected {
                    return Err(ModelError::DimensionMismatch {
                        view: to,
                        expected,
                        actual,
                    });
                }
                Chart::from_rgba(to, actual.0, actual.1, source.rgba().to_vec())?
            }
            ReassignMode::RecreateEmpty => empty_chart(self.bounds, to)?,
        };

        let mut charts = self.charts.clone();
        charts[index] = target;
        let replacement = Self::new(self.bounds, charts)?;
        *self = replacement;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCharts {
    bounds: Bounds,
    charts: Vec<Chart>,
}

impl ResolvedCharts {
    pub const fn bounds(&self) -> Bounds {
        self.bounds
    }

    pub fn charts(&self) -> &[Chart] {
        &self.charts
    }

    pub fn chart(&self, view: CanonicalView) -> Option<&Chart> {
        self.charts
            .binary_search_by_key(&view.rank(), |chart| chart.view().rank())
            .ok()
            .map(|index| &self.charts[index])
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ModelError {
    #[error("model must contain between one and six authored charts, got {0}")]
    ChartCount(usize),
    #[error("model already contains {0:?}")]
    DuplicateView(CanonicalView),
    #[error("model has no authored {0:?} chart")]
    MissingView(CanonicalView),
    #[error("the last authored chart cannot be removed")]
    LastChart,
    #[error("{view:?} dimensions {actual:?} do not match {expected:?}")]
    DimensionMismatch {
        view: CanonicalView,
        expected: (u32, u32),
        actual: (u32, u32),
    },
    #[error("{view:?} pixel ({x}, {y}) has inward depth {actual}, above maximum {maximum}")]
    ReliefBeyondMaximum {
        view: CanonicalView,
        x: u32,
        y: u32,
        actual: u8,
        maximum: u8,
    },
    #[error("resizing would discard authored pixels on {edges:?}")]
    ResizeWouldDiscard { edges: Vec<ChartEdge> },
    #[error(transparent)]
    Chart(#[from] ChartError),
}

fn empty_chart(bounds: Bounds, view: CanonicalView) -> Result<Chart, ChartError> {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(
        view,
        width,
        height,
        vec![EMPTY_RGBA; (width * height) as usize],
    )
}

fn validate_chart(bounds: Bounds, chart: &Chart) -> Result<(), ModelError> {
    let view = chart.view();
    let expected = view.dimensions(bounds);
    let actual = chart.dimensions();
    if actual != expected {
        return Err(ModelError::DimensionMismatch {
            view,
            expected,
            actual,
        });
    }

    let maximum = view.maximum_inward_depth(bounds);
    let width = actual.0;
    for (index, pixel) in chart.rgba().iter().enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let relief = 255 - pixel[3];
        if relief > maximum {
            return Err(ModelError::ReliefBeyondMaximum {
                view,
                x: index as u32 % width,
                y: index as u32 / width,
                actual: relief,
                maximum,
            });
        }
    }
    Ok(())
}
