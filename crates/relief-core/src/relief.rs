use num_rational::Ratio;

use crate::{Chart, ComponentId, ComponentMap, DecodedTexel, SourcePoint, rational::abs_ratio};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReliefField {
    width: u32,
    height: u32,
    rgb: Vec<Option<[u8; 3]>>,
    relief: Vec<Option<u8>>,
    components: ComponentMap,
}

impl ReliefField {
    pub fn new(chart: &Chart) -> Self {
        let (width, height) = chart.dimensions();
        let mut rgb = Vec::with_capacity(chart.texels().len());
        let mut relief = Vec::with_capacity(chart.texels().len());

        for texel in chart.texels() {
            match texel {
                DecodedTexel::Background => {
                    rgb.push(None);
                    relief.push(None);
                }
                DecodedTexel::Relief {
                    rgb: color,
                    eighths,
                } => {
                    rgb.push(Some(*color));
                    relief.push(Some(*eighths));
                }
            }
        }

        Self {
            width,
            height,
            rgb,
            relief,
            components: ComponentMap::label(chart),
        }
    }

    pub fn sample(&self, x: Ratio<i64>, y: Ratio<i64>) -> Option<Ratio<i64>> {
        let zero = Ratio::from_integer(0);
        if x < zero || y < zero {
            return None;
        }

        let cell_x = x.to_integer();
        let cell_y = y.to_integer();
        if cell_x < 0
            || cell_y < 0
            || cell_x >= i64::from(self.width)
            || cell_y >= i64::from(self.height)
        {
            return None;
        }

        let component = self.components.at(cell_x as u32, cell_y as u32)?;
        self.sample_component(SourcePoint::new(x, y), component)
    }

    pub fn foreground_cell(&self, x: u32, y: u32) -> Option<ForegroundCell<'_>> {
        let component = self.components.at(x, y)?;
        Some(ForegroundCell {
            field: self,
            x,
            y,
            component,
        })
    }

    fn sample_component(&self, point: SourcePoint, component: ComponentId) -> Option<Ratio<i64>> {
        let SourcePoint { x, y } = point;
        let cell_x = x.to_integer();
        let cell_y = y.to_integer();
        let mut weighted = Ratio::from_integer(0);
        let mut total = Ratio::from_integer(0);

        for sample_y in (cell_y - 1).max(0)..=(cell_y + 1).min(i64::from(self.height) - 1) {
            for sample_x in (cell_x - 1).max(0)..=(cell_x + 1).min(i64::from(self.width) - 1) {
                if self.components.at(sample_x as u32, sample_y as u32) != Some(component) {
                    continue;
                }

                let center_x = Ratio::new(2 * sample_x + 1, 2);
                let center_y = Ratio::new(2 * sample_y + 1, 2);
                let weight_x =
                    (Ratio::from_integer(1) - abs_ratio(x - center_x)).max(Ratio::from_integer(0));
                let weight_y =
                    (Ratio::from_integer(1) - abs_ratio(y - center_y)).max(Ratio::from_integer(0));
                let weight = weight_x * weight_y;
                let index = (sample_y as u32 * self.width + sample_x as u32) as usize;
                let sample_relief = Ratio::from_integer(i64::from(self.relief[index]?));

                weighted += sample_relief * weight;
                total += weight;
            }
        }

        (total != Ratio::from_integer(0)).then(|| weighted / total)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ForegroundCell<'a> {
    field: &'a ReliefField,
    x: u32,
    y: u32,
    component: ComponentId,
}

impl ForegroundCell<'_> {
    pub fn sample_closure(&self, point: SourcePoint) -> Option<Ratio<i64>> {
        let left = Ratio::from_integer(i64::from(self.x));
        let right = Ratio::from_integer(i64::from(self.x) + 1);
        let top = Ratio::from_integer(i64::from(self.y));
        let bottom = Ratio::from_integer(i64::from(self.y) + 1);

        if point.x < left || point.x > right || point.y < top || point.y > bottom {
            return None;
        }

        self.field.sample_component(point, self.component)
    }
}
