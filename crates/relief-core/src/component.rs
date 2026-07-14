use std::collections::VecDeque;

use crate::{Chart, DecodedTexel};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentId(u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentMap {
    width: u32,
    height: u32,
    labels: Vec<Option<ComponentId>>,
}

impl ComponentMap {
    pub fn label(chart: &Chart) -> Self {
        let (width, height) = chart.dimensions();
        let mut labels = vec![None; (width as usize) * (height as usize)];
        let mut next_id = 0;

        for y in 0..height {
            for x in 0..width {
                let index = (y * width + x) as usize;
                if labels[index].is_some()
                    || !matches!(chart.texel_at(x, y), Some(DecodedTexel::Relief { .. }))
                {
                    continue;
                }

                let component = ComponentId(next_id);
                next_id += 1;
                labels[index] = Some(component);

                let mut pending = VecDeque::from([(x, y)]);
                while let Some((current_x, current_y)) = pending.pop_front() {
                    for (neighbor_x, neighbor_y) in neighbors(current_x, current_y, width, height) {
                        let neighbor_index = (neighbor_y * width + neighbor_x) as usize;
                        if labels[neighbor_index].is_none()
                            && matches!(
                                chart.texel_at(neighbor_x, neighbor_y),
                                Some(DecodedTexel::Relief { .. })
                            )
                        {
                            labels[neighbor_index] = Some(component);
                            pending.push_back((neighbor_x, neighbor_y));
                        }
                    }
                }
            }
        }

        Self {
            width,
            height,
            labels,
        }
    }

    pub fn at(&self, x: u32, y: u32) -> Option<ComponentId> {
        (x < self.width && y < self.height)
            .then(|| self.labels[(y * self.width + x) as usize])
            .flatten()
    }
}

fn neighbors(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut neighbors = [(0, 0); 4];
    let mut count = 0;

    if x > 0 {
        neighbors[count] = (x - 1, y);
        count += 1;
    }
    if x + 1 < width {
        neighbors[count] = (x + 1, y);
        count += 1;
    }
    if y > 0 {
        neighbors[count] = (x, y - 1);
        count += 1;
    }
    if y + 1 < height {
        neighbors[count] = (x, y + 1);
        count += 1;
    }

    neighbors.into_iter().take(count)
}
