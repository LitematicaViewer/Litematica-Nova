use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::{
    FaceMode, MaterialDesc, MaterialId, MinecraftSemanticVersion, ResourceGeneration, SectionId,
    SectionMesh, SectionPosition, TerrainRenderLayer, TextureDesc, TextureId, TextureSemantic,
};

/// Immutable, backend-independent render input for one resource generation.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSnapshot {
    minecraft_semantics: MinecraftSemanticVersion,
    generation: ResourceGeneration,
    textures: Box<[TextureDesc]>,
    materials: Box<[MaterialDesc]>,
    sections: Box<[SectionMesh]>,
    primitive_count: u64,
}

/// Canonicalizes provisional scene resource IDs without retaining scene geometry.
///
/// The input tables use the registration-order IDs referenced by section
/// vertices and draws. The canonical tables are sorted and deduplicated exactly
/// as [`SceneSnapshot::new`] requires, while [`Self::validate_section`] keeps
/// validating against the provisional material table.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneResourceCanonicalizer {
    generation: ResourceGeneration,
    textures: Box<[TextureDesc]>,
    materials: Box<[MaterialDesc]>,
    provisional_material_layers: Box<[TerrainRenderLayer]>,
    provisional_material_face_modes: Box<[FaceMode]>,
    material_remap: Box<[MaterialId]>,
}

