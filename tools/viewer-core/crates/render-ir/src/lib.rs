#![forbid(unsafe_code)]

//! Backend-independent scene, material, resource identity, and frame-view contracts.

mod atlas;
mod frame;
mod material;
mod mesh;
mod resource;
mod scene;

pub use atlas::{
    AtlasAnimationContract, AtlasAnimationMode, AtlasAnimationUpdate, AtlasAnimationUpdatePlan,
    AtlasAnimationUpdatePlanValidationError, AtlasBlitDesc, AtlasColorWrite, AtlasEdgeFill,
    AtlasFrameRegion, AtlasFrameUpload, AtlasInitialContents, AtlasMipPlane, AtlasMipPlaneError,
    AtlasMipSection, AtlasMipTarget, AtlasSpriteUpload, AtlasUploadPlan,
    AtlasUploadPlanValidationError, FilterMode, SamplerDesc, TextureAddressMode, TextureAlphaMode,
    TextureDesc, TextureFormat, TextureSemantic, TextureUsage,
};
pub use frame::{
    DepthConvention, FrameMatrix, FrameView, FrameViewValidationError, TerrainFog,
    TerrainFogValidationError,
};
pub use material::{
    CUTOUT_ALPHA_CUTOFF, FaceMode, MaterialDesc, MaterialId, MaterialValidationError,
    TRANSLUCENT_ALPHA_CUTOFF, TerrainRenderLayer, TextureId, TintSource,
};
pub use mesh::{
    Aabb, AabbBound, DrawRange, IrisTerrainColor, IrisTerrainRenderType,
    IrisTerrainVertexAttributes, LayerMesh, LightCoordinates, MeshValidationError, SectionId,
    SectionMesh, SectionPosition, ShaderVertex, StablePrimitiveId, VertexAttribute, VertexColor,
};
pub use resource::{
    MinecraftIdentifier, MinecraftIdentifierError, MinecraftRelease, MinecraftSemanticVersion,
    ResourceCacheKey, ResourceContentHash, ResourceGeneration, ResourceKey, ResourceKeyError,
};
pub use scene::{SceneResourceCanonicalizer, SceneSnapshot, SceneValidationError};
