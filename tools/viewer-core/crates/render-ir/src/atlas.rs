use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::Arc;

use crate::{MinecraftIdentifier, ResourceContentHash, ResourceGeneration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextureFormat {
    Rgba8Unorm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextureSemantic {
    BaseColor,
    Normal,
    Specular,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextureAlphaMode {
    Straight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextureAddressMode {
    ClampToEdge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FilterMode {
    Nearest,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureUsage {
    copy_source: bool,
    copy_destination: bool,
    sampled: bool,
    render_attachment: bool,
}

impl TextureUsage {
    pub const fn new(
        copy_source: bool,
        copy_destination: bool,
        sampled: bool,
        render_attachment: bool,
    ) -> Self {
        Self {
            copy_source,
            copy_destination,
            sampled,
            render_attachment,
        }
    }

    pub const fn copy_source(self) -> bool {
        self.copy_source
    }

    pub const fn copy_destination(self) -> bool {
        self.copy_destination
    }

    pub const fn sampled(self) -> bool {
        self.sampled
    }

    pub const fn render_attachment(self) -> bool {
        self.render_attachment
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SamplerDesc {
    address_u: TextureAddressMode,
    address_v: TextureAddressMode,
    min_filter: FilterMode,
    mag_filter: FilterMode,
    mipmap_filter: FilterMode,
    max_lod: u32,
}

impl SamplerDesc {
    pub const fn new(
        address_u: TextureAddressMode,
        address_v: TextureAddressMode,
        min_filter: FilterMode,
        mag_filter: FilterMode,
        mipmap_filter: FilterMode,
        max_lod: u32,
    ) -> Self {
        Self {
            address_u,
            address_v,
            min_filter,
            mag_filter,
            mipmap_filter,
            max_lod,
        }
    }

    pub const fn address_u(self) -> TextureAddressMode {
        self.address_u
    }

    pub const fn address_v(self) -> TextureAddressMode {
        self.address_v
    }

    pub const fn min_filter(self) -> FilterMode {
        self.min_filter
    }

    pub const fn mag_filter(self) -> FilterMode {
        self.mag_filter
    }

    pub const fn mipmap_filter(self) -> FilterMode {
        self.mipmap_filter
    }

    pub const fn max_lod(self) -> u32 {
        self.max_lod
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureDesc {
    id: MinecraftIdentifier,
    generation: ResourceGeneration,
    content_hash: ResourceContentHash,
    size: [u32; 2],
    mip_level_count: u32,
    format: TextureFormat,
    alpha_mode: TextureAlphaMode,
    semantic: TextureSemantic,
    usage: TextureUsage,
    sampler: SamplerDesc,
}

impl TextureDesc {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: MinecraftIdentifier,
        generation: ResourceGeneration,
        content_hash: ResourceContentHash,
        size: [u32; 2],
        mip_level_count: u32,
        format: TextureFormat,
        alpha_mode: TextureAlphaMode,
        semantic: TextureSemantic,
        usage: TextureUsage,
        sampler: SamplerDesc,
    ) -> Self {
        Self {
            id,
            generation,
            content_hash,
            size,
            mip_level_count,
            format,
            alpha_mode,
            semantic,
            usage,
            sampler,
        }
    }

    pub fn id(&self) -> &MinecraftIdentifier {
        &self.id
    }

    pub const fn generation(&self) -> ResourceGeneration {
        self.generation
    }

    pub const fn content_hash(&self) -> ResourceContentHash {
        self.content_hash
    }

    pub const fn size(&self) -> [u32; 2] {
        self.size
    }

    pub const fn mip_level_count(&self) -> u32 {
        self.mip_level_count
    }

    pub const fn format(&self) -> TextureFormat {
        self.format
    }

    pub const fn alpha_mode(&self) -> TextureAlphaMode {
        self.alpha_mode
    }

    pub const fn semantic(&self) -> TextureSemantic {
        self.semantic
    }

    pub const fn usage(&self) -> TextureUsage {
        self.usage
    }

    pub const fn sampler(&self) -> SamplerDesc {
        self.sampler
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasEdgeFill {
    ClampToEdge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasColorWrite {
    ReplaceRgba,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasInitialContents {
    Undefined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasBlitDesc {
    edge_fill: AtlasEdgeFill,
    filter: FilterMode,
    color_write: AtlasColorWrite,
}

impl AtlasBlitDesc {
    pub const fn new(
        edge_fill: AtlasEdgeFill,
        filter: FilterMode,
        color_write: AtlasColorWrite,
    ) -> Self {
        Self {
            edge_fill,
            filter,
            color_write,
        }
    }

    pub const fn edge_fill(self) -> AtlasEdgeFill {
        self.edge_fill
    }

    pub const fn filter(self) -> FilterMode {
        self.filter
    }

    pub const fn color_write(self) -> AtlasColorWrite {
        self.color_write
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasMipPlane {
    level: u32,
    size: [u32; 2],
    row_bytes: usize,
    rgba8: Arc<[u8]>,
}

impl AtlasMipPlane {
    pub fn new(level: u32, size: [u32; 2], rgba8: Arc<[u8]>) -> Result<Self, AtlasMipPlaneError> {
        let row_bytes = usize::try_from(size[0])
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or(AtlasMipPlaneError::SizeOverflow { size })?;
        let expected_bytes = usize::try_from(size[1])
            .ok()
            .and_then(|height| row_bytes.checked_mul(height))
            .ok_or(AtlasMipPlaneError::SizeOverflow { size })?;
        if rgba8.len() != expected_bytes {
            return Err(AtlasMipPlaneError::ByteLengthMismatch {
                size,
                expected_bytes,
                actual_bytes: rgba8.len(),
            });
        }
        Ok(Self {
            level,
            size,
            row_bytes,
            rgba8,
        })
    }

    pub const fn level(&self) -> u32 {
        self.level
    }

    pub const fn size(&self) -> [u32; 2] {
        self.size
    }

    pub const fn row_bytes(&self) -> usize {
        self.row_bytes
    }

    pub fn rgba8(&self) -> &[u8] {
        &self.rgba8
    }

    pub fn shared_rgba8(&self) -> Arc<[u8]> {
        Arc::clone(&self.rgba8)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AtlasMipPlaneError {
    SizeOverflow {
        size: [u32; 2],
    },
    ByteLengthMismatch {
        size: [u32; 2],
        expected_bytes: usize,
        actual_bytes: usize,
    },
}

impl Display for AtlasMipPlaneError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SizeOverflow { size } => write!(
                formatter,
                "RGBA8 mip size {}x{} overflows this platform",
                size[0], size[1]
            ),
            Self::ByteLengthMismatch {
                size,
                expected_bytes,
                actual_bytes,
            } => write!(
                formatter,
                "RGBA8 mip {}x{} requires {expected_bytes} bytes but received {actual_bytes}",
                size[0], size[1]
            ),
        }
    }
}

impl Error for AtlasMipPlaneError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasFrameRegion {
    level: u32,
    source_origin: [u32; 2],
    size: [u32; 2],
}

impl AtlasFrameRegion {
    pub const fn new(level: u32, source_origin: [u32; 2], size: [u32; 2]) -> Self {
        Self {
            level,
            source_origin,
            size,
        }
    }

    pub const fn level(&self) -> u32 {
        self.level
    }

    pub const fn source_origin(&self) -> [u32; 2] {
        self.source_origin
    }

    pub const fn size(&self) -> [u32; 2] {
        self.size
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasFrameUpload {
    physical_index: u32,
    regions: Box<[AtlasFrameRegion]>,
}

impl AtlasFrameUpload {
    pub fn new(physical_index: u32, regions: Box<[AtlasFrameRegion]>) -> Self {
        Self {
            physical_index,
            regions,
        }
    }

    pub const fn physical_index(&self) -> u32 {
        self.physical_index
    }

    pub fn regions(&self) -> &[AtlasFrameRegion] {
        &self.regions
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasMipTarget {
    level: u32,
    slot_origin: [u32; 2],
    content_origin: [u32; 2],
    frame_size: [u32; 2],
    padded_size: [u32; 2],
}

impl AtlasMipTarget {
    pub const fn new(
        level: u32,
        slot_origin: [u32; 2],
        content_origin: [u32; 2],
        frame_size: [u32; 2],
        padded_size: [u32; 2],
    ) -> Self {
        Self {
            level,
            slot_origin,
            content_origin,
            frame_size,
            padded_size,
        }
    }

    pub const fn level(&self) -> u32 {
        self.level
    }

    pub const fn slot_origin(&self) -> [u32; 2] {
        self.slot_origin
    }

    pub const fn content_origin(&self) -> [u32; 2] {
        self.content_origin
    }

    pub const fn frame_size(&self) -> [u32; 2] {
        self.frame_size
    }

    pub const fn padded_size(&self) -> [u32; 2] {
        self.padded_size
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasAnimationMode {
    Static,
    Discrete,
    Interpolated,
}

impl AtlasAnimationMode {
    pub const fn is_animated(self) -> bool {
        !matches!(self, Self::Static)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasSpriteUpload {
    sprite_id: MinecraftIdentifier,
    requested_name: MinecraftIdentifier,
    source_mips: Box<[AtlasMipPlane]>,
    frames: Box<[AtlasFrameUpload]>,
    targets: Box<[AtlasMipTarget]>,
    initial_frame: u32,
    animation_mode: AtlasAnimationMode,
}

impl AtlasSpriteUpload {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sprite_id: MinecraftIdentifier,
        requested_name: MinecraftIdentifier,
        source_mips: Box<[AtlasMipPlane]>,
        frames: Box<[AtlasFrameUpload]>,
        targets: Box<[AtlasMipTarget]>,
        initial_frame: u32,
        animation_mode: AtlasAnimationMode,
    ) -> Self {
        Self {
            sprite_id,
            requested_name,
            source_mips,
            frames,
            targets,
            initial_frame,
            animation_mode,
        }
    }

    pub fn sprite_id(&self) -> &MinecraftIdentifier {
        &self.sprite_id
    }

    pub fn requested_name(&self) -> &MinecraftIdentifier {
        &self.requested_name
    }

    pub fn source_mips(&self) -> &[AtlasMipPlane] {
        &self.source_mips
    }

    pub fn frames(&self) -> &[AtlasFrameUpload] {
        &self.frames
    }

    pub fn targets(&self) -> &[AtlasMipTarget] {
        &self.targets
    }

    pub const fn initial_frame(&self) -> u32 {
        self.initial_frame
    }

    pub const fn is_animated(&self) -> bool {
        self.animation_mode.is_animated()
    }

    pub const fn animation_mode(&self) -> AtlasAnimationMode {
        self.animation_mode
    }
}

#[derive(Debug)]
#[must_use = "the atlas upload plan must be staged by a renderer"]
pub struct AtlasUploadPlan {
    atlas_id: MinecraftIdentifier,
    texture: TextureDesc,
    padding: u32,
    blit: AtlasBlitDesc,
    initial_contents: AtlasInitialContents,
    sprites: Box<[AtlasSpriteUpload]>,
}

impl AtlasUploadPlan {
    pub fn new(
        atlas_id: MinecraftIdentifier,
        texture: TextureDesc,
        padding: u32,
        blit: AtlasBlitDesc,
        initial_contents: AtlasInitialContents,
        sprites: Box<[AtlasSpriteUpload]>,
    ) -> Result<Self, AtlasUploadPlanValidationError> {
        let plan = Self {
            atlas_id,
            texture,
            padding,
            blit,
            initial_contents,
            sprites,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn atlas_id(&self) -> &MinecraftIdentifier {
        &self.atlas_id
    }

    pub fn texture(&self) -> &TextureDesc {
        &self.texture
    }

    pub const fn padding(&self) -> u32 {
        self.padding
    }

    pub const fn blit(&self) -> AtlasBlitDesc {
        self.blit
    }

    pub const fn initial_contents(&self) -> AtlasInitialContents {
        self.initial_contents
    }

    pub fn sprites(&self) -> &[AtlasSpriteUpload] {
        &self.sprites
    }

    pub fn sprite(&self, id: &MinecraftIdentifier) -> Option<&AtlasSpriteUpload> {
        self.sprites.iter().find(|sprite| sprite.sprite_id == *id)
    }

    pub fn animation_contract(&self) -> AtlasAnimationContract {
        let animations = self
            .sprites
            .iter()
            .filter(|sprite| sprite.is_animated())
            .map(|sprite| {
                (
                    sprite.sprite_id.clone(),
                    AtlasAnimationSpriteContract {
                        mode: sprite.animation_mode,
                        physical_frames: sprite
                            .frames
                            .iter()
                            .map(|frame| frame.physical_index)
                            .collect(),
                    },
                )
            })
            .collect();
        AtlasAnimationContract {
            texture_id: self.texture.id.clone(),
            generation: self.texture.generation,
            content_hash: self.texture.content_hash,
            animations,
        }
    }

    fn validate(&self) -> Result<(), AtlasUploadPlanValidationError> {
        let texture_size = self.texture.size();
        if texture_size[0] == 0 || texture_size[1] == 0 {
            return Err(AtlasUploadPlanValidationError::InvalidTextureSize { size: texture_size });
        }
        let mip_level_count = self.texture.mip_level_count();
        let max_mip_level_count = u32::BITS - texture_size[0].max(texture_size[1]).leading_zeros();
        if mip_level_count == 0 || mip_level_count > max_mip_level_count {
            return Err(
                AtlasUploadPlanValidationError::InvalidTextureMipLevelCount {
                    size: texture_size,
                    mip_level_count,
                    max_mip_level_count,
                },
            );
        }
        if self.texture.sampler().max_lod() >= mip_level_count {
            return Err(AtlasUploadPlanValidationError::InvalidSamplerMaxLod {
                max_lod: self.texture.sampler().max_lod(),
                mip_level_count,
            });
        }
        let usage = self.texture.usage();
        for (name, enabled) in [
            ("copy_source", usage.copy_source()),
            ("copy_destination", usage.copy_destination()),
            ("sampled", usage.sampled()),
            ("render_attachment", usage.render_attachment()),
        ] {
            if !enabled {
                return Err(AtlasUploadPlanValidationError::MissingTextureUsage { name });
            }
        }
        if self.sprites.is_empty() {
            return Err(AtlasUploadPlanValidationError::EmptySprites);
        }

        let expected_mips = mip_level_count as usize;
        let mut targets_by_level = (0..expected_mips)
            .map(|_| Vec::new())
            .collect::<Vec<Vec<AtlasTargetRect>>>();
        let mut sprite_ids = BTreeSet::new();
        let mut animated_seen = false;
        for sprite in &self.sprites {
            if !sprite_ids.insert(sprite.sprite_id.clone()) {
                return Err(AtlasUploadPlanValidationError::DuplicateSprite {
                    sprite: sprite.sprite_id.clone(),
                });
            }
            if sprite.is_animated() {
                animated_seen = true;
            } else if animated_seen {
                return Err(AtlasUploadPlanValidationError::StaticAfterAnimated {
                    sprite: sprite.sprite_id.clone(),
                });
            }
            if sprite.source_mips.len() != expected_mips {
                return Err(AtlasUploadPlanValidationError::SpriteMipCountMismatch {
                    sprite: sprite.sprite_id.clone(),
                    expected: expected_mips,
                    actual: sprite.source_mips.len(),
                });
            }
            if sprite.targets.len() != expected_mips {
                return Err(AtlasUploadPlanValidationError::SpriteTargetCountMismatch {
                    sprite: sprite.sprite_id.clone(),
                    expected: expected_mips,
                    actual: sprite.targets.len(),
                });
            }
            if sprite.frames.is_empty() {
                return Err(AtlasUploadPlanValidationError::EmptyFrames {
                    sprite: sprite.sprite_id.clone(),
                });
            }
            if !sprite.is_animated() && sprite.frames.len() != 1 {
                return Err(AtlasUploadPlanValidationError::StaticFrameCountMismatch {
                    sprite: sprite.sprite_id.clone(),
                    actual: sprite.frames.len(),
                });
            }

            let mut physical_frames = BTreeSet::new();
            for frame in &sprite.frames {
                if !physical_frames.insert(frame.physical_index) {
                    return Err(AtlasUploadPlanValidationError::DuplicatePhysicalFrame {
                        sprite: sprite.sprite_id.clone(),
                        physical_frame: frame.physical_index,
                    });
                }
                if frame.regions.len() != expected_mips {
                    return Err(AtlasUploadPlanValidationError::FrameRegionCountMismatch {
                        sprite: sprite.sprite_id.clone(),
                        physical_frame: frame.physical_index,
                        expected: expected_mips,
                        actual: frame.regions.len(),
                    });
                }
            }
            if !physical_frames.contains(&sprite.initial_frame) {
                return Err(AtlasUploadPlanValidationError::MissingInitialFrame {
                    sprite: sprite.sprite_id.clone(),
                    initial_frame: sprite.initial_frame,
                });
            }

            let base_sheet_size = sprite.source_mips[0].size();
            let base_target = &sprite.targets[0];
            let base_frame_size = base_target.frame_size();
            let doubled_padding = self.padding.checked_mul(2).ok_or_else(|| {
                AtlasUploadPlanValidationError::TargetArithmeticOverflow {
                    sprite: sprite.sprite_id.clone(),
                    level: 0,
                }
            })?;
            let base_padded_size = [
                base_frame_size[0]
                    .checked_add(doubled_padding)
                    .ok_or_else(
                        || AtlasUploadPlanValidationError::TargetArithmeticOverflow {
                            sprite: sprite.sprite_id.clone(),
                            level: 0,
                        },
                    )?,
                base_frame_size[1]
                    .checked_add(doubled_padding)
                    .ok_or_else(
                        || AtlasUploadPlanValidationError::TargetArithmeticOverflow {
                            sprite: sprite.sprite_id.clone(),
                            level: 0,
                        },
                    )?,
            ];
            let base_content_origin = [
                base_target.slot_origin[0].checked_add(self.padding),
                base_target.slot_origin[1].checked_add(self.padding),
            ];
            if base_content_origin[0] != Some(base_target.content_origin[0])
                || base_content_origin[1] != Some(base_target.content_origin[1])
            {
                return Err(AtlasUploadPlanValidationError::InvalidTargetGeometry {
                    sprite: sprite.sprite_id.clone(),
                    level: 0,
                });
            }

            for (level_index, ((plane, target), level_targets)) in sprite
                .source_mips
                .iter()
                .zip(sprite.targets.iter())
                .zip(targets_by_level.iter_mut())
                .enumerate()
            {
                let level = level_index as u32;
                if plane.level != level {
                    return Err(AtlasUploadPlanValidationError::MipLevelMismatch {
                        sprite: sprite.sprite_id.clone(),
                        section: AtlasMipSection::Source,
                        expected: level,
                        actual: plane.level,
                    });
                }
                if target.level != level {
                    return Err(AtlasUploadPlanValidationError::MipLevelMismatch {
                        sprite: sprite.sprite_id.clone(),
                        section: AtlasMipSection::Target,
                        expected: level,
                        actual: target.level,
                    });
                }
                let expected_sheet_size = [
                    (base_sheet_size[0] >> level).max(1),
                    (base_sheet_size[1] >> level).max(1),
                ];
                if plane.size != expected_sheet_size {
                    return Err(AtlasUploadPlanValidationError::MipSizeMismatch {
                        sprite: sprite.sprite_id.clone(),
                        level,
                        expected: expected_sheet_size,
                        actual: plane.size,
                    });
                }
                let expected_frame_size = [
                    (base_frame_size[0] >> level).max(1),
                    (base_frame_size[1] >> level).max(1),
                ];
                let expected_padded_size = [
                    (base_padded_size[0] >> level).max(1),
                    (base_padded_size[1] >> level).max(1),
                ];
                let expected_slot_origin = [
                    base_target.slot_origin[0] >> level,
                    base_target.slot_origin[1] >> level,
                ];
                let expected_content_origin = [
                    base_target.content_origin[0] >> level,
                    base_target.content_origin[1] >> level,
                ];
                if target.slot_origin != expected_slot_origin
                    || target.content_origin != expected_content_origin
                    || target.frame_size != expected_frame_size
                    || target.padded_size != expected_padded_size
                {
                    return Err(AtlasUploadPlanValidationError::InvalidTargetGeometry {
                        sprite: sprite.sprite_id.clone(),
                        level,
                    });
                }
                let atlas_mip_size = [
                    (texture_size[0] >> level).max(1),
                    (texture_size[1] >> level).max(1),
                ];
                let target_end = [
                    target.slot_origin[0].checked_add(target.padded_size[0]),
                    target.slot_origin[1].checked_add(target.padded_size[1]),
                ];
                let [Some(target_end_x), Some(target_end_y)] = target_end else {
                    return Err(AtlasUploadPlanValidationError::TargetOutOfBounds {
                        sprite: sprite.sprite_id.clone(),
                        level,
                        atlas_size: atlas_mip_size,
                    });
                };
                if target_end_x > atlas_mip_size[0] || target_end_y > atlas_mip_size[1] {
                    return Err(AtlasUploadPlanValidationError::TargetOutOfBounds {
                        sprite: sprite.sprite_id.clone(),
                        level,
                        atlas_size: atlas_mip_size,
                    });
                }
                level_targets.push(AtlasTargetRect {
                    sprite: sprite.sprite_id.clone(),
                    origin: target.slot_origin,
                    end: [target_end_x, target_end_y],
                });

                for frame in &sprite.frames {
                    let region = &frame.regions[level_index];
                    if region.level != level {
                        return Err(AtlasUploadPlanValidationError::MipLevelMismatch {
                            sprite: sprite.sprite_id.clone(),
                            section: AtlasMipSection::Frame,
                            expected: level,
                            actual: region.level,
                        });
                    }
                    if region.size != expected_frame_size {
                        return Err(AtlasUploadPlanValidationError::FrameRegionSizeMismatch {
                            sprite: sprite.sprite_id.clone(),
                            physical_frame: frame.physical_index,
                            level,
                            expected: expected_frame_size,
                            actual: region.size,
                        });
                    }
                    let source_end = [
                        region.source_origin[0].checked_add(region.size[0]),
                        region.source_origin[1].checked_add(region.size[1]),
                    ];
                    if source_end[0].is_none_or(|end| end > plane.size[0])
                        || source_end[1].is_none_or(|end| end > plane.size[1])
                    {
                        return Err(AtlasUploadPlanValidationError::FrameRegionOutOfBounds {
                            sprite: sprite.sprite_id.clone(),
                            physical_frame: frame.physical_index,
                            level,
                            sheet_size: plane.size,
                        });
                    }
                }
            }
        }
        for (level, targets) in targets_by_level.iter_mut().enumerate() {
            validate_non_overlapping_targets(level as u32, targets)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct AtlasTargetRect {
    sprite: MinecraftIdentifier,
    origin: [u32; 2],
    end: [u32; 2],
}

fn validate_non_overlapping_targets(
    level: u32,
    targets: &mut [AtlasTargetRect],
) -> Result<(), AtlasUploadPlanValidationError> {
    targets.sort_unstable_by(|left, right| {
        left.origin
            .cmp(&right.origin)
            .then_with(|| left.sprite.cmp(&right.sprite))
    });
    let mut active_by_y = BTreeMap::<u32, usize>::new();
    let mut expiration = BinaryHeap::<Reverse<(u32, u32, usize)>>::new();
    for (index, target) in targets.iter().enumerate() {
        while let Some(Reverse((end_x, origin_y, expired_index))) = expiration.peek().copied() {
            if end_x > target.origin[0] {
                break;
            }
            expiration.pop();
            if active_by_y.get(&origin_y) == Some(&expired_index) {
                active_by_y.remove(&origin_y);
            }
        }

        let predecessor = active_by_y
            .range(..=target.origin[1])
            .next_back()
            .map(|(_, &other_index)| other_index)
            .filter(|&other_index| targets[other_index].end[1] > target.origin[1]);
        let successor = active_by_y
            .range((Excluded(target.origin[1]), Unbounded))
            .next()
            .map(|(_, &other_index)| other_index)
            .filter(|&other_index| targets[other_index].origin[1] < target.end[1]);
        if let Some(other_index) = predecessor.or(successor) {
            return Err(AtlasUploadPlanValidationError::OverlappingTargets {
                first: targets[other_index].sprite.clone(),
                second: target.sprite.clone(),
                level,
            });
        }

        active_by_y.insert(target.origin[1], index);
        expiration.push(Reverse((target.end[0], target.origin[1], index)));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasMipSection {
    Source,
    Target,
    Frame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AtlasUploadPlanValidationError {
    InvalidTextureSize {
        size: [u32; 2],
    },
    InvalidTextureMipLevelCount {
        size: [u32; 2],
        mip_level_count: u32,
        max_mip_level_count: u32,
    },
    InvalidSamplerMaxLod {
        max_lod: u32,
        mip_level_count: u32,
    },
    MissingTextureUsage {
        name: &'static str,
    },
    EmptySprites,
    DuplicateSprite {
        sprite: MinecraftIdentifier,
    },
    StaticAfterAnimated {
        sprite: MinecraftIdentifier,
    },
    SpriteMipCountMismatch {
        sprite: MinecraftIdentifier,
        expected: usize,
        actual: usize,
    },
    SpriteTargetCountMismatch {
        sprite: MinecraftIdentifier,
        expected: usize,
        actual: usize,
    },
    EmptyFrames {
        sprite: MinecraftIdentifier,
    },
    StaticFrameCountMismatch {
        sprite: MinecraftIdentifier,
        actual: usize,
    },
    DuplicatePhysicalFrame {
        sprite: MinecraftIdentifier,
        physical_frame: u32,
    },
    FrameRegionCountMismatch {
        sprite: MinecraftIdentifier,
        physical_frame: u32,
        expected: usize,
        actual: usize,
    },
    MissingInitialFrame {
        sprite: MinecraftIdentifier,
        initial_frame: u32,
    },
    TargetArithmeticOverflow {
        sprite: MinecraftIdentifier,
        level: u32,
    },
    MipLevelMismatch {
        sprite: MinecraftIdentifier,
        section: AtlasMipSection,
        expected: u32,
        actual: u32,
    },
    MipSizeMismatch {
        sprite: MinecraftIdentifier,
        level: u32,
        expected: [u32; 2],
        actual: [u32; 2],
    },
    InvalidTargetGeometry {
        sprite: MinecraftIdentifier,
        level: u32,
    },
    TargetOutOfBounds {
        sprite: MinecraftIdentifier,
        level: u32,
        atlas_size: [u32; 2],
    },
    OverlappingTargets {
        first: MinecraftIdentifier,
        second: MinecraftIdentifier,
        level: u32,
    },
    FrameRegionSizeMismatch {
        sprite: MinecraftIdentifier,
        physical_frame: u32,
        level: u32,
        expected: [u32; 2],
        actual: [u32; 2],
    },
    FrameRegionOutOfBounds {
        sprite: MinecraftIdentifier,
        physical_frame: u32,
        level: u32,
        sheet_size: [u32; 2],
    },
}

impl Display for AtlasUploadPlanValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid atlas upload plan: {self:?}")
    }
}

impl Error for AtlasUploadPlanValidationError {}

#[derive(Clone, Debug)]
pub struct AtlasAnimationContract {
    texture_id: MinecraftIdentifier,
    generation: ResourceGeneration,
    content_hash: ResourceContentHash,
    animations: BTreeMap<MinecraftIdentifier, AtlasAnimationSpriteContract>,
}

#[derive(Clone, Debug)]
struct AtlasAnimationSpriteContract {
    mode: AtlasAnimationMode,
    physical_frames: BTreeSet<u32>,
}

impl AtlasAnimationContract {
    pub fn texture_id(&self) -> &MinecraftIdentifier {
        &self.texture_id
    }

    pub const fn generation(&self) -> ResourceGeneration {
        self.generation
    }

    pub const fn content_hash(&self) -> ResourceContentHash {
        self.content_hash
    }

    pub fn animation_count(&self) -> usize {
        self.animations.len()
    }

    pub fn animations(
        &self,
    ) -> impl ExactSizeIterator<Item = (&MinecraftIdentifier, AtlasAnimationMode)> {
        self.animations
            .iter()
            .map(|(sprite, contract)| (sprite, contract.mode))
    }

    pub fn animation_mode(&self, sprite: &MinecraftIdentifier) -> Option<AtlasAnimationMode> {
        self.animations.get(sprite).map(|contract| contract.mode)
    }

    pub fn contains_physical_frame(
        &self,
        sprite: &MinecraftIdentifier,
        physical_frame: u32,
    ) -> bool {
        self.animations
            .get(sprite)
            .is_some_and(|contract| contract.physical_frames.contains(&physical_frame))
    }

    pub fn build_update_plan(
        &self,
        updates: Box<[AtlasAnimationUpdate]>,
    ) -> Result<AtlasAnimationUpdatePlan, AtlasAnimationUpdatePlanValidationError> {
        let mut seen_sprites = BTreeSet::new();
        for update in &updates {
            if !seen_sprites.insert(update.sprite_id.clone()) {
                return Err(AtlasAnimationUpdatePlanValidationError::DuplicateSprite {
                    sprite: update.sprite_id.clone(),
                });
            }
            let Some(animation) = self.animations.get(&update.sprite_id) else {
                return Err(
                    AtlasAnimationUpdatePlanValidationError::UnknownAnimatedSprite {
                        sprite: update.sprite_id.clone(),
                    },
                );
            };
            if update.interpolation_milli > 1_000 {
                return Err(
                    AtlasAnimationUpdatePlanValidationError::InvalidInterpolationMilli {
                        sprite: update.sprite_id.clone(),
                        interpolation_milli: update.interpolation_milli,
                    },
                );
            }
            match animation.mode {
                AtlasAnimationMode::Static => {
                    return Err(
                        AtlasAnimationUpdatePlanValidationError::UnknownAnimatedSprite {
                            sprite: update.sprite_id.clone(),
                        },
                    );
                }
                AtlasAnimationMode::Discrete
                    if update.next_frame.is_some() || update.interpolation_milli != 0 =>
                {
                    return Err(
                        AtlasAnimationUpdatePlanValidationError::InterpolationOnDiscreteSprite {
                            sprite: update.sprite_id.clone(),
                            next_frame: update.next_frame,
                            interpolation_milli: update.interpolation_milli,
                        },
                    );
                }
                AtlasAnimationMode::Interpolated if update.next_frame.is_none() => {
                    return Err(AtlasAnimationUpdatePlanValidationError::MissingNextFrame {
                        sprite: update.sprite_id.clone(),
                    });
                }
                AtlasAnimationMode::Discrete | AtlasAnimationMode::Interpolated => {}
            }
            for physical_frame in [Some(update.current_frame), update.next_frame]
                .into_iter()
                .flatten()
            {
                if !animation.physical_frames.contains(&physical_frame) {
                    return Err(
                        AtlasAnimationUpdatePlanValidationError::UnknownPhysicalFrame {
                            sprite: update.sprite_id.clone(),
                            physical_frame,
                        },
                    );
                }
            }
        }
        Ok(AtlasAnimationUpdatePlan {
            texture_id: self.texture_id.clone(),
            generation: self.generation,
            content_hash: self.content_hash,
            updates,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasAnimationUpdate {
    sprite_id: MinecraftIdentifier,
    current_frame: u32,
    next_frame: Option<u32>,
    interpolation_milli: u16,
}

impl AtlasAnimationUpdate {
    pub const fn new(
        sprite_id: MinecraftIdentifier,
        current_frame: u32,
        next_frame: Option<u32>,
        interpolation_milli: u16,
    ) -> Self {
        Self {
            sprite_id,
            current_frame,
            next_frame,
            interpolation_milli,
        }
    }

    pub fn sprite_id(&self) -> &MinecraftIdentifier {
        &self.sprite_id
    }

    pub const fn current_frame(&self) -> u32 {
        self.current_frame
    }

    pub const fn next_frame(&self) -> Option<u32> {
        self.next_frame
    }

    /// Next-frame mix weight in thousandths; zero means the current frame only.
    pub const fn interpolation_milli(&self) -> u16 {
        self.interpolation_milli
    }
}

#[derive(Debug)]
#[must_use = "animation updates must be submitted to the matching atlas generation"]
pub struct AtlasAnimationUpdatePlan {
    texture_id: MinecraftIdentifier,
    generation: ResourceGeneration,
    content_hash: ResourceContentHash,
    updates: Box<[AtlasAnimationUpdate]>,
}

impl AtlasAnimationUpdatePlan {
    pub fn texture_id(&self) -> &MinecraftIdentifier {
        &self.texture_id
    }

    pub const fn generation(&self) -> ResourceGeneration {
        self.generation
    }

    pub const fn content_hash(&self) -> ResourceContentHash {
        self.content_hash
    }

    pub fn updates(&self) -> &[AtlasAnimationUpdate] {
        &self.updates
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AtlasAnimationUpdatePlanValidationError {
    DuplicateSprite {
        sprite: MinecraftIdentifier,
    },
    UnknownAnimatedSprite {
        sprite: MinecraftIdentifier,
    },
    UnknownPhysicalFrame {
        sprite: MinecraftIdentifier,
        physical_frame: u32,
    },
    InvalidInterpolationMilli {
        sprite: MinecraftIdentifier,
        interpolation_milli: u16,
    },
    InterpolationOnDiscreteSprite {
        sprite: MinecraftIdentifier,
        next_frame: Option<u32>,
        interpolation_milli: u16,
    },
    MissingNextFrame {
        sprite: MinecraftIdentifier,
    },
}

impl Display for AtlasAnimationUpdatePlanValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateSprite { sprite } => {
                write!(formatter, "animation update repeats sprite {sprite}")
            }
            Self::UnknownAnimatedSprite { sprite } => {
                write!(
                    formatter,
                    "animation update references unknown sprite {sprite}"
                )
            }
            Self::UnknownPhysicalFrame {
                sprite,
                physical_frame,
            } => write!(
                formatter,
                "animation update for {sprite} references unknown physical frame {physical_frame}"
            ),
            Self::InvalidInterpolationMilli {
                sprite,
                interpolation_milli,
            } => write!(
                formatter,
                "animation update for {sprite} has invalid mix weight {interpolation_milli}; expected 0..=1000"
            ),
            Self::InterpolationOnDiscreteSprite {
                sprite,
                next_frame,
                interpolation_milli,
            } => write!(
                formatter,
                "discrete animation update for {sprite} has next frame {next_frame:?} and mix weight {interpolation_milli}"
            ),
            Self::MissingNextFrame { sprite } => {
                write!(
                    formatter,
                    "interpolated animation update for {sprite} has no next frame"
                )
            }
        }
    }
}

impl Error for AtlasAnimationUpdatePlanValidationError {}
