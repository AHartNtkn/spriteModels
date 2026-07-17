use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart};

use crate::{ImportError, Lighting, Triangle, TriangleScene, View, light_direction, rasterize};

pub const ALL_VIEWS: [CanonicalView; 6] = [
    CanonicalView::Front,
    CanonicalView::Back,
    CanonicalView::Left,
    CanonicalView::Right,
    CanonicalView::Top,
    CanonicalView::Bottom,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SideMode {
    Capture,
    FromOpposite,
    FromOppositeMirrored,
    Off,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SideModes {
    modes: [SideMode; 6],
}

impl Default for SideModes {
    fn default() -> Self {
        Self {
            modes: [SideMode::Capture; 6],
        }
    }
}

impl SideModes {
    pub fn get(&self, view: CanonicalView) -> SideMode {
        self.modes[view.rank() as usize]
    }

    /// Whether `view` may be set to `FromOpposite`/`FromOppositeMirrored`
    /// in the current state. The single predicate `set` enforces and
    /// `legal_modes` queries, so the two can never drift apart.
    fn allows_from_opposite(&self, view: CanonicalView) -> bool {
        self.get(view.opposite()) == SideMode::Capture
    }

    /// Sets one side's mode. `FromOpposite*` requires the opposite side to
    /// be `Capture`. Moving a side out of `Capture` resets an opposite that
    /// depended on it to `Off`.
    pub fn set(&mut self, view: CanonicalView, mode: SideMode) -> Result<(), ImportError> {
        if matches!(
            mode,
            SideMode::FromOpposite | SideMode::FromOppositeMirrored
        ) && !self.allows_from_opposite(view)
        {
            return Err(ImportError::UnsatisfiedOpposite {
                side: view,
                opposite: view.opposite(),
            });
        }
        self.modes[view.rank() as usize] = mode;
        if mode != SideMode::Capture
            && matches!(
                self.get(view.opposite()),
                SideMode::FromOpposite | SideMode::FromOppositeMirrored
            )
        {
            self.modes[view.opposite().rank() as usize] = SideMode::Off;
        }
        Ok(())
    }

    /// The modes `set` would accept for this side in the current state.
    /// This is the single source of truth the UI queries; `set` remains the
    /// enforcing mutation.
    pub fn legal_modes(&self, view: CanonicalView) -> impl Iterator<Item = SideMode> + '_ {
        [
            SideMode::Capture,
            SideMode::FromOpposite,
            SideMode::FromOppositeMirrored,
            SideMode::Off,
        ]
        .into_iter()
        .filter(move |mode| {
            !matches!(
                mode,
                SideMode::FromOpposite | SideMode::FromOppositeMirrored
            ) || self.allows_from_opposite(view)
        })
    }

    pub fn validate(&self) -> Result<(), ImportError> {
        for view in ALL_VIEWS {
            if matches!(
                self.get(view),
                SideMode::FromOpposite | SideMode::FromOppositeMirrored
            ) && !self.allows_from_opposite(view)
            {
                return Err(ImportError::UnsatisfiedOpposite {
                    side: view,
                    opposite: view.opposite(),
                });
            }
        }
        if ALL_VIEWS
            .iter()
            .all(|&view| self.get(view) != SideMode::Capture)
        {
            return Err(ImportError::NoCaptureSides);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq)]
pub struct ImportSettings {
    /// Rotation applied to the mesh before fitting (mesh -> box frame).
    pub rotation: [[f32; 3]; 3],
    pub side_modes: SideModes,
    pub longest_axis_pixels: u32,
    pub light_azimuth_degrees: f32,
    pub light_elevation_degrees: f32,
    pub ambient: f32,
}

impl Default for ImportSettings {
    fn default() -> Self {
        Self {
            rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            side_modes: SideModes::default(),
            longest_axis_pixels: 63,
            // Spec defaults: light from upper-front-left, ambient 0.25.
            light_azimuth_degrees: -35.0,
            light_elevation_degrees: 35.0,
            ambient: 0.25,
        }
    }
}

fn rotate(rotation: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        rotation[0][0] * v[0] + rotation[0][1] * v[1] + rotation[0][2] * v[2],
        rotation[1][0] * v[0] + rotation[1][1] * v[1] + rotation[1][2] * v[2],
        rotation[2][0] * v[0] + rotation[2][1] * v[1] + rotation[2][2] * v[2],
    ]
}

struct Fit {
    bounds: Bounds,
    scale: f32,
    rotated_min: [f32; 3],
    offset: [f32; 3],
}

fn fit(
    scene: &TriangleScene,
    rotation: [[f32; 3]; 3],
    longest_axis_pixels: u32,
) -> Result<Fit, ImportError> {
    if !(1..=63).contains(&longest_axis_pixels) {
        return Err(ImportError::LongestAxisRange(longest_axis_pixels));
    }
    if scene.triangles.is_empty() {
        return Err(ImportError::NoTriangles);
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for tri in &scene.triangles {
        for &p in &tri.positions {
            let r = rotate(rotation, p);
            for axis in 0..3 {
                min[axis] = min[axis].min(r[axis]);
                max[axis] = max[axis].max(r[axis]);
            }
        }
    }
    let extents = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let longest = extents[0].max(extents[1]).max(extents[2]);
    // A scene of coincident points has no extent to scale; scale 1 keeps
    // the (single-texel) geometry finite.
    let scale = if longest > 0.0 {
        longest_axis_pixels as f32 / longest
    } else {
        1.0
    };
    // ceil so the mesh always fits inside the box (fitting itself never
    // clamps); min() removes float overshoot past the mathematical bound
    // extents * scale <= longest_axis_pixels; floor of 1 for flat axes.
    let dim = |extent: f32| -> u32 {
        ((extent * scale).ceil() as u32)
            .min(longest_axis_pixels)
            .max(1)
    };
    let bounds = Bounds::new(dim(extents[0]), dim(extents[1]), dim(extents[2]))?;
    let dims = [
        bounds.width() as f32,
        bounds.height() as f32,
        bounds.depth() as f32,
    ];
    // Center the mesh inside the box on each axis.
    let offset = [
        (dims[0] - extents[0] * scale) / 2.0,
        (dims[1] - extents[1] * scale) / 2.0,
        (dims[2] - extents[2] * scale) / 2.0,
    ];
    Ok(Fit {
        bounds,
        scale,
        rotated_min: min,
        offset,
    })
}

fn to_box_space(scene: &TriangleScene, rotation: [[f32; 3]; 3], fit: &Fit) -> Vec<Triangle> {
    scene
        .triangles
        .iter()
        .map(|tri| {
            let mut out = *tri;
            for vertex in 0..3 {
                let r = rotate(rotation, tri.positions[vertex]);
                out.positions[vertex] = [
                    (r[0] - fit.rotated_min[0]) * fit.scale + fit.offset[0],
                    (r[1] - fit.rotated_min[1]) * fit.scale + fit.offset[1],
                    (r[2] - fit.rotated_min[2]) * fit.scale + fit.offset[2],
                ];
                // Uniform scale + rotation: normals rotate, no re-scaling.
                out.normals[vertex] = rotate(rotation, tri.normals[vertex]);
            }
            out
        })
        .collect()
}

pub fn derived_bounds(
    scene: &TriangleScene,
    rotation: [[f32; 3]; 3],
    longest_axis_pixels: u32,
) -> Result<Bounds, ImportError> {
    Ok(fit(scene, rotation, longest_axis_pixels)?.bounds)
}

/// The box-space triangle scene plus bounds, shared by capture and the
/// dialog's mesh preview so both always show identical geometry.
pub fn box_space_scene(
    scene: &TriangleScene,
    rotation: [[f32; 3]; 3],
    longest_axis_pixels: u32,
) -> Result<(TriangleScene, Bounds), ImportError> {
    let fit = fit(scene, rotation, longest_axis_pixels)?;
    let triangles = to_box_space(scene, rotation, &fit);
    Ok((
        TriangleScene {
            triangles,
            materials: scene.materials.clone(),
        },
        fit.bounds,
    ))
}

pub fn convert(
    scene: &TriangleScene,
    settings: &ImportSettings,
) -> Result<AuthoredModel, ImportError> {
    settings.side_modes.validate()?;
    let (box_scene, bounds) =
        box_space_scene(scene, settings.rotation, settings.longest_axis_pixels)?;
    convert_box_space(&box_scene, bounds, settings)
}

/// The capture half of `convert`, taking an already-fitted box-space scene
/// and its bounds directly. Shared with the import dialog, which computes
/// `box_space_scene` once for its mesh preview and feeds the same result
/// here instead of paying for the mesh -> box-space transform twice per
/// settings change.
pub fn convert_box_space(
    box_scene: &TriangleScene,
    bounds: Bounds,
    settings: &ImportSettings,
) -> Result<AuthoredModel, ImportError> {
    settings.side_modes.validate()?;
    let lighting = Lighting {
        direction: light_direction(
            settings.light_azimuth_degrees,
            settings.light_elevation_degrees,
        ),
        ambient: settings.ambient,
    };

    let mut charts = Vec::new();
    for view in ALL_VIEWS {
        if settings.side_modes.get(view) != SideMode::Capture {
            continue;
        }
        let frame = view.frame(bounds);
        let (width, height) = view.dimensions(bounds);
        let raster = rasterize(
            box_scene,
            &View {
                origin: frame.origin.map(|c| c as f32),
                right: frame.source_u.map(|c| c as f32),
                down: frame.source_v.map(|c| c as f32),
                forward: frame.inward.map(|c| c as f32),
                scale: 1.0,
                width,
                height,
            },
            &lighting,
        );
        let h_max = i64::from(view.maximum_inward_depth(bounds));
        let rgba: Vec<[u8; 4]> = raster
            .depth
            .iter()
            .zip(raster.color.iter())
            .map(|(&depth, &color)| {
                if depth == f32::INFINITY {
                    [0, 0, 0, 0]
                } else {
                    // depth is in model pixels from the face plane; float
                    // error can dip epsilon-negative, clamp handles it.
                    let relief = ((f64::from(depth) * 8.0).round() as i64).clamp(0, h_max);
                    [color[0], color[1], color[2], (255 - relief) as u8]
                }
            })
            .collect();
        let mut chart = Chart::from_rgba(view, width, height, rgba)?;
        let opposite_mode = settings.side_modes.get(view.opposite());
        if opposite_mode == SideMode::FromOpposite {
            chart = chart.with_opposite_assignment();
        }
        if opposite_mode == SideMode::FromOppositeMirrored {
            chart = chart.with_opposite_assignment().with_mirrored_opposite();
        }
        charts.push(chart);
    }
    Ok(AuthoredModel::new(bounds, charts)?)
}
