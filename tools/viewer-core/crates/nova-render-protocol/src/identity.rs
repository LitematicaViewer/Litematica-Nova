//! Deterministic render identity derivation shared by protocol participants.
//!
//! These functions are deliberately pure: callers provide already captured
//! fingerprints and digests. In particular, this module never opens, scans or
//! hashes scene/resource files.

use crate::{
    BuildIdentityMaterial, BuildStart, ContentIdentity, PresentationSpec, RenderBackend,
    RenderBuildMode,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvisionalSceneKey<'a> {
    source_path: &'a Path,
    display_mode: &'a str,
    build_mode: RenderBuildMode,
    content: &'a crate::SceneContent,
    build_options: &'a BTreeMap<String, String>,
}

/// Correlates an unresolved request without inspecting any referenced file.
pub fn provisional_scene_key(request: &BuildStart) -> Result<String, serde_json::Error> {
    hash_identity(
        "scene-build-pending",
        &ProvisionalSceneKey {
            source_path: &request.source_path,
            display_mode: &request.display_mode,
            build_mode: request.build_mode,
            content: &request.content,
            build_options: &request.build_options,
        },
    )
}

/// Derives the stable scene/build identity from fingerprints captured once by
/// the build worker. Request-local ids and generations are intentionally not
/// identity material.
pub fn scene_build_identity(material: &BuildIdentityMaterial) -> Result<String, serde_json::Error> {
    let mut content = material.resource_inputs.clone();
    content.sort_by(|left, right| {
        (&left.kind, &left.name, &left.sha256).cmp(&(&right.kind, &right.name, &right.sha256))
    });
    scene_build_identity_from_parts(
        &material.source_sha256,
        material.data_version,
        material.canonical_schema_version,
        material.adapter_revision,
        &content,
        &material.assets_digest,
        &material.build_options,
    )
}

fn scene_build_identity_from_parts(
    source_sha256: &str,
    data_version: i32,
    canonical_schema_version: u32,
    adapter_revision: u32,
    resource_inputs: &[ContentIdentity],
    assets_digest: &str,
    build_options: &BTreeMap<String, String>,
) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Material<'a> {
        source_sha256: &'a str,
        data_version: i32,
        canonical_schema_version: u32,
        adapter_revision: u32,
        resource_inputs: &'a [ContentIdentity],
        assets_digest: &'a str,
        build_options: &'a BTreeMap<String, String>,
    }

    hash_identity(
        "scene-build",
        &Material {
            source_sha256,
            data_version,
            canonical_schema_version,
            adapter_revision,
            resource_inputs,
            assets_digest,
            build_options,
        },
    )
}

/// Derives a presentation correlation key without reading the shader pack.
pub fn provisional_presentation_identity(
    scene_identity: &str,
    presentation: &PresentationSpec,
) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Material<'a> {
        scene_identity: &'a str,
        backend: RenderBackend,
        shaderpack_path: Option<&'a Path>,
        render_settings: &'a BTreeMap<String, String>,
    }

    let mut render_settings = presentation.render_settings.clone();
    render_settings.remove("clickStartedAtEpochMs");
    hash_identity(
        "presentation-pending",
        &Material {
            scene_identity,
            backend: presentation.backend,
            shaderpack_path: presentation.shaderpack.as_deref(),
            render_settings: &render_settings,
        },
    )
}

/// Derives a stable presentation identity from a digest captured by the
/// render service's existing content cache.
pub fn presentation_identity_from_digest(
    scene_identity: &str,
    presentation: &PresentationSpec,
    shaderpack_sha256: Option<String>,
) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Material<'a> {
        scene_identity: &'a str,
        backend: RenderBackend,
        shaderpack_sha256: Option<String>,
        render_settings: &'a BTreeMap<String, String>,
    }

    let mut render_settings = presentation.render_settings.clone();
    render_settings.remove("clickStartedAtEpochMs");
    hash_identity(
        "presentation",
        &Material {
            scene_identity,
            backend: presentation.backend,
            shaderpack_sha256,
            render_settings: &render_settings,
        },
    )
}

