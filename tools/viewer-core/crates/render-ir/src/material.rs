use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::MinecraftIdentifier;

/// MC 26.2 terrain cutout threshold. Alpha exactly at the threshold survives.
pub const CUTOUT_ALPHA_CUTOFF: f32 = 0.5;

/// MC 26.2 terrain translucent threshold. Alpha exactly at the threshold survives.
pub const TRANSLUCENT_ALPHA_CUTOFF: f32 = 0.1;

/// Dense index into [`crate::SceneSnapshot::textures`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureId(u32);

impl TextureId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

/// Dense index into [`crate::SceneSnapshot::materials`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialId(u32);

impl MaterialId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TerrainRenderLayer {
    Opaque,
    Cutout,
    Translucent,
}

impl TerrainRenderLayer {
    pub const fn alpha_cutoff(self) -> Option<f32> {
        match self {
            Self::Opaque => None,
            Self::Cutout => Some(CUTOUT_ALPHA_CUTOFF),
            Self::Translucent => Some(TRANSLUCENT_ALPHA_CUTOFF),
        }
    }

    /// Mirrors MC 26.2's `alpha < cutoff` discard rule.
    pub fn keeps_fragment(self, alpha: f32) -> bool {
        alpha.is_finite()
            && self
                .alpha_cutoff()
                .is_none_or(|alpha_cutoff| alpha >= alpha_cutoff)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaceMode {
    CullBack,
    DoubleSided,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TintSource {
    None,
    Block(i32),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MaterialDesc {
    base_color_texture: TextureId,
    sprite_id: MinecraftIdentifier,
    layer: TerrainRenderLayer,
    face_mode: FaceMode,
    tint: TintSource,
    light_emission: u8,
}

impl MaterialDesc {
    pub fn new(
        base_color_texture: TextureId,
        sprite_id: MinecraftIdentifier,
        layer: TerrainRenderLayer,
        face_mode: FaceMode,
        tint: TintSource,
        light_emission: u8,
    ) -> Result<Self, MaterialValidationError> {
        if let TintSource::Block(tint_index) = tint
            && tint_index < 0
        {
            return Err(MaterialValidationError::InvalidTintIndex { tint_index });
        }
        if light_emission > 15 {
            return Err(MaterialValidationError::InvalidLightEmission { light_emission });
        }

        Ok(Self {
            base_color_texture,
            sprite_id,
            layer,
            face_mode,
            tint,
            light_emission,
        })
    }

    pub const fn base_color_texture(&self) -> TextureId {
        self.base_color_texture
    }

    pub fn sprite_id(&self) -> &MinecraftIdentifier {
        &self.sprite_id
    }

    pub const fn layer(&self) -> TerrainRenderLayer {
        self.layer
    }

    pub const fn face_mode(&self) -> FaceMode {
        self.face_mode
    }

    pub const fn tint(&self) -> TintSource {
        self.tint
    }

    pub const fn light_emission(&self) -> u8 {
        self.light_emission
    }

    pub(crate) fn remap_base_color_texture(&mut self, texture_id: TextureId) {
        self.base_color_texture = texture_id;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaterialValidationError {
    InvalidTintIndex { tint_index: i32 },
    InvalidLightEmission { light_emission: u8 },
}

impl Display for MaterialValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTintIndex { tint_index } => {
                write!(
                    formatter,
                    "block tint index must be non-negative, got {tint_index}"
                )
            }
            Self::InvalidLightEmission { light_emission } => write!(
                formatter,
                "MC light emission must be in 0..=15, got {light_emission}"
            ),
        }
    }
}

impl Error for MaterialValidationError {}
