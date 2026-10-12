use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthConvention {
    ReversedZeroToOne,
}

impl DepthConvention {
    pub const fn clear_depth(self) -> f32 {
        match self {
            Self::ReversedZeroToOne => 0.0,
        }
    }

    pub const fn near_is_greater(self) -> bool {
        match self {
            Self::ReversedZeroToOne => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameMatrix {
    WorldToView,
    ViewRotation,
    ViewToClip,
    WorldToClip,
}

/// MC 26.2's complete 40-byte terrain fog payload before backend packing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainFog {
    color: [f32; 4],
    environmental_range: [f32; 2],
    render_distance_range: [f32; 2],
    sky_end: f32,
    cloud_end: f32,
}

impl TerrainFog {
    pub fn new(
        color: [f32; 4],
        environmental_range: [f32; 2],
        render_distance_range: [f32; 2],
        sky_end: f32,
        cloud_end: f32,
    ) -> Result<Self, TerrainFogValidationError> {
        for (component, value) in color.into_iter().enumerate() {
            if !value.is_finite() {
                return Err(TerrainFogValidationError::NonFiniteColor { component });
            }
        }
        for (name, value) in [
            ("environmental start", environmental_range[0]),
            ("environmental end", environmental_range[1]),
            ("render-distance start", render_distance_range[0]),
            ("render-distance end", render_distance_range[1]),
            ("sky end", sky_end),
            ("cloud end", cloud_end),
        ] {
            if !value.is_finite() {
                return Err(TerrainFogValidationError::NonFiniteDistance { name });
            }
        }
        Ok(Self {
            color,
            environmental_range,
            render_distance_range,
            sky_end,
            cloud_end,
        })
    }

    pub const fn color(self) -> [f32; 4] {
        self.color
    }

    pub const fn environmental_range(self) -> [f32; 2] {
        self.environmental_range
    }

    pub const fn render_distance_range(self) -> [f32; 2] {
        self.render_distance_range
    }

    pub const fn sky_end(self) -> f32 {
        self.sky_end
    }

    pub const fn cloud_end(self) -> f32 {
        self.cloud_end
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TerrainFogValidationError {
    NonFiniteColor { component: usize },
    NonFiniteDistance { name: &'static str },
}

impl Display for TerrainFogValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteColor { component } => {
                write!(formatter, "fog color component {component} is not finite")
            }
            Self::NonFiniteDistance { name } => {
                write!(formatter, "fog {name} is not finite")
            }
        }
    }
}

impl Error for TerrainFogValidationError {}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameView {
    world_to_view: [[f32; 4]; 4],
    view_rotation: [[f32; 4]; 4],
    view_to_clip: [[f32; 4]; 4],
    world_to_clip: [[f32; 4]; 4],
    camera_position: [f32; 3],
    precise_camera_position: [f64; 3],
    camera_block_position: [i32; 3],
    camera_offset: [f32; 3],
    terrain_fog: Option<TerrainFog>,
    viewport: [u32; 2],
    near_clip: f32,
    far_clip: f32,
    sample_count: u32,
    depth_convention: DepthConvention,
}

impl FrameView {
    /// Builds a frame from column-major matrices using column vectors.
    ///
    /// `view_to_clip` must map the camera-space near plane to device depth 1
    /// and the far plane to device depth 0. This rejects forward-Z and OpenGL
    /// `[-1, 1]` projections before they can reach the Vulkan backend.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        world_to_view: [[f32; 4]; 4],
        view_to_clip: [[f32; 4]; 4],
        camera_position: [f32; 3],
        viewport: [u32; 2],
        near_clip: f32,
        far_clip: f32,
        sample_count: u32,
    ) -> Result<Self, FrameViewValidationError> {
        validate_frame_scalars(viewport, near_clip, far_clip, sample_count)?;
        validate_matrix(FrameMatrix::WorldToView, &world_to_view)?;
        validate_matrix(FrameMatrix::ViewToClip, &view_to_clip)?;
        validate_rigid_world_to_view(&world_to_view)?;
        for (axis, coordinate) in camera_position.into_iter().enumerate() {
            if !coordinate.is_finite() {
                return Err(FrameViewValidationError::NonFiniteCameraPosition { axis });
            }
        }
        validate_reverse_z_projection(&view_to_clip, near_clip, far_clip)?;
        validate_camera_origin(&world_to_view, camera_position)?;
        let precise_camera_position = camera_position.map(f64::from);
        let (camera_block_position, camera_offset) =
            split_camera_position(precise_camera_position)?;
        let view_rotation = extract_view_rotation(world_to_view);
        let world_to_clip = multiply_column_major(view_to_clip, world_to_view);
        validate_matrix(FrameMatrix::WorldToClip, &world_to_clip)?;

        Ok(Self {
            world_to_view,
            view_rotation,
            view_to_clip,
            world_to_clip,
            camera_position,
            precise_camera_position,
            camera_block_position,
            camera_offset,
            terrain_fog: None,
            viewport,
            near_clip,
            far_clip,
            sample_count,
            depth_convention: DepthConvention::ReversedZeroToOne,
        })
    }

    /// Builds the production MC 26.2 camera-relative frame contract.
    ///
    /// The view matrix supplied here is rotation-only. World translation stays
    /// split into integer `CameraBlockPos` and fractional `CameraOffset`, so a
    /// section can be transformed on the GPU without losing precision at the
    /// world border. The legacy world matrices remain available only for
    /// diagnostics and must not be used by production terrain lowering.
    #[allow(clippy::too_many_arguments)]
    pub fn new_camera_relative(
        view_rotation: [[f32; 4]; 4],
        view_to_clip: [[f32; 4]; 4],
        precise_camera_position: [f64; 3],
        terrain_fog: TerrainFog,
        viewport: [u32; 2],
        near_clip: f32,
        far_clip: f32,
        sample_count: u32,
    ) -> Result<Self, FrameViewValidationError> {
        validate_frame_scalars(viewport, near_clip, far_clip, sample_count)?;
        validate_matrix(FrameMatrix::ViewRotation, &view_rotation)?;
        validate_matrix(FrameMatrix::ViewToClip, &view_to_clip)?;
        validate_rigid_world_to_view(&view_rotation)?;
        let translation = [
            view_rotation[3][0],
            view_rotation[3][1],
            view_rotation[3][2],
        ];
        if translation
            .iter()
            .any(|coordinate| coordinate.abs() > 1.0e-6)
        {
            return Err(FrameViewValidationError::ViewRotationHasTranslation { translation });
        }
        validate_reverse_z_projection(&view_to_clip, near_clip, far_clip)?;
        let (camera_block_position, camera_offset) =
            split_camera_position(precise_camera_position)?;
        let camera_position = precise_camera_position.map(|coordinate| coordinate as f32);
        let world_to_view = world_to_view_from_rotation(view_rotation, camera_position);
        let world_to_clip = multiply_column_major(view_to_clip, world_to_view);
        validate_matrix(FrameMatrix::WorldToView, &world_to_view)?;
        validate_matrix(FrameMatrix::WorldToClip, &world_to_clip)?;

        Ok(Self {
            world_to_view,
            view_rotation,
            view_to_clip,
            world_to_clip,
            camera_position,
            precise_camera_position,
            camera_block_position,
            camera_offset,
            terrain_fog: Some(terrain_fog),
            viewport,
            near_clip,
            far_clip,
            sample_count,
            depth_convention: DepthConvention::ReversedZeroToOne,
        })
    }

    pub const fn world_to_view(&self) -> &[[f32; 4]; 4] {
        &self.world_to_view
    }

    pub const fn view_rotation(&self) -> &[[f32; 4]; 4] {
        &self.view_rotation
    }

    pub const fn view_to_clip(&self) -> &[[f32; 4]; 4] {
        &self.view_to_clip
    }

    pub const fn world_to_clip(&self) -> &[[f32; 4]; 4] {
        &self.world_to_clip
    }

    pub const fn camera_position(&self) -> [f32; 3] {
        self.camera_position
    }

    pub const fn precise_camera_position(&self) -> [f64; 3] {
        self.precise_camera_position
    }

    pub const fn camera_block_position(&self) -> [i32; 3] {
        self.camera_block_position
    }

    pub const fn camera_offset(&self) -> [f32; 3] {
        self.camera_offset
    }

    pub const fn terrain_fog(&self) -> Option<TerrainFog> {
        self.terrain_fog
    }

    pub const fn viewport(&self) -> [u32; 2] {
        self.viewport
    }

    pub const fn near_clip(&self) -> f32 {
        self.near_clip
    }

    pub const fn far_clip(&self) -> f32 {
        self.far_clip
    }

    pub const fn sample_count(&self) -> u32 {
        self.sample_count
    }

    pub const fn depth_convention(&self) -> DepthConvention {
        self.depth_convention
    }

    /// Returns positive camera-space depth (`-view_z`) for points in front of the camera.
    pub fn linear_view_depth(&self, world_position: [f32; 3]) -> Option<f32> {
        if world_position
            .into_iter()
            .any(|coordinate| !coordinate.is_finite())
        {
            return None;
        }

        let view = transform_point(self.world_to_view, world_position);
        let homogeneous_w = view[3];
        if !homogeneous_w.is_finite() || homogeneous_w == 0.0 {
            return None;
        }
        let view_z = view[2] / homogeneous_w;
        let depth = -view_z;
        (depth.is_finite() && depth > 0.0).then_some(depth)
    }
}

fn validate_frame_scalars(
    viewport: [u32; 2],
    near_clip: f32,
    far_clip: f32,
    sample_count: u32,
) -> Result<(), FrameViewValidationError> {
    if viewport[0] == 0 || viewport[1] == 0 {
        return Err(FrameViewValidationError::ZeroViewport { viewport });
    }
    if !near_clip.is_finite() || !far_clip.is_finite() || near_clip <= 0.0 || far_clip <= near_clip
    {
        return Err(FrameViewValidationError::InvalidClipPlanes {
            near_clip,
            far_clip,
        });
    }
    if sample_count != 1 {
        return Err(FrameViewValidationError::UnsupportedSampleCount { sample_count });
    }
    Ok(())
}

fn split_camera_position(
    camera_position: [f64; 3],
) -> Result<([i32; 3], [f32; 3]), FrameViewValidationError> {
    let mut block_position = [0_i32; 3];
    let mut offset = [0.0_f32; 3];
    for (axis, coordinate) in camera_position.into_iter().enumerate() {
        if !coordinate.is_finite() {
            return Err(FrameViewValidationError::NonFinitePreciseCameraPosition { axis });
        }
        let floored = coordinate.floor();
        if floored < f64::from(i32::MIN) || floored > f64::from(i32::MAX) {
            return Err(FrameViewValidationError::CameraBlockPositionOutOfRange {
                axis,
                coordinate,
            });
        }
        block_position[axis] = floored as i32;
        offset[axis] = (f64::from(block_position[axis]) - coordinate) as f32;
    }
    Ok((block_position, offset))
}

fn extract_view_rotation(mut world_to_view: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    world_to_view[3] = [0.0, 0.0, 0.0, 1.0];
    world_to_view
}

fn world_to_view_from_rotation(
    view_rotation: [[f32; 4]; 4],
    camera_position: [f32; 3],
) -> [[f32; 4]; 4] {
    let mut world_to_view = view_rotation;
    for row in 0..3 {
        world_to_view[3][row] = -(view_rotation[0][row] * camera_position[0]
            + view_rotation[1][row] * camera_position[1]
            + view_rotation[2][row] * camera_position[2]);
    }
    world_to_view
}

fn validate_rigid_world_to_view(
    world_to_view: &[[f32; 4]; 4],
) -> Result<(), FrameViewValidationError> {
    let homogeneous_row = [
        world_to_view[0][3],
        world_to_view[1][3],
        world_to_view[2][3],
        world_to_view[3][3],
    ];
    if homogeneous_row[0].abs() > 1.0e-6
        || homogeneous_row[1].abs() > 1.0e-6
        || homogeneous_row[2].abs() > 1.0e-6
        || (homogeneous_row[3] - 1.0).abs() > 1.0e-6
    {
        return Err(FrameViewValidationError::NonAffineWorldToView { homogeneous_row });
    }

    let basis = [
        [
            world_to_view[0][0],
            world_to_view[0][1],
            world_to_view[0][2],
        ],
        [
            world_to_view[1][0],
            world_to_view[1][1],
            world_to_view[1][2],
        ],
        [
            world_to_view[2][0],
            world_to_view[2][1],
            world_to_view[2][2],
        ],
    ];
    let mut max_orthonormal_error = 0.0_f32;
    for (left_index, left) in basis.iter().enumerate() {
        for (right_index, right) in basis.iter().enumerate().skip(left_index) {
            let expected = if left_index == right_index { 1.0 } else { 0.0 };
            max_orthonormal_error =
                max_orthonormal_error.max((dot(*left, *right) - expected).abs());
        }
    }
    let determinant = dot(basis[0], cross(basis[1], basis[2]));
    if max_orthonormal_error > 1.0e-3 || (determinant - 1.0).abs() > 1.0e-3 {
        Err(FrameViewValidationError::NonRigidWorldToView {
            determinant,
            max_orthonormal_error,
        })
    } else {
        Ok(())
    }
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn validate_reverse_z_projection(
    view_to_clip: &[[f32; 4]; 4],
    near_clip: f32,
    far_clip: f32,
) -> Result<(), FrameViewValidationError> {
    let midpoint = (near_clip as f64 * far_clip as f64).sqrt() as f32;
    let near_depth = projected_device_depth(*view_to_clip, near_clip);
    let midpoint_depth = projected_device_depth(*view_to_clip, midpoint);
    let far_depth = projected_device_depth(*view_to_clip, far_clip);
    let valid = near_depth.is_some_and(|depth| (depth - 1.0).abs() <= 1.0e-4)
        && far_depth.is_some_and(|depth| depth.abs() <= 1.0e-4)
        && matches!(
            (near_depth, midpoint_depth, far_depth),
            (Some(near), Some(mid), Some(far))
                if (0.0..=1.0).contains(&mid) && near > mid && mid > far
        );
    if valid {
        Ok(())
    } else {
        Err(FrameViewValidationError::InvalidReverseZProjection {
            near_depth,
            midpoint_depth,
            far_depth,
        })
    }
}

fn projected_device_depth(view_to_clip: [[f32; 4]; 4], positive_view_depth: f32) -> Option<f32> {
    let clip = transform_point(view_to_clip, [0.0, 0.0, -positive_view_depth]);
    if !clip[2].is_finite() || !clip[3].is_finite() || clip[3] <= 0.0 {
        return None;
    }
    let depth = clip[2] / clip[3];
    depth.is_finite().then_some(depth)
}

fn validate_camera_origin(
    world_to_view: &[[f32; 4]; 4],
    camera_position: [f32; 3],
) -> Result<(), FrameViewValidationError> {
    let view = transform_point(*world_to_view, camera_position);
    if !view[3].is_finite() || view[3] == 0.0 {
        return Err(FrameViewValidationError::CameraPositionMismatch {
            view_position: [f32::NAN; 3],
        });
    }
    let view_position = [view[0] / view[3], view[1] / view[3], view[2] / view[3]];
    if view_position
        .iter()
        .all(|coordinate| coordinate.is_finite() && coordinate.abs() <= 1.0e-4)
    {
        Ok(())
    } else {
        Err(FrameViewValidationError::CameraPositionMismatch { view_position })
    }
}

fn validate_matrix(
    matrix_name: FrameMatrix,
    matrix: &[[f32; 4]; 4],
) -> Result<(), FrameViewValidationError> {
    for (column, values) in matrix.iter().enumerate() {
        for (row, value) in values.iter().enumerate() {
            if !value.is_finite() {
                return Err(FrameViewValidationError::NonFiniteMatrix {
                    matrix: matrix_name,
                    column,
                    row,
                });
            }
        }
    }
    Ok(())
}

fn multiply_column_major(left: [[f32; 4]; 4], right: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut product = [[0.0; 4]; 4];
    for (column, product_column) in product.iter_mut().enumerate() {
        for (row, product_value) in product_column.iter_mut().enumerate() {
            *product_value = (0..4)
                .map(|component| left[component][row] * right[column][component])
                .sum();
        }
    }
    product
}

fn transform_point(matrix: [[f32; 4]; 4], position: [f32; 3]) -> [f32; 4] {
    let vector = [position[0], position[1], position[2], 1.0];
    let mut transformed = [0.0; 4];
    for (row, output) in transformed.iter_mut().enumerate() {
        *output = (0..4)
            .map(|column| matrix[column][row] * vector[column])
            .sum();
    }
    transformed
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum FrameViewValidationError {
    ZeroViewport {
        viewport: [u32; 2],
    },
    InvalidClipPlanes {
        near_clip: f32,
        far_clip: f32,
    },
    UnsupportedSampleCount {
        sample_count: u32,
    },
    NonAffineWorldToView {
        homogeneous_row: [f32; 4],
    },
    NonRigidWorldToView {
        determinant: f32,
        max_orthonormal_error: f32,
    },
    InvalidReverseZProjection {
        near_depth: Option<f32>,
        midpoint_depth: Option<f32>,
        far_depth: Option<f32>,
    },
    NonFiniteMatrix {
        matrix: FrameMatrix,
        column: usize,
        row: usize,
    },
    NonFiniteCameraPosition {
        axis: usize,
    },
    NonFinitePreciseCameraPosition {
        axis: usize,
    },
    CameraBlockPositionOutOfRange {
        axis: usize,
        coordinate: f64,
    },
    ViewRotationHasTranslation {
        translation: [f32; 3],
    },
    CameraPositionMismatch {
        view_position: [f32; 3],
    },
}

impl Display for FrameViewValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroViewport { viewport } => write!(
                formatter,
                "frame viewport must be non-zero, got {}x{}",
                viewport[0], viewport[1]
            ),
            Self::InvalidClipPlanes {
                near_clip,
                far_clip,
            } => write!(
                formatter,
                "clip planes must be finite and satisfy 0 < near < far, got {near_clip}..{far_clip}"
            ),
            Self::UnsupportedSampleCount { sample_count } => write!(
                formatter,
                "render IR currently requires one sample, got {sample_count}"
            ),
            Self::NonAffineWorldToView { homogeneous_row } => write!(
                formatter,
                "world-to-view matrix must be affine with homogeneous row [0, 0, 0, 1], got {homogeneous_row:?}"
            ),
            Self::NonRigidWorldToView {
                determinant,
                max_orthonormal_error,
            } => write!(
                formatter,
                "world-to-view basis must be a proper rigid rotation, got determinant {determinant} and orthonormal error {max_orthonormal_error}"
            ),
            Self::InvalidReverseZProjection {
                near_depth,
                midpoint_depth,
                far_depth,
            } => write!(
                formatter,
                "view-to-clip matrix must map near/mid/far to decreasing Vulkan reverse-Z depths 1..0, got {near_depth:?}/{midpoint_depth:?}/{far_depth:?}"
            ),
            Self::NonFiniteMatrix {
                matrix,
                column,
                row,
            } => write!(
                formatter,
                "{matrix:?} matrix component [{column}][{row}] is not finite"
            ),
            Self::NonFiniteCameraPosition { axis } => {
                write!(formatter, "camera position component {axis} is not finite")
            }
            Self::NonFinitePreciseCameraPosition { axis } => write!(
                formatter,
                "precise camera position component {axis} is not finite"
            ),
            Self::CameraBlockPositionOutOfRange { axis, coordinate } => write!(
                formatter,
                "camera position component {axis}={coordinate} has no i32 block coordinate"
            ),
            Self::ViewRotationHasTranslation { translation } => write!(
                formatter,
                "camera-relative view rotation must have zero translation, got {translation:?}"
            ),
            Self::CameraPositionMismatch { view_position } => write!(
                formatter,
                "camera position must map to view-space origin, got [{}, {}, {}]",
                view_position[0], view_position[1], view_position[2]
            ),
        }
    }
}

impl Error for FrameViewValidationError {}

#[cfg(test)]
mod tests {
    use super::{multiply_column_major, transform_point};

    #[test]
    fn column_major_multiplication_composes_right_then_left() {
        let translation = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [2.0, 3.0, 4.0, 1.0],
        ];
        let scale = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        let composed = multiply_column_major(scale, translation);
        assert_eq!(
            transform_point(composed, [1.0, 1.0, 1.0]),
            [6.0, 12.0, 20.0, 1.0]
        );
    }
}