impl SceneResourceCanonicalizer {
    pub fn new(
        generation: ResourceGeneration,
        textures: Box<[TextureDesc]>,
        mut materials: Box<[MaterialDesc]>,
    ) -> Result<Self, SceneValidationError> {
        validate_textures(generation, &textures)?;
        validate_material_textures(&textures, &materials)?;

        let provisional_material_layers = materials
            .iter()
            .map(MaterialDesc::layer)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let provisional_material_face_modes = materials
            .iter()
            .map(MaterialDesc::face_mode)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let (textures, texture_remap) = canonicalize_table(textures);
        for material in &mut materials {
            material.remap_base_color_texture(TextureId::new(
                texture_remap[material.base_color_texture().value() as usize],
            ));
        }

        let (materials, material_remap) = canonicalize_table(materials);
        let material_remap = material_remap
            .into_iter()
            .map(MaterialId::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(Self {
            generation,
            textures,
            materials,
            provisional_material_layers,
            provisional_material_face_modes,
            material_remap,
        })
    }

    /// Validates one section against the provisional registration-order table.
    pub fn validate_section(&self, section: &SectionMesh) -> Result<(), SceneValidationError> {
        validate_section_materials(section, &self.provisional_material_layers)
    }

    /// Validates a section whose first triangle canonicalization was deferred.
    ///
    /// With an injective material remap, valid sections remain untouched. A
    /// non-injective remap still performs the required provisional ordering.
    /// If validation fails, the provisional triangle order is canonicalized
    /// before the error is recomputed so the reported first error remains
    /// identical to [`Self::validate_section`] after construction with
    /// [`SectionMesh::new`].
    pub fn validate_deferred_section(
        &self,
        section: &mut SectionMesh,
    ) -> Result<(), SceneValidationError> {
        if !self.material_remap_is_injective() {
            section.canonicalize_triangle_order();
            return validate_section_materials(section, &self.provisional_material_layers);
        }
        match validate_section_materials(section, &self.provisional_material_layers) {
            Ok(()) => Ok(()),
            Err(_) => {
                section.canonicalize_triangle_order();
                validate_section_materials(section, &self.provisional_material_layers)
            }
        }
    }

    /// Remaps one previously validated section to the canonical material table.
    ///
    /// This also restores the canonical material and triangle order after the
    /// IDs change. Call [`Self::validate_section`] for this section first.
    ///
    /// # Panics
    ///
    /// Panics when the section contains a provisional material ID outside the
    /// table passed to [`Self::new`].
    pub fn canonicalize_validated_section(&self, section: &mut SectionMesh) {
        section.remap_materials(&self.material_remap);
    }

    pub fn textures(&self) -> &[TextureDesc] {
        &self.textures
    }

    pub const fn generation(&self) -> ResourceGeneration {
        self.generation
    }

    pub fn materials(&self) -> &[MaterialDesc] {
        &self.materials
    }

    /// Resolves a registration-order material identifier without touching any
    /// scene geometry. Streaming GPU upload uses this only after the finalized
    /// artifact has published its immutable resource table.
    pub fn canonical_material_id(&self, provisional: MaterialId) -> Option<MaterialId> {
        self.material_remap
            .get(provisional.value() as usize)
            .copied()
    }

    /// Returns the layer declared by the registration-order material table.
    /// This lets a backend validate committed draw metadata before applying
    /// the canonical ID remap.
    pub fn provisional_material_layer(
        &self,
        provisional: MaterialId,
    ) -> Option<TerrainRenderLayer> {
        self.provisional_material_layers
            .get(provisional.value() as usize)
            .copied()
    }

    /// Returns the face mode declared by the registration-order material
    /// table. Backends use this compact table to validate metadata that was
    /// bucketed before final material canonicalization.
    pub fn provisional_material_face_mode(&self, provisional: MaterialId) -> Option<FaceMode> {
        self.provisional_material_face_modes
            .get(provisional.value() as usize)
            .copied()
    }

    /// Registration-order to canonical material identifiers. This table is
    /// independent of scene geometry and can be uploaded once as a compact GPU
    /// remap for incrementally sealed backend pages.
    pub fn material_remap(&self) -> &[MaterialId] {
        &self.material_remap
    }

    /// Returns true when distinct provisional material IDs remain distinct
    /// after canonical resource remapping.
    pub fn material_remap_is_injective(&self) -> bool {
        self.material_remap.len() == self.materials.len()
    }

    pub fn into_resource_tables(self) -> (Box<[TextureDesc]>, Box<[MaterialDesc]>) {
        (self.textures, self.materials)
    }
}

impl SceneSnapshot {
    pub fn new(
        minecraft_semantics: MinecraftSemanticVersion,
        generation: ResourceGeneration,
        textures: Box<[TextureDesc]>,
        materials: Box<[MaterialDesc]>,
        mut sections: Box<[SectionMesh]>,
    ) -> Result<Self, SceneValidationError> {
        let resource_canonicalizer =
            SceneResourceCanonicalizer::new(generation, textures, materials)?;

        sections.sort_by_key(|section| section.position());
        if let Some(duplicate) = sections
            .windows(2)
            .find(|pair| pair[0].position() == pair[1].position())
        {
            return Err(SceneValidationError::DuplicateSectionPosition {
                position: duplicate[0].position(),
            });
        }

        for section in &sections {
            resource_canonicalizer.validate_section(section)?;
        }

        for section in &mut sections {
            resource_canonicalizer.canonicalize_validated_section(section);
        }
        let (textures, materials) = resource_canonicalizer.into_resource_tables();

        let primitive_count = sections.iter().try_fold(0_u64, |total, section| {
            total
                .checked_add(
                    section
                        .checked_primitive_count()
                        .ok_or(SceneValidationError::PrimitiveIdOverflow)?,
                )
                .ok_or(SceneValidationError::PrimitiveIdOverflow)
        })?;

        let mut next_primitive_id = 0_u64;
        for (section_index, section) in sections.iter_mut().enumerate() {
            let section_id = u32::try_from(section_index)
                .map(SectionId::new)
                .map_err(|_| SceneValidationError::SectionIdOverflow { section_index })?;
            section.set_id(section_id);
            section
                .assign_stable_primitive_ids(&mut next_primitive_id)
                .map_err(|()| SceneValidationError::PrimitiveIdOverflow)?;
        }
        debug_assert_eq!(next_primitive_id, primitive_count);

        Ok(Self {
            minecraft_semantics,
            generation,
            textures,
            materials,
            sections,
            primitive_count,
        })
    }

