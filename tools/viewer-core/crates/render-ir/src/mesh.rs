use std::cmp::Ordering;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::{MaterialId, TerrainRenderLayer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionId(u32);

impl SectionId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Dense primitive rank within one immutable scene snapshot.
///
/// The rank follows canonical section, layer, material, and triangle order, so
/// equivalent geometry receives the same PPLL tie ordering even when worker or
/// draw construction order differs. It is not a persistent identity across
/// snapshots with different scene contents.
pub struct StablePrimitiveId(u64);

impl StablePrimitiveId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionPosition {
    x: i32,
    y: i32,
    z: i32,
}

impl SectionPosition {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn x(self) -> i32 {
        self.x
    }

    pub const fn y(self) -> i32 {
        self.y
    }

    pub const fn z(self) -> i32 {
        self.z
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    min: [f32; 3],
    max: [f32; 3],
}

impl Aabb {
    pub fn new(min: [f32; 3], max: [f32; 3]) -> Result<Self, MeshValidationError> {
        for axis in 0..3 {
            if !min[axis].is_finite() {
                return Err(MeshValidationError::NonFiniteAabbCoordinate {
                    bound: AabbBound::Min,
                    axis,
                });
            }
            if !max[axis].is_finite() {
                return Err(MeshValidationError::NonFiniteAabbCoordinate {
                    bound: AabbBound::Max,
                    axis,
                });
            }
            if min[axis] > max[axis] {
                return Err(MeshValidationError::InvalidAabbOrder { axis });
            }
        }
        Ok(Self { min, max })
    }

    pub const fn min(self) -> [f32; 3] {
        self.min
    }

    pub const fn max(self) -> [f32; 3] {
        self.max
    }

    fn contains(self, position: [f32; 3]) -> Result<(), (usize, f32)> {
        for (axis, coordinate) in position.into_iter().enumerate() {
            if coordinate < self.min[axis] || coordinate > self.max[axis] {
                return Err((axis, coordinate));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AabbBound {
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaderVertex {
    position: [f32; 3],
    color: VertexColor,
    atlas_uv: [f32; 2],
    light_coordinates: LightCoordinates,
    normal: [f32; 3],
    material_id: MaterialId,
    iris_terrain: Option<IrisTerrainVertexAttributes>,
}

/// Shader-pack-independent terrain semantics required to lower one vertex to
/// Iris' extended terrain layout.
///
/// The final `mc_Entity.x` value is intentionally absent: it is selected by
/// the active shader pack's `block.properties`. `catalog_state_index` is the
/// stable MC 26.2 lookup key, while `runtime_state_index` retains the dense
/// index for consumers sharing the current catalog runtime. Keeping both lets
/// cache-backed lowering map before entering a graphics backend without
/// truncating either identity to `i16`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IrisTerrainVertexAttributes {
    catalog_state_index: u32,
    runtime_state_index: u32,
    render_type: IrisTerrainRenderType,
    color: IrisTerrainColor,
    face_normal: [f32; 3],
    mid_texture_coordinate: [f32; 2],
    tangent_snorm8: [u8; 4],
    mid_block_snorm8: [u8; 4],
}

impl IrisTerrainVertexAttributes {
    pub fn new(
        catalog_state_index: u32,
        runtime_state_index: u32,
        render_type: IrisTerrainRenderType,
        color: IrisTerrainColor,
        face_normal: [f32; 3],
        mid_texture_coordinate: [f32; 2],
        tangent_snorm8: [u8; 4],
        mid_block_snorm8: [u8; 4],
    ) -> Result<Self, MeshValidationError> {
        validate_finite(VertexAttribute::IrisFaceNormal, &face_normal)?;
        validate_finite(
            VertexAttribute::IrisMidTextureCoordinate,
            &mid_texture_coordinate,
        )?;
        if mid_block_snorm8[3] > 15 {
            return Err(MeshValidationError::InvalidIrisBlockEmission {
                emission: mid_block_snorm8[3],
            });
        }
        Ok(Self {
            catalog_state_index,
            runtime_state_index,
            render_type,
            color,
            face_normal,
            mid_texture_coordinate,
            tangent_snorm8,
            mid_block_snorm8,
        })
    }

    pub const fn catalog_state_index(self) -> u32 {
        self.catalog_state_index
    }

    pub const fn runtime_state_index(self) -> u32 {
        self.runtime_state_index
    }

    pub const fn render_type(self) -> IrisTerrainRenderType {
        self.render_type
    }

    pub const fn color(self) -> IrisTerrainColor {
        self.color
    }

    /// Combines a shader-pack-mapped block ID with the already captured
    /// ShadersMod terrain render type for the legacy `mc_Entity` attribute.
    pub const fn mc_entity(self, mapped_block_id: i16) -> [i16; 2] {
        [mapped_block_id, self.render_type.shadersmod_id()]
    }

    pub const fn face_normal(self) -> [f32; 3] {
        self.face_normal
    }

    pub const fn mid_texture_coordinate(self) -> [f32; 2] {
        self.mid_texture_coordinate
    }

    pub const fn tangent_snorm8(self) -> [u8; 4] {
        self.tangent_snorm8
    }

    pub const fn mid_block_snorm8(self) -> [u8; 4] {
        self.mid_block_snorm8
    }

    pub const fn block_emission(self) -> u8 {
        self.mid_block_snorm8[3]
    }
}

/// Shader-pack-independent Sodium terrain color inputs retained until upload.
///
/// Locked Iris writes the raw ABGR tint to `ChunkVertexEncoder.Vertex.color`
/// and keeps lighting in `Vertex.ao`. `oldLighting=false` redirects every
/// directional `CardinalLighting.byFace` lookup to `Direction.UP`, while
/// `oldLighting=true` preserves the original directional result. Both values
/// are quantized exactly once using Sodium's normalized-float-to-byte rule so
/// the GL backend never has to infer either coefficient from final RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IrisTerrainColor {
    tint: VertexColor,
    ao: u8,
    old_lighting_ao: u8,
}

impl IrisTerrainColor {
    pub const fn new(tint: VertexColor, ao: u8, old_lighting_ao: u8) -> Self {
        Self {
            tint,
            ao,
            old_lighting_ao,
        }
    }

    pub const fn tint(self) -> VertexColor {
        self.tint
    }

    /// Coefficient produced with Iris' `Direction.UP` redirect.
    pub const fn ao(self) -> u8 {
        self.ao
    }

    /// Coefficient produced by Sodium's original directional-lighting path.
    pub const fn old_lighting_ao(self) -> u8 {
        self.old_lighting_ao
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IrisTerrainRenderType {
    Block,
    Fluid,
}

impl IrisTerrainRenderType {
    pub const fn shadersmod_id(self) -> i16 {
        match self {
            // Locked Iris' Sodium path calls `beginBlock(..., (byte) 0, ...)`.
            // `SodiumTransformer.replaceMCEntity` forwards that render type
            // unchanged as `mc_Entity.y`; only fluid vertices use 1.
            Self::Block => 0,
            Self::Fluid => 1,
        }
    }
}

/// Exact MC 26.2 `Color` payload consumed by the terrain vertex shader.
///
/// `DefaultVertexFormat.BLOCK` stores the four components as `RGBA8_UNORM`.
/// Keeping the bytes in the IR avoids a float round-trip before GPU upload and
/// preserves colors emitted by native model and resource-pack adapters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VertexColor([u8; 4]);

impl VertexColor {
    pub const WHITE: Self = Self::new(255, 255, 255, 255);

    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self([red, green, blue, alpha])
    }

    pub const fn rgba8(self) -> [u8; 4] {
        self.0
    }
}

/// Exact MC 26.2 `UV2` payload consumed by the terrain lightmap shader.
///
/// `DefaultVertexFormat.BLOCK` stores this attribute as `RG16_SINT`. Minecraft
/// writes the low and high 16-bit halves of the packed light integer directly;
/// smooth lighting may therefore contain values between the usual multiples
/// of 16. Keeping the signed components intact also preserves the behavior of
/// custom native producers instead of normalizing or re-packing their input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LightCoordinates([i16; 2]);

impl LightCoordinates {
    pub const FULL_BRIGHT: Self = Self::from_packed(15_728_880);

    pub const fn new(block: i16, sky: i16) -> Self {
        Self([block, sky])
    }

    pub const fn from_packed(packed: i32) -> Self {
        Self([packed as i16, (packed >> 16) as i16])
    }

    pub const fn packed(self) -> i32 {
        let block = self.0[0] as u16 as u32;
        let sky = (self.0[1] as u16 as u32) << 16;
        (block | sky) as i32
    }

    pub const fn vertex_components(self) -> [i16; 2] {
        self.0
    }

    pub const fn block(self) -> i16 {
        self.0[0]
    }

    pub const fn sky(self) -> i16 {
        self.0[1]
    }
}

impl ShaderVertex {
    pub fn new(
        position: [f32; 3],
        color: VertexColor,
        atlas_uv: [f32; 2],
        light_coordinates: LightCoordinates,
        normal: [f32; 3],
        material_id: MaterialId,
    ) -> Result<Self, MeshValidationError> {
        validate_finite(VertexAttribute::Position, &position)?;
        validate_finite(VertexAttribute::AtlasUv, &atlas_uv)?;
        validate_finite(VertexAttribute::Normal, &normal)?;
        Ok(Self {
            position,
            color,
            atlas_uv,
            light_coordinates,
            normal,
            material_id,
            iris_terrain: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_iris_terrain(
        position: [f32; 3],
        color: VertexColor,
        atlas_uv: [f32; 2],
        light_coordinates: LightCoordinates,
        normal: [f32; 3],
        material_id: MaterialId,
        iris_terrain: IrisTerrainVertexAttributes,
    ) -> Result<Self, MeshValidationError> {
        let mut vertex = Self::new(
            position,
            color,
            atlas_uv,
            light_coordinates,
            normal,
            material_id,
        )?;
        vertex.iris_terrain = Some(iris_terrain);
        Ok(vertex)
    }

    pub const fn position(self) -> [f32; 3] {
        self.position
    }

    pub const fn color(self) -> VertexColor {
        self.color
    }

    pub const fn atlas_uv(self) -> [f32; 2] {
        self.atlas_uv
    }

    pub const fn light_coordinates(self) -> LightCoordinates {
        self.light_coordinates
    }

    pub const fn normal(self) -> [f32; 3] {
        self.normal
    }

    pub const fn material_id(self) -> MaterialId {
        self.material_id
    }

    pub const fn iris_terrain(self) -> Option<IrisTerrainVertexAttributes> {
        self.iris_terrain
    }
}

fn validate_finite<const N: usize>(
    attribute: VertexAttribute,
    values: &[f32; N],
) -> Result<(), MeshValidationError> {
    for (component, value) in values.iter().enumerate() {
        if !value.is_finite() {
            return Err(MeshValidationError::NonFiniteVertexAttribute {
                attribute,
                component,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexAttribute {
    Position,
    Color,
    AtlasUv,
    Normal,
    IrisFaceNormal,
    IrisMidTextureCoordinate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawRange {
    first_index: u32,
    index_count: u32,
    material_id: MaterialId,
    first_primitive_id: StablePrimitiveId,
}

impl DrawRange {
    pub const fn new(first_index: u32, index_count: u32, material_id: MaterialId) -> Self {
        Self {
            first_index,
            index_count,
            material_id,
            first_primitive_id: StablePrimitiveId::new(0),
        }
    }

    pub const fn first_index(self) -> u32 {
        self.first_index
    }

    pub const fn index_count(self) -> u32 {
        self.index_count
    }

    pub const fn material_id(self) -> MaterialId {
        self.material_id
    }

    pub const fn first_primitive_id(self) -> StablePrimitiveId {
        self.first_primitive_id
    }

    pub const fn primitive_count(self) -> u64 {
        (self.index_count / 3) as u64
    }

    fn set_first_primitive_id(&mut self, first_primitive_id: StablePrimitiveId) {
        self.first_primitive_id = first_primitive_id;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerMesh {
    indices: Box<[u32]>,
    draws: Box<[DrawRange]>,
}

impl LayerMesh {
    pub fn new(indices: Box<[u32]>, draws: Box<[DrawRange]>) -> Result<Self, MeshValidationError> {
        let index_count =
            u32::try_from(indices.len()).map_err(|_| MeshValidationError::IndexBufferTooLarge {
                index_count: indices.len(),
            })?;
        let mut expected_first_index = 0_u32;
        for (draw_index, draw) in draws.iter().enumerate() {
            if draw.first_index != expected_first_index {
                return Err(MeshValidationError::DrawRangePartition {
                    draw_index,
                    expected_first_index,
                    actual_first_index: draw.first_index,
                });
            }
            if draw.index_count == 0 {
                return Err(MeshValidationError::EmptyDrawRange { draw_index });
            }
            if draw.index_count % 3 != 0 {
                return Err(MeshValidationError::IndexCountNotTriangleAligned {
                    draw_index,
                    index_count: draw.index_count,
                });
            }
            let end = draw.first_index.checked_add(draw.index_count).ok_or(
                MeshValidationError::DrawRangeOverflow {
                    draw_index,
                    first_index: draw.first_index,
                    index_count: draw.index_count,
                },
            )?;
            if end > index_count {
                return Err(MeshValidationError::DrawRangeOutOfBounds {
                    draw_index,
                    range_end: end,
                    index_count,
                });
            }
            expected_first_index = end;
        }
        if expected_first_index != index_count {
            return Err(MeshValidationError::UncoveredIndices {
                covered_index_count: expected_first_index,
                index_count,
            });
        }
        Ok(Self { indices, draws })
    }

    pub fn empty() -> Self {
        Self {
            indices: Box::new([]),
            draws: Box::new([]),
        }
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub fn draws(&self) -> &[DrawRange] {
        &self.draws
    }

    pub fn primitive_count(&self) -> u64 {
        self.draws.iter().map(|draw| draw.primitive_count()).sum()
    }

    fn checked_primitive_count(&self) -> Option<u64> {
        self.draws.iter().try_fold(0_u64, |total, draw| {
            total.checked_add(draw.primitive_count())
        })
    }

    fn canonicalize(&mut self, vertices: &[ShaderVertex]) {
        let mut triangles = Vec::with_capacity(self.indices.len() / 3);
        for draw in &self.draws {
            let start = draw.first_index as usize;
            let end = start + draw.index_count as usize;
            for triangle in self.indices[start..end].chunks_exact(3) {
                triangles.push((
                    draw.material_id,
                    canonical_triangle(vertices, [triangle[0], triangle[1], triangle[2]]),
                ));
            }
        }
        triangles.sort_by(|(left_material, left), (right_material, right)| {
            left_material
                .cmp(right_material)
                .then_with(|| compare_triangles(vertices, *left, *right))
        });

        let mut indices = Vec::with_capacity(self.indices.len());
        let mut draws = Vec::<DrawRange>::with_capacity(self.draws.len());
        for (material_id, triangle) in triangles {
            if draws.last().map(|draw| draw.material_id()) != Some(material_id) {
                draws.push(DrawRange::new(indices.len() as u32, 0, material_id));
            }
            indices.extend_from_slice(&triangle);
            draws.last_mut().expect("draw was just created").index_count += 3;
        }
        self.indices = indices.into_boxed_slice();
        self.draws = draws.into_boxed_slice();
    }

    fn remap_materials(&mut self, vertices: &[ShaderVertex], remap: &[MaterialId]) {
        for draw in &mut self.draws {
            draw.material_id = remap[draw.material_id.value() as usize];
        }
        self.canonicalize(vertices);
    }

    fn rewrite_material_ids_preserving_triangle_order_with<F>(&mut self, remap: &F)
    where
        F: Fn(MaterialId) -> MaterialId,
    {
        for draw in &mut self.draws {
            draw.material_id = remap(draw.material_id);
        }
    }

    fn assign_stable_primitive_ids(&mut self, next: &mut u64) -> Result<(), ()> {
        for draw in &mut self.draws {
            draw.set_first_primitive_id(StablePrimitiveId::new(*next));
            *next = next.checked_add(draw.primitive_count()).ok_or(())?;
        }
        Ok(())
    }
}

impl Default for LayerMesh {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SectionMesh {
    id: SectionId,
    position: SectionPosition,
    world_origin: [i32; 3],
    bounds: Aabb,
    vertices: Box<[ShaderVertex]>,
    opaque: LayerMesh,
    cutout: LayerMesh,
    translucent: LayerMesh,
}

impl SectionMesh {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        position: SectionPosition,
        world_origin: [i32; 3],
        bounds: Aabb,
        vertices: Box<[ShaderVertex]>,
        opaque: LayerMesh,
        cutout: LayerMesh,
        translucent: LayerMesh,
    ) -> Result<Self, MeshValidationError> {
        let mut section = Self::new_preserving_triangle_order(
            position,
            world_origin,
            bounds,
            vertices,
            opaque,
            cutout,
            translucent,
        )?;
        section.canonicalize_triangle_order();
        Ok(section)
    }

    /// Validates a section without reordering its already partitioned triangles.
    ///
    /// This preserves every validation performed by [`Self::new`]. It is
    /// intended for trusted intermediate records that will undergo a later
    /// canonical material remap and triangle sort before becoming observable.
    #[allow(clippy::too_many_arguments)]
    pub fn new_preserving_triangle_order(
        position: SectionPosition,
        world_origin: [i32; 3],
        bounds: Aabb,
        vertices: Box<[ShaderVertex]>,
        opaque: LayerMesh,
        cutout: LayerMesh,
        translucent: LayerMesh,
    ) -> Result<Self, MeshValidationError> {
        for (vertex_index, vertex) in vertices.iter().enumerate() {
            if let Err((axis, coordinate)) = bounds.contains(vertex.position()) {
                return Err(MeshValidationError::VertexOutsideBounds {
                    vertex_index,
                    axis,
                    coordinate,
                    minimum: bounds.min()[axis],
                    maximum: bounds.max()[axis],
                });
            }
        }
        validate_layer_vertices(TerrainRenderLayer::Opaque, &vertices, &opaque)?;
        validate_layer_vertices(TerrainRenderLayer::Cutout, &vertices, &cutout)?;
        validate_layer_vertices(TerrainRenderLayer::Translucent, &vertices, &translucent)?;
        Ok(Self {
            id: SectionId::new(0),
            position,
            world_origin,
            bounds,
            vertices,
            opaque,
            cutout,
            translucent,
        })
    }

    pub(crate) fn canonicalize_triangle_order(&mut self) {
        self.opaque.canonicalize(&self.vertices);
        self.cutout.canonicalize(&self.vertices);
        self.translucent.canonicalize(&self.vertices);
    }

    pub const fn id(&self) -> SectionId {
        self.id
    }

    pub const fn position(&self) -> SectionPosition {
        self.position
    }

    pub const fn world_origin(&self) -> [i32; 3] {
        self.world_origin
    }

    pub const fn bounds(&self) -> Aabb {
        self.bounds
    }

    pub fn vertices(&self) -> &[ShaderVertex] {
        &self.vertices
    }

    pub const fn opaque(&self) -> &LayerMesh {
        &self.opaque
    }

    pub const fn cutout(&self) -> &LayerMesh {
        &self.cutout
    }

    pub const fn translucent(&self) -> &LayerMesh {
        &self.translucent
    }

    /// Partitions existing triangles into presentation-specific draw layers.
    ///
    /// The deterministic callback selects a layer from each triangle's first
    /// vertex. Callers must first validate any attributes they require to be
    /// uniform across that triangle. Vertices, triangle index order, material
    /// IDs, and stable primitive IDs are preserved; no geometry is regenerated.
    /// Destination order follows the original opaque/cutout/translucent scan.
    /// An unchanged section retains all allocations. This does not rewrite a
    /// shared scene's material table and is intended for backend-local prepare.
    pub fn reroute_triangle_layers<F>(&mut self, route: F) -> Result<usize, MeshValidationError>
    where
        F: Fn(TerrainRenderLayer, &ShaderVertex) -> TerrainRenderLayer,
    {
        let layer_slot = |layer| match layer {
            TerrainRenderLayer::Opaque => 0,
            TerrainRenderLayer::Cutout => 1,
            TerrainRenderLayer::Translucent => 2,
        };
        let mut index_counts = [0_usize; 3];
        let mut moved_triangles = 0;
        for (source_layer, mesh) in self.layers() {
            for triangle in mesh.indices.chunks_exact(3) {
                let target_layer = route(source_layer, &self.vertices[triangle[0] as usize]);
                let count = &mut index_counts[layer_slot(target_layer)];
                *count = count
                    .checked_add(3)
                    .filter(|&value| value <= u32::MAX as usize)
                    .ok_or(MeshValidationError::IndexBufferTooLarge {
                        index_count: count.saturating_add(3),
                    })?;
                moved_triangles += usize::from(target_layer != source_layer);
            }
        }
        if moved_triangles == 0 {
            return Ok(0);
        }

        let mut indices = index_counts.map(Vec::<u32>::with_capacity);
        let mut draws: [Vec<DrawRange>; 3] = std::array::from_fn(|_| Vec::new());
        for (source_layer, mesh) in self.layers() {
            for draw in mesh.draws.iter().copied() {
                let start = draw.first_index as usize;
                let end = start + draw.index_count as usize;
                for (offset, triangle) in mesh.indices[start..end].chunks_exact(3).enumerate() {
                    let target =
                        layer_slot(route(source_layer, &self.vertices[triangle[0] as usize]));
                    let target_indices = &mut indices[target];
                    let target_draws = &mut draws[target];
                    let first_index = u32::try_from(target_indices.len()).map_err(|_| {
                        MeshValidationError::IndexBufferTooLarge {
                            index_count: target_indices.len(),
                        }
                    })?;
                    let primitive_id = draw.first_primitive_id.value() + offset as u64;
                    let continues_draw = target_draws.last().is_some_and(|last| {
                        last.material_id == draw.material_id
                            && last
                                .first_primitive_id
                                .value()
                                .checked_add(last.primitive_count())
                                == Some(primitive_id)
                    });
                    if continues_draw {
                        target_draws.last_mut().expect("draw exists").index_count += 3;
                    } else {
                        target_draws.push(DrawRange {
                            first_index,
                            index_count: 3,
                            material_id: draw.material_id,
                            first_primitive_id: StablePrimitiveId::new(primitive_id),
                        });
                    }
                    target_indices.extend_from_slice(triangle);
                }
            }
        }
        let [opaque_indices, cutout_indices, translucent_indices] = indices;
        let [opaque_draws, cutout_draws, translucent_draws] = draws;
        let opaque = LayerMesh::new(
            opaque_indices.into_boxed_slice(),
            opaque_draws.into_boxed_slice(),
        )?;
        let cutout = LayerMesh::new(
            cutout_indices.into_boxed_slice(),
            cutout_draws.into_boxed_slice(),
        )?;
        let translucent = LayerMesh::new(
            translucent_indices.into_boxed_slice(),
            translucent_draws.into_boxed_slice(),
        )?;
        self.opaque = opaque;
        self.cutout = cutout;
        self.translucent = translucent;
        Ok(moved_triangles)
    }

    pub fn primitive_count(&self) -> u64 {
        self.opaque.primitive_count()
            + self.cutout.primitive_count()
            + self.translucent.primitive_count()
    }

    pub(crate) fn layers(&self) -> [(TerrainRenderLayer, &LayerMesh); 3] {
        [
            (TerrainRenderLayer::Opaque, &self.opaque),
            (TerrainRenderLayer::Cutout, &self.cutout),
            (TerrainRenderLayer::Translucent, &self.translucent),
        ]
    }

    pub(crate) fn checked_primitive_count(&self) -> Option<u64> {
        self.opaque
            .checked_primitive_count()?
            .checked_add(self.cutout.checked_primitive_count()?)?
            .checked_add(self.translucent.checked_primitive_count()?)
    }

    pub(crate) fn set_id(&mut self, id: SectionId) {
        self.id = id;
    }

    /// Rewrites material IDs without sorting or rebuilding layer geometry.
    ///
    /// Callers must provide an entry for every material ID referenced by this
    /// mesh. Vertex order, index order, draw ranges, and triangle order remain
    /// byte-for-byte unchanged. This narrow path exists for an intermediate
    /// local-to-request ID rewrite. `SectionMesh::new` already performed the
    /// initial section-local canonicalization; a later `SceneSnapshot`
    /// construction performs the final global-material canonicalization.
    pub fn rewrite_material_ids_preserving_triangle_order(&mut self, remap: &[MaterialId]) {
        self.rewrite_material_ids_preserving_triangle_order_with(|material_id| {
            remap[material_id.value() as usize]
        });
    }

    pub fn rewrite_material_ids_preserving_triangle_order_with<F>(&mut self, remap: F)
    where
        F: Fn(MaterialId) -> MaterialId,
    {
        for vertex in &mut self.vertices {
            vertex.material_id = remap(vertex.material_id);
        }
        self.opaque
            .rewrite_material_ids_preserving_triangle_order_with(&remap);
        self.cutout
            .rewrite_material_ids_preserving_triangle_order_with(&remap);
        self.translucent
            .rewrite_material_ids_preserving_triangle_order_with(&remap);
    }

    pub(crate) fn remap_materials(&mut self, remap: &[MaterialId]) {
        for vertex in &mut self.vertices {
            vertex.material_id = remap[vertex.material_id.value() as usize];
        }
        self.opaque.remap_materials(&self.vertices, remap);
        self.cutout.remap_materials(&self.vertices, remap);
        self.translucent.remap_materials(&self.vertices, remap);
    }

    pub(crate) fn assign_stable_primitive_ids(&mut self, next: &mut u64) -> Result<(), ()> {
        self.opaque.assign_stable_primitive_ids(next)?;
        self.cutout.assign_stable_primitive_ids(next)?;
        self.translucent.assign_stable_primitive_ids(next)
    }
}

fn canonical_triangle(vertices: &[ShaderVertex], triangle: [u32; 3]) -> [u32; 3] {
    let rotations = [
        triangle,
        [triangle[1], triangle[2], triangle[0]],
        [triangle[2], triangle[0], triangle[1]],
    ];
    rotations
        .into_iter()
        .min_by(|left, right| compare_triangles(vertices, *left, *right))
        .expect("a triangle always has three cyclic rotations")
}

fn compare_triangles(vertices: &[ShaderVertex], left: [u32; 3], right: [u32; 3]) -> Ordering {
    left.into_iter()
        .zip(right)
        .map(|(left_index, right_index)| {
            compare_vertices(
                &vertices[left_index as usize],
                &vertices[right_index as usize],
            )
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

fn compare_vertices(left: &ShaderVertex, right: &ShaderVertex) -> Ordering {
    compare_float_arrays(&left.position, &right.position)
        .then_with(|| left.color.cmp(&right.color))
        .then_with(|| compare_float_arrays(&left.atlas_uv, &right.atlas_uv))
        .then_with(|| left.light_coordinates.cmp(&right.light_coordinates))
        .then_with(|| compare_float_arrays(&left.normal, &right.normal))
        .then_with(|| left.material_id.cmp(&right.material_id))
        .then_with(|| compare_iris_terrain(left.iris_terrain, right.iris_terrain))
}

fn compare_iris_terrain(
    left: Option<IrisTerrainVertexAttributes>,
    right: Option<IrisTerrainVertexAttributes>,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => left
            .catalog_state_index
            .cmp(&right.catalog_state_index)
            .then_with(|| left.runtime_state_index.cmp(&right.runtime_state_index))
            .then_with(|| left.render_type.cmp(&right.render_type))
            .then_with(|| compare_float_arrays(&left.face_normal, &right.face_normal))
            .then_with(|| {
                compare_float_arrays(&left.mid_texture_coordinate, &right.mid_texture_coordinate)
            })
            .then_with(|| left.tangent_snorm8.cmp(&right.tangent_snorm8))
            .then_with(|| left.mid_block_snorm8.cmp(&right.mid_block_snorm8)),
    }
}

fn compare_float_arrays<const N: usize>(left: &[f32; N], right: &[f32; N]) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(left, right)| left.total_cmp(right))
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

fn validate_layer_vertices(
    layer: TerrainRenderLayer,
    vertices: &[ShaderVertex],
    mesh: &LayerMesh,
) -> Result<(), MeshValidationError> {
    for (draw_index, draw) in mesh.draws.iter().enumerate() {
        let start = draw.first_index as usize;
        let end = start + draw.index_count as usize;
        for (index_offset, vertex_index) in mesh.indices[start..end].iter().copied().enumerate() {
            let vertex = vertices.get(vertex_index as usize).ok_or(
                MeshValidationError::VertexIndexOutOfBounds {
                    layer,
                    draw_index,
                    index_offset,
                    vertex_index,
                    vertex_count: vertices.len(),
                },
            )?;
            if vertex.material_id != draw.material_id {
                return Err(MeshValidationError::VertexMaterialMismatch {
                    layer,
                    draw_index,
                    index_offset,
                    vertex_index,
                    expected_material: draw.material_id,
                    actual_material: vertex.material_id,
                });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum MeshValidationError {
    NonFiniteAabbCoordinate {
        bound: AabbBound,
        axis: usize,
    },
    InvalidAabbOrder {
        axis: usize,
    },
    NonFiniteVertexAttribute {
        attribute: VertexAttribute,
        component: usize,
    },
    InvalidIrisBlockEmission {
        emission: u8,
    },
    VertexOutsideBounds {
        vertex_index: usize,
        axis: usize,
        coordinate: f32,
        minimum: f32,
        maximum: f32,
    },
    IndexBufferTooLarge {
        index_count: usize,
    },
    EmptyDrawRange {
        draw_index: usize,
    },
    IndexCountNotTriangleAligned {
        draw_index: usize,
        index_count: u32,
    },
    DrawRangePartition {
        draw_index: usize,
        expected_first_index: u32,
        actual_first_index: u32,
    },
    DrawRangeOverflow {
        draw_index: usize,
        first_index: u32,
        index_count: u32,
    },
    DrawRangeOutOfBounds {
        draw_index: usize,
        range_end: u32,
        index_count: u32,
    },
    UncoveredIndices {
        covered_index_count: u32,
        index_count: u32,
    },
    VertexIndexOutOfBounds {
        layer: TerrainRenderLayer,
        draw_index: usize,
        index_offset: usize,
        vertex_index: u32,
        vertex_count: usize,
    },
    VertexMaterialMismatch {
        layer: TerrainRenderLayer,
        draw_index: usize,
        index_offset: usize,
        vertex_index: u32,
        expected_material: MaterialId,
        actual_material: MaterialId,
    },
}

impl Display for MeshValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteAabbCoordinate { bound, axis } => {
                write!(formatter, "AABB {bound:?} component {axis} is not finite")
            }
            Self::InvalidAabbOrder { axis } => {
                write!(formatter, "AABB minimum exceeds maximum on axis {axis}")
            }
            Self::NonFiniteVertexAttribute {
                attribute,
                component,
            } => write!(
                formatter,
                "vertex {attribute:?} component {component} is not finite"
            ),
            Self::InvalidIrisBlockEmission { emission } => write!(
                formatter,
                "Iris terrain block emission {emission} exceeds the MC light level maximum 15"
            ),
            Self::VertexOutsideBounds {
                vertex_index,
                axis,
                coordinate,
                minimum,
                maximum,
            } => write!(
                formatter,
                "vertex {vertex_index} component {axis}={coordinate} lies outside section bounds {minimum}..={maximum}"
            ),
            Self::IndexBufferTooLarge { index_count } => write!(
                formatter,
                "index buffer contains {index_count} entries and cannot use u32 draw ranges"
            ),
            Self::EmptyDrawRange { draw_index } => {
                write!(
                    formatter,
                    "draw {draw_index} must contain at least one triangle"
                )
            }
            Self::IndexCountNotTriangleAligned {
                draw_index,
                index_count,
            } => write!(
                formatter,
                "draw {draw_index} index count {index_count} is not divisible by three"
            ),
            Self::DrawRangePartition {
                draw_index,
                expected_first_index,
                actual_first_index,
            } => write!(
                formatter,
                "draw {draw_index} must start at index {expected_first_index}, got {actual_first_index}"
            ),
            Self::DrawRangeOverflow {
                draw_index,
                first_index,
                index_count,
            } => write!(
                formatter,
                "draw {draw_index} range {first_index}+{index_count} overflows u32"
            ),
            Self::DrawRangeOutOfBounds {
                draw_index,
                range_end,
                index_count,
            } => write!(
                formatter,
                "draw {draw_index} ends at {range_end}, beyond index buffer length {index_count}"
            ),
            Self::UncoveredIndices {
                covered_index_count,
                index_count,
            } => write!(
                formatter,
                "draw ranges cover {covered_index_count} of {index_count} indices"
            ),
            Self::VertexIndexOutOfBounds {
                layer,
                draw_index,
                index_offset,
                vertex_index,
                vertex_count,
            } => write!(
                formatter,
                "{layer:?} draw {draw_index} index {index_offset} references vertex {vertex_index}, but only {vertex_count} vertices exist"
            ),
            Self::VertexMaterialMismatch {
                layer,
                draw_index,
                index_offset,
                vertex_index,
                expected_material,
                actual_material,
            } => write!(
                formatter,
                "{layer:?} draw {draw_index} index {index_offset} references vertex {vertex_index} with material {}, expected {}",
                actual_material.value(),
                expected_material.value()
            ),
        }
    }
}

impl Error for MeshValidationError {}