/// Binds a stable presentation to one renderer device generation. Equality is
/// correlation only; it does not claim that an artifact or Viewer is ready.
pub fn session_identity(presentation_identity: &str, device_generation: u64) -> String {
    hash_identity("session", &(presentation_identity, device_generation))
        .expect("session identity serialization cannot fail")
}

/// Stable labeled JSON hash primitive used by legacy identity material that
/// cannot live in this renderer-independent crate. Prefer the typed helpers
/// above for new call sites.
pub fn hash_identity(label: &str, value: &impl Serialize) -> Result<String, serde_json::Error> {
    let mut hash = Sha256::new();
    hash.update(label.as_bytes());
    hash.update([0]);
    hash.update(serde_json::to_vec(value)?);
    Ok(format!("{label}-{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SceneContent;
    use std::path::PathBuf;

    fn fixture_material() -> BuildIdentityMaterial {
        BuildIdentityMaterial {
            build_id: "build-1".to_owned(),
            generation: 1,
            source_sha256: "source".to_owned(),
            data_version: 3955,
            canonical_schema_version: 4,
            adapter_revision: 12,
            resource_inputs: vec![],
            assets_digest: "assets".to_owned(),
            build_options: BTreeMap::new(),
        }
    }

    fn fixture_presentation() -> PresentationSpec {
        PresentationSpec {
            backend: RenderBackend::IrisGl46,
            shaderpack: Some(PathBuf::from(r"C:\shaderpacks\fixture.zip")),
            render_settings: BTreeMap::from([("clouds".to_owned(), "fancy".to_owned())]),
        }
    }

    #[test]
    fn identity_derivation_is_pure_and_order_stable() {
        let mut material = fixture_material();
        material.resource_inputs = vec![
            ContentIdentity {
                kind: "resource_pack".to_owned(),
                name: "z-last".to_owned(),
                sha256: "resource-z".to_owned(),
            },
            ContentIdentity {
                kind: "fabric_mod".to_owned(),
                name: "a-first".to_owned(),
                sha256: "resource-a".to_owned(),
            },
        ];
        let identity = scene_build_identity(&material).unwrap();
        let mut reordered = material.clone();
        reordered.resource_inputs.reverse();
        reordered.build_id = "another-request".to_owned();
        reordered.generation = 1;
        assert_eq!(identity, scene_build_identity(&reordered).unwrap());
    }

    #[test]
    fn provisional_key_uses_references_without_reading_them() {
        let request = BuildStart {
            source_path: PathBuf::from(r"Z:\definitely-missing\fixture.litematic"),
            display_mode: "full".to_owned(),
            build_mode: RenderBuildMode::Prebuilt,
            content: SceneContent {
                fabric_mods_dir: Some(PathBuf::from(r"Z:\definitely-missing\mods")),
                resource_packs: vec![PathBuf::from(r"Z:\definitely-missing\pack.zip")],
                asset_inputs: vec![],
            },
            build_options: BTreeMap::new(),
            presentation: fixture_presentation(),
        };
        assert!(
            provisional_scene_key(&request)
                .unwrap()
                .starts_with("scene-build-pending-")
        );

        let mut realtime = request.clone();
        realtime.build_mode = RenderBuildMode::Realtime;
        assert_ne!(
            provisional_scene_key(&request).unwrap(),
            provisional_scene_key(&realtime).unwrap(),
            "build mode is frozen into the active-build correlation key"
        );
    }

    #[test]
    fn stable_identity_fixture_matches_pre_refactor_output() {
        let scene = scene_build_identity(&fixture_material()).unwrap();
        let presentation = presentation_identity_from_digest(
            &scene,
            &fixture_presentation(),
            Some("shader-a".to_owned()),
        )
        .unwrap();
        let session = session_identity(&presentation, 41);

        assert_eq!(
            scene,
            "scene-build-d3d80060ef20ed3acb32f0bdbdcab273aeaa9de6b483221c79e32e12cbdc5bfe"
        );
        assert_eq!(
            presentation,
            "presentation-f559b6d47be8805f7525ee8d2415bae06aa0d871bbee50d5711a991820d8f04c"
        );
        assert_eq!(
            session,
            "session-244fe24a3c961a8316c383606aa8c143e94fbda381fdb0e912eb0658e3965833"
        );
    }
}