    pub const fn minecraft_semantics(&self) -> MinecraftSemanticVersion {
        self.minecraft_semantics
    }

    pub const fn generation(&self) -> ResourceGeneration {
        self.generation
    }

    pub fn textures(&self) -> &[TextureDesc] {
        &self.textures
    }

    pub fn materials(&self) -> &[MaterialDesc] {
        &self.materials
    }

    pub fn sections(&self) -> &[SectionMesh] {
        &self.sections
    }

    pub const fn primitive_count(&self) -> u64 {
        self.primitive_count
    }
}

fn canonicalize_table<T: Ord>(values: Box<[T]>) -> (Box<[T]>, Vec<u32>) {
    let value_count = values.len();
    let mut indexed_values = values
        .into_vec()
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();
    indexed_values.sort_by(|(left_index, left), (right_index, right)| {
        left.cmp(right).then_with(|| left_index.cmp(right_index))
    });

    let mut canonical = Vec::<T>::with_capacity(value_count);
    let mut remap = vec![0_u32; value_count];
    for (old_index, value) in indexed_values {
        if canonical.last().is_none_or(|previous| previous != &value) {
            canonical.push(value);
        }
        remap[old_index] = (canonical.len() - 1) as u32;
    }
    (canonical.into_boxed_slice(), remap)
}

fn validate_textures(
    generation: ResourceGeneration,
    textures: &[TextureDesc],
) -> Result<(), SceneValidationError> {
    for (texture_index, texture) in textures.iter().enumerate() {
        let texture_id = u32::try_from(texture_index)
            .map(TextureId::new)
            .map_err(|_| SceneValidationError::TextureTableTooLarge {
                texture_count: textures.len(),
            })?;
        if texture.generation() != generation {
            return Err(SceneValidationError::TextureGenerationMismatch {
                texture_id,
                expected: generation,
                actual: texture.generation(),
            });
        }
    }
    Ok(())
}

fn validate_material_textures(
    textures: &[TextureDesc],
    materials: &[MaterialDesc],
) -> Result<(), SceneValidationError> {
    for (material_index, material) in materials.iter().enumerate() {
        let material_id = u32::try_from(material_index)
            .map(MaterialId::new)
            .map_err(|_| SceneValidationError::MaterialTableTooLarge {
                material_count: materials.len(),
            })?;
        let texture_id = material.base_color_texture();
        let texture = textures.get(texture_id.value() as usize).ok_or(
            SceneValidationError::MaterialTextureOutOfBounds {
                material_id,
                texture_id,
                texture_count: textures.len(),
            },
        )?;
        if texture.semantic() != TextureSemantic::BaseColor {
            return Err(SceneValidationError::InvalidBaseColorTextureSemantic {
                texture_id,
                actual: texture.semantic(),
            });
        }
        if !texture.usage().sampled() {
            return Err(SceneValidationError::BaseColorTextureNotSampled { texture_id });
        }
    }
    Ok(())
}

fn validate_section_materials(
    section: &SectionMesh,
    material_layers: &[TerrainRenderLayer],
) -> Result<(), SceneValidationError> {
    for vertex in section.vertices() {
        validate_material_id(section.position(), vertex.material_id(), material_layers)?;
    }

    for (actual_layer, mesh) in section.layers() {
        for (draw_index, draw) in mesh.draws().iter().enumerate() {
            let material_layer =
                validate_material_id(section.position(), draw.material_id(), material_layers)?;
            if material_layer != actual_layer {
                return Err(SceneValidationError::MaterialLayerMismatch {
                    position: section.position(),
                    draw_index,
                    material_id: draw.material_id(),
                    material_layer,
                    mesh_layer: actual_layer,
                });
            }
        }
    }
    Ok(())
}

fn validate_material_id(
    position: SectionPosition,
    material_id: MaterialId,
    material_layers: &[TerrainRenderLayer],
) -> Result<TerrainRenderLayer, SceneValidationError> {
    material_layers
        .get(material_id.value() as usize)
        .copied()
        .ok_or(SceneValidationError::MaterialIdOutOfBounds {
            position,
            material_id,
            material_count: material_layers.len(),
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SceneValidationError {
    TextureTableTooLarge {
        texture_count: usize,
    },
    TextureGenerationMismatch {
        texture_id: TextureId,
        expected: ResourceGeneration,
        actual: ResourceGeneration,
    },
    InvalidBaseColorTextureSemantic {
        texture_id: TextureId,
        actual: TextureSemantic,
    },
    BaseColorTextureNotSampled {
        texture_id: TextureId,
    },
    MaterialTableTooLarge {
        material_count: usize,
    },
    MaterialTextureOutOfBounds {
        material_id: MaterialId,
        texture_id: TextureId,
        texture_count: usize,
    },
    DuplicateSectionPosition {
        position: SectionPosition,
    },
    MaterialIdOutOfBounds {
        position: SectionPosition,
        material_id: MaterialId,
        material_count: usize,
    },
    MaterialLayerMismatch {
        position: SectionPosition,
        draw_index: usize,
        material_id: MaterialId,
        material_layer: TerrainRenderLayer,
        mesh_layer: TerrainRenderLayer,
    },
    SectionIdOverflow {
        section_index: usize,
    },
    PrimitiveIdOverflow,
}

impl Display for SceneValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextureTableTooLarge { texture_count } => write!(
                formatter,
                "texture table contains {texture_count} entries and cannot use u32 IDs"
            ),
            Self::TextureGenerationMismatch {
                texture_id,
                expected,
                actual,
            } => write!(
                formatter,
                "texture {} belongs to generation {}, expected scene generation {}",
                texture_id.value(),
                actual.value(),
                expected.value()
            ),
            Self::InvalidBaseColorTextureSemantic { texture_id, actual } => write!(
                formatter,
                "material texture {} must use BaseColor semantics, got {actual:?}",
                texture_id.value()
            ),
            Self::BaseColorTextureNotSampled { texture_id } => write!(
                formatter,
                "material texture {} must allow shader sampling",
                texture_id.value()
            ),
            Self::MaterialTableTooLarge { material_count } => write!(
                formatter,
                "material table contains {material_count} entries and cannot use u32 IDs"
            ),
            Self::MaterialTextureOutOfBounds {
                material_id,
                texture_id,
                texture_count,
            } => write!(
                formatter,
                "material {} references texture {}, but only {texture_count} textures exist",
                material_id.value(),
                texture_id.value()
            ),
            Self::DuplicateSectionPosition { position } => write!(
                formatter,
                "scene contains duplicate section ({}, {}, {})",
                position.x(),
                position.y(),
                position.z()
            ),
            Self::MaterialIdOutOfBounds {
                position,
                material_id,
                material_count,
            } => write!(
                formatter,
                "section ({}, {}, {}) references material {}, but only {material_count} materials exist",
                position.x(),
                position.y(),
                position.z(),
                material_id.value()
            ),
            Self::MaterialLayerMismatch {
                position,
                draw_index,
                material_id,
                material_layer,
                mesh_layer,
            } => write!(
                formatter,
                "section ({}, {}, {}) {mesh_layer:?} draw {draw_index} uses material {} from {material_layer:?}",
                position.x(),
                position.y(),
                position.z(),
                material_id.value()
            ),
            Self::SectionIdOverflow { section_index } => write!(
                formatter,
                "section index {section_index} cannot be represented by SectionId"
            ),
            Self::PrimitiveIdOverflow => {
                formatter.write_str("scene primitive count overflows StablePrimitiveId")
            }
        }
    }
}

impl Error for SceneValidationError {}
