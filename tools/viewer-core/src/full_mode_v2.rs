#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fastnbt::Value as NbtValue;
use image::imageops::overlay;
use image::{DynamicImage, Rgba, RgbaImage};
use serde::Deserialize;
use serde_json::Value;
use zip::ZipArchive;

use crate::full_mode::{
    FULL_MODE_MATERIAL_CACHE_FORMAT, FullModeAlphaMode, FullModeBlockModelQuads,
    FullModeMaterialCache, FullModeMaterialSlot, FullModeModelQuad, FullModePaletteMaterial,
    FullModeStats,
};
use crate::mesh::ChunkSceneIndex;
use crate::nbt::{
    RegionBounds, bits_for_palette, palette_index_at, region_bounds, region_coord_to_storage_coord,
    region_volume,
};

const RENDER_ASSET_ROOT: &str = "third_party/render-assets";
const VANILLA_JAR_REL: &str = "vanilla/26.1.jar";
const FAITHFUL_ROOT_REL: &str = "faithful-64x-release-13";
const BLOCK_26_ROOT_REL: &str = "block-26.1";
const XK_ROOT_REL: &str = "xk-redstone-display-26.0.1";

#[derive(Debug, Clone)]
struct OwnedModelRef {
    model: String,
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Default)]
struct ResolvedModel {
    textures: HashMap<String, String>,
    elements: Vec<Value>,
}

#[derive(Debug, Clone, Default)]
struct TypedResolvedModel {
    textures: HashMap<String, String>,
    elements: Vec<ResolvedElement>,
}

#[derive(Debug, Deserialize)]
struct TypedBlockState {
    #[serde(default)]
    variants: HashMap<String, TypedModelApply>,
    #[serde(default)]
    multipart: Vec<TypedMultipartPart>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TypedModelApply {
    Single(TypedModelRef),
    Weighted(Vec<TypedModelRef>),
}

#[derive(Debug, Deserialize)]
struct TypedMultipartPart {
    #[serde(default)]
    when: Option<TypedMultipartWhen>,
    apply: TypedModelApply,
}

#[derive(Debug, Deserialize)]
struct TypedMultipartWhen {
    #[serde(default, rename = "OR")]
    or: Vec<BTreeMap<String, String>>,
    #[serde(default)]
    #[allow(dead_code)]
    #[serde(rename = "AND")]
    and: Vec<BTreeMap<String, String>>,
    #[serde(flatten)]
    properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TypedModelRef {
    model: String,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    weight: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TypedBlockModel {
    parent: Option<String>,
    #[serde(default)]
    textures: HashMap<String, TypedTextureRef>,
    #[serde(default)]
    elements: Vec<TypedModelElement>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum TypedTextureRef {
    Id(String),
    Sprite {
        sprite: String,
        #[serde(default)]
        force_translucent: bool,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct TypedModelElement {
    from: [f32; 3],
    to: [f32; 3],
    #[serde(default)]
    rotation: Option<TypedElementRotation>,
    faces: HashMap<String, TypedModelFace>,
}

#[derive(Debug, Clone, Deserialize)]
struct TypedElementRotation {
    origin: [f32; 3],
    axis: String,
    angle: f32,
    #[serde(default)]
    rescale: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct TypedModelFace {
    texture: String,
    #[serde(default)]
    uv: Option<[f32; 4]>,
    #[serde(default)]
    rotation: i32,
    #[serde(default)]
    cullface: Option<String>,
    #[serde(default)]
    tintindex: Option<i32>,
}

#[derive(Debug, Clone)]
struct ResolvedElement {
    from: [f32; 3],
    to: [f32; 3],
    rotation: Option<TypedElementRotation>,
    faces: HashMap<String, ResolvedFace>,
}

#[derive(Debug, Clone)]
struct ResolvedFace {
    texture: String,
    uv: Option<[f32; 4]>,
    rotation: i32,
    cullface: Option<String>,
    tintindex: Option<i32>,
}

#[derive(Debug, Clone)]
struct MaterialImage {
    key: String,
    image: RgbaImage,
    alpha_mode: FullModeAlphaMode,
}

#[derive(Debug, Clone, Copy)]
struct FaceSpec<'a> {
    texture_id: &'a str,
    uv: Option<[f32; 4]>,
    rotation: i32,
    tint_index: Option<i32>,
}

#[derive(Debug, Clone)]
struct TemplateQuad {
    vertices: [[f32; 3]; 4],
    material: MaterialImage,
    uv: Option<[[f32; 2]; 4]>,
    double_sided: bool,
    cullface: Option<String>,
}

#[derive(Debug, Clone)]
struct ChestDebugBCProvenance {
    sample_label: &'static str,
    facing: String,
    chest_type: String,
    template_identity: &'static str,
    part: &'static str,
    local_face: &'static str,
    world_face: String,
    face_role: &'static str,
    pair_half: &'static str,
    candidate_label: &'static str,
    local_cuboid_bounds: String,
    provenance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceFlavor {
    Vanilla,
    Faithful,
    Fallback,
    Xk,
}

pub fn build_full_mode_v2_scene_assets(
    scene: &ChunkSceneIndex,
    litematic_path: &Path,
) -> Result<(FullModeMaterialCache, RgbaImage)> {
    let mut builder = FullModeV2Builder::new()?;
    builder.build(scene, litematic_path)
}

struct FullModeV2Builder {
    vanilla: ZipArchive<File>,
    faithful_root: PathBuf,
    block_26_root: PathBuf,
    xk_root: PathBuf,
    model_cache: HashMap<String, Option<ResolvedModel>>,
    typed_model_cache: HashMap<String, Option<TypedResolvedModel>>,
    xk_typed_model_cache: HashMap<String, Option<TypedResolvedModel>>,
    json_cache: HashMap<String, Option<Value>>,
    texture_cache: HashMap<String, Option<(RgbaImage, ResourceFlavor)>>,
    chest_debug_bc_provenance: HashMap<String, ChestDebugBCProvenance>,
    font_ascii: Option<RgbaImage>,
}

impl FullModeV2Builder {
    fn new() -> Result<Self> {
        let workspace_root = locate_workspace_root()?;
        let render_asset_root = workspace_root.join(RENDER_ASSET_ROOT);
        let vanilla_jar = render_asset_root.join(VANILLA_JAR_REL);
        let faithful_root = render_asset_root.join(FAITHFUL_ROOT_REL);
        let block_26_root = render_asset_root.join(BLOCK_26_ROOT_REL);
        let xk_root = render_asset_root.join(XK_ROOT_REL);
        let file = File::open(&vanilla_jar)
            .with_context(|| format!("open vanilla jar failed: {}", vanilla_jar.display()))?;
        Ok(Self {
            vanilla: ZipArchive::new(file).context("open vanilla jar as zip failed")?,
            faithful_root,
            block_26_root,
            xk_root,
            model_cache: HashMap::new(),
            typed_model_cache: HashMap::new(),
            xk_typed_model_cache: HashMap::new(),
            json_cache: HashMap::new(),
            texture_cache: HashMap::new(),
            chest_debug_bc_provenance: HashMap::new(),
            font_ascii: None,
        })
    }

    fn build(
        &mut self,
        scene: &ChunkSceneIndex,
        litematic_path: &Path,
    ) -> Result<(FullModeMaterialCache, RgbaImage)> {
        let mut material_order = Vec::<MaterialImage>::new();
        let mut material_index = HashMap::<String, u32>::new();
        self.chest_debug_bc_provenance.clear();
        let mut palette_keys = Vec::<String>::new();
        let mut palette_materials = Vec::<FullModePaletteMaterial>::new();
        let mut palette_model_quads = Vec::<Vec<FullModeModelQuad>>::new();
        let mut vanilla_quads = 0_usize;

        for entry in scene.palette() {
            let properties = scene
                .property_pool()
                .get(entry.property_id)
                .cloned()
                .unwrap_or_default();
            let key = state_key(&entry.block_id, &properties);
            let template = self.build_state_template(&entry.block_id, &properties)?;
            let mut palette_material = FullModePaletteMaterial {
                geometry_hint: Some("v2_template".to_string()),
                ..Default::default()
            };
            palette_keys.push(key);
            vanilla_quads += template.len();
            let baked_quads = self.template_to_model_quads_with_context(
                &entry.block_id,
                &properties,
                template,
                &mut material_order,
                &mut material_index,
                Some(&mut palette_material),
            );
            if palette_material.cross.is_none() {
                palette_material.cross = palette_material
                    .down
                    .or(palette_material.up)
                    .or(palette_material.north)
                    .or(palette_material.south)
                    .or(palette_material.west)
                    .or(palette_material.east);
            }
            palette_model_quads.push(baked_quads);
            palette_materials.push(palette_material);
        }

        let block_model_quads = self.build_block_overrides(
            scene,
            litematic_path,
            &mut material_order,
            &mut material_index,
        )?;
        let (atlas, materials) = pack_materials(&material_order);
        let cache = FullModeMaterialCache {
            format: FULL_MODE_MATERIAL_CACHE_FORMAT.to_string(),
            source:
                "full_mode_v2_rust_runtime: 26.1.jar blockstate/model + Faithful + XK + block-26.1"
                    .to_string(),
            atlas_file: String::new(),
            palette_keys,
            materials,
            palette_materials,
            palette_model_quads,
            block_model_quads,
            stats: Some(FullModeStats {
                palette_entries: scene.palette().len(),
                material_slots: material_order.len(),
                baked_palette_entries: scene.palette().len(),
                fallback_palette_entries: 0,
            }),
            cache_manifest: None,
        };
        println!(
            "[LBA_FULL_MODE_V2] runtime_cache_ready palette_entries={} material_slots={} template_quads={} block_overrides={} atlas={}x{}",
            cache.palette_keys.len(),
            cache.materials.len(),
            vanilla_quads,
            cache.block_model_quads.len(),
            atlas.width(),
            atlas.height()
        );
        Ok((cache, atlas))
    }

    fn material_slot(
        &self,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
        material: MaterialImage,
    ) -> u32 {
        if let Some(slot) = material_index.get(&material.key).copied() {
            return slot;
        }
        let slot = material_order.len() as u32;
        material_index.insert(material.key.clone(), slot);
        material_order.push(material);
        slot
    }

    fn build_state_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let normalized_properties = full_mode_v2_critical_default_properties(block_id, properties);
        let properties = &normalized_properties;
        let local = local_id(block_id);
        if local == "water" {
            return self.water_template(block_id, properties);
        }
        if local == "lava" {
            return self.lava_template(block_id, properties);
        }
        if local == "moving_piston" {
            return self.moving_piston_template(properties, 0.0, None, false, true);
        }
        if local == "redstone_wire" {
            return self.typed_model_template(block_id, properties);
        }
        if full_mode_special_plant_template_local(local) {
            let quads = self.typed_model_template(block_id, properties)?;
            trace_plant_template_classification(
                block_id,
                properties,
                "special_plant_typed_model",
                quads.len(),
            );
            return Ok(quads);
        }
        if full_mode_explicit_plant_template_local(local) {
            return self.explicit_plant_template(block_id, properties);
        }
        if local.ends_with("_pane") {
            return self.glass_pane_template(block_id, properties);
        }
        if local == "chain" {
            return self.chain_typed_model_template(block_id, properties);
        }
        if local.ends_with("_banner") {
            return self.banner_template(block_id, properties);
        }
        if local.ends_with("_sign") {
            return self.sign_template(block_id, properties);
        }
        if full_mode_stripped_log_or_wood_local(local) {
            let quads = self.typed_model_template(block_id, properties)?;
            trace_stripped_log_template(block_id, properties, &quads);
            return Ok(quads);
        }
        if is_full_glass_block(local) {
            return self.typed_model_template(block_id, properties);
        }
        if matches!(
            local,
            "repeater" | "barrel" | "piston" | "sticky_piston" | "piston_head"
        ) || is_basic_rail_family(local)
        {
            return self.typed_model_template(block_id, properties);
        }
        if matches!(
            local,
            "cauldron" | "water_cauldron" | "lava_cauldron" | "powder_snow_cauldron"
        ) {
            return self.cauldron_template(block_id, properties);
        }
        if local == "composter" {
            return self.composter_template(block_id, properties);
        }
        if matches!(local, "chest" | "trapped_chest" | "ender_chest") {
            return self.chest_template(block_id, properties);
        }
        if local == "hopper" {
            return self.typed_model_template(block_id, properties);
        }
        let refs = self.select_model_refs(block_id, properties)?;
        let mut quads = Vec::new();
        for model_ref in refs {
            if let Some(model) = self.resolve_model(&model_ref.model)? {
                quads.extend(self.collect_model_quads(block_id, properties, &model, &model_ref)?);
            }
        }
        if quads.is_empty() {
            if local_id(block_id).ends_with("_sign") {
                return self.sign_template(block_id, properties);
            }
            log_skipped_unknown_block(block_id, properties, "no_model_quads");
        }
        Ok(quads)
    }

    fn sign_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let wood = local_id(block_id)
            .trim_end_matches("_wall_hanging_sign")
            .trim_end_matches("_wall_sign")
            .trim_end_matches("_hanging_sign")
            .trim_end_matches("_sign");
        let texture_id = format!("minecraft:block/{wood}_planks");
        let material = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: &texture_id,
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "sign",
            true,
        )?;
        let local = local_id(block_id);
        let is_wall_sign = full_mode_wall_sign_local(local);
        let (board_min, board_max, attachment_offset) = if is_wall_sign {
            (
                [0.125, 0.25, 0.0],
                [0.875, 0.75, 1.0 / 16.0],
                "north_surface",
            )
        } else {
            ([0.125, 0.25, 0.46875], [0.875, 0.75, 0.53125], "centered")
        };
        let mut quads = cuboid_template(material.clone(), board_min, board_max, false);
        if !full_mode_wall_attached_sign_local(local) {
            quads.extend(cuboid_template(
                material,
                [0.4375, 0.0, 0.46875],
                [0.5625, 0.25, 0.53125],
                false,
            ));
        }
        let rotation = if let Some(facing) = properties.get("facing").map(String::as_str) {
            match facing {
                "south" => 180.0,
                "west" => 90.0,
                "east" => 270.0,
                _ => 0.0,
            }
        } else {
            properties
                .get("rotation")
                .and_then(|value| value.parse::<f32>().ok())
                .unwrap_or(0.0)
                * 22.5
        };
        if rotation != 0.0 {
            for quad in &mut quads {
                for vertex in &mut quad.vertices {
                    *vertex = rotate_point(*vertex, [0.5, 0.5, 0.5], "y", rotation, false);
                }
            }
        }
        trace_wall_attached_sign_transform(
            block_id,
            properties,
            rotation,
            attachment_offset,
            board_min,
            board_max,
            &quads,
        );
        Ok(quads)
    }

    fn select_model_refs(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<OwnedModelRef>> {
        let Some(payload) = self.read_json(&blockstate_path(block_id))? else {
            return Ok(Vec::new());
        };
        let mut refs = Vec::<OwnedModelRef>::new();
        if let Some(variants) = payload.get("variants").and_then(Value::as_object) {
            let mut best_score = i32::MIN;
            let mut best_value: Option<&Value> = None;
            for (key, value) in variants {
                if let Some(score) = variant_score(key, properties)
                    && score > best_score
                {
                    best_score = score;
                    best_value = Some(value);
                }
            }
            if let Some(value) = best_value {
                refs.extend(model_refs_from_apply(value));
            }
        }
        if let Some(parts) = payload.get("multipart").and_then(Value::as_array) {
            for part in parts {
                let when = part.get("when");
                if when.is_some_and(|when| !when_matches(when, properties)) {
                    continue;
                }
                if let Some(apply) = part.get("apply") {
                    refs.extend(model_refs_from_apply(apply));
                }
            }
        }
        Ok(refs)
    }

    fn resolve_model(&mut self, model_ref: &str) -> Result<Option<ResolvedModel>> {
        let path = model_path(model_ref);
        if let Some(cached) = self.model_cache.get(&path) {
            return Ok(cached.clone());
        }
        let Some(payload) = self.read_json(&path)? else {
            self.model_cache.insert(path, None);
            return Ok(None);
        };
        let mut textures = HashMap::<String, String>::new();
        let mut elements = Vec::<Value>::new();
        if let Some(parent_ref) = payload.get("parent").and_then(Value::as_str)
            && let Some(parent) = self.resolve_model(parent_ref)?
        {
            textures.extend(parent.textures);
            elements.extend(parent.elements);
        }
        if let Some(map) = payload.get("textures").and_then(Value::as_object) {
            for (key, value) in map {
                if let Some(texture) = value.as_str() {
                    textures.insert(key.clone(), texture.to_string());
                }
            }
        }
        if let Some(items) = payload.get("elements").and_then(Value::as_array) {
            elements = items.clone();
        }
        let resolved = ResolvedModel { textures, elements };
        self.model_cache.insert(path, Some(resolved.clone()));
        Ok(Some(resolved))
    }

    fn collect_model_quads(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        model: &ResolvedModel,
        model_ref: &OwnedModelRef,
    ) -> Result<Vec<TemplateQuad>> {
        let mut quads = Vec::new();
        for element in &model.elements {
            let Some(faces) = element.get("faces").and_then(Value::as_object) else {
                continue;
            };
            for (face_name, face) in faces {
                let Some(face) = face.as_object() else {
                    continue;
                };
                let Some(texture_ref) = face.get("texture").and_then(Value::as_str) else {
                    continue;
                };
                let Some(texture_id) = resolve_texture_id(texture_ref, &model.textures) else {
                    continue;
                };
                let texture_id = if local_id(block_id) == "observer" {
                    observer_effective_texture_id(properties, face_name, &texture_id).to_string()
                } else {
                    texture_id
                };
                let uv = face.get("uv").and_then(json_f32x4);
                let rotation = face.get("rotation").and_then(Value::as_i64).unwrap_or(0) as i32;
                let world_face = rotate_direction(face_name, model_ref.x, model_ref.y);
                let mut material = self.material_for_texture(
                    block_id,
                    properties,
                    &FaceSpec {
                        texture_id: &texture_id,
                        uv,
                        rotation,
                        tint_index: face
                            .get("tintindex")
                            .and_then(Value::as_i64)
                            .map(|value| value as i32),
                    },
                    face_name,
                    true,
                )?;
                let observer_rep = observer_semantic_trace_target(properties, world_face.as_str());
                let observer_detect_rep =
                    observer_detect_resource_target(properties, face_name, world_face.as_str());
                let observer_trace_rep =
                    observer_rep.clone().or_else(|| observer_detect_rep.clone());
                let observer_family = observer_texture_family(&texture_id);
                if let (Some(rep), Some(family)) = (observer_trace_rep.as_deref(), observer_family)
                {
                    println!(
                        "[LBA_OBSERVER_FAMILY_20260423D] rep={} state={} local_face={} world_face={} texture_id={} family={} material_key={}",
                        rep,
                        state_key(block_id, properties),
                        face_name,
                        world_face,
                        texture_id,
                        family,
                        material.key,
                    );
                }
                if let (Some(rep), Some(family)) = (observer_trace_rep.as_deref(), observer_family)
                    && observer_family_probe_requested(family)
                {
                    material = observer_family_probe_material(rep, family);
                    println!(
                        "[LBA_OBSERVER_FAMILY_PROBE_20260423D] rep={} state={} local_face={} world_face={} family={} texture_id={} branch=observer_real_path_family_probe",
                        rep,
                        state_key(block_id, properties),
                        face_name,
                        world_face,
                        family,
                        texture_id,
                    );
                }
                if let Some(rep) = observer_detect_rep.as_deref() {
                    println!(
                        "[LBA_OBSERVER_DETECT_RESOURCE_A] sample={} state={} facing={} world_face={} local_face={} texture_id={} family={} material_key={} face_uv={:?} rotation={}",
                        rep,
                        state_key(block_id, properties),
                        properties.get("facing").map(String::as_str).unwrap_or("-"),
                        world_face,
                        face_name,
                        texture_id,
                        observer_family.unwrap_or("?"),
                        material.key,
                        uv,
                        rotation,
                    );
                }
                if local_id(block_id) == "observer"
                    && let Some(probe_id) =
                        observer_owner_probe_target(properties, world_face.as_str())
                {
                    material = observer_owner_probe_material(
                        &probe_id,
                        state_key(block_id, properties).as_str(),
                        face_name,
                        world_face.as_str(),
                    );
                    println!(
                        "[LBA_OBSERVER_OWNER_PROBE_20260423B] probe={} state={} local_face={} world_face={} texture_id={} branch=observer_real_path_material_override",
                        probe_id,
                        state_key(block_id, properties),
                        face_name,
                        world_face,
                        texture_id,
                    );
                }
                if let Some(family) = observer_family
                    && observer_front_probe_requested(family)
                {
                    material = observer_front_probe_material(
                        state_key(block_id, properties).as_str(),
                        face_name,
                        world_face.as_str(),
                        family,
                    );
                    println!(
                        "[LBA_OBSERVER_FRONT_PROBE_20260424A] state={} local_face={} world_face={} family={} texture_id={} material_key={}",
                        state_key(block_id, properties),
                        face_name,
                        world_face,
                        family,
                        texture_id,
                        material.key,
                    );
                }
                let Some(vertices) =
                    element_face_quad(element, face_name, model_ref, local_id(block_id))
                else {
                    continue;
                };
                let actual_world_face = dominant_face_from_vertices(&vertices);
                let final_uv = if local_id(block_id) == "observer" {
                    let base = observer_model_base_quad_uv(face_name, &texture_id, uv, rotation);
                    let final_uv = observer_real_quad_uv(face_name, &texture_id, base);
                    if std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG").is_some() {
                        println!(
                            "[LBA_OBSERVER_EXE_MARKER_20260423A] branch=observer_real_path_v2 fn=collect_model_quads helper=observer_model_base_quad_uv state={} local_face={} texture_id={} base_quad_uv={} final_quad_uv={}",
                            state_key(block_id, properties),
                            face_name,
                            texture_id,
                            format_uv2(&base),
                            format_uv2(&final_uv),
                        );
                        println!(
                            "[LBA_FULL_MODE_V2_OBSERVER_REAL] state={} local_face={} world_face={} texture_id={} face_uv={:?} material_rotation={} base_quad_uv={} final_quad_uv={}",
                            state_key(block_id, properties),
                            face_name,
                            world_face,
                            texture_id,
                            uv,
                            rotation,
                            format_uv2(&base),
                            format_uv2(&final_uv),
                        );
                    }
                    Some(final_uv)
                } else {
                    None
                };
                if let Some(rep) = observer_trace_rep.as_deref() {
                    println!(
                        "[LBA_OBSERVER_SEMANTIC_20260423C] rep={} state={} local_face={} world_face={} texture_id={} face_uv={:?} rotation={} template_quad_uv={} material_key={}",
                        rep,
                        state_key(block_id, properties),
                        face_name,
                        world_face,
                        texture_id,
                        uv,
                        rotation,
                        final_uv
                            .as_ref()
                            .map(format_uv2)
                            .unwrap_or_else(|| "None".to_string()),
                        material.key,
                    );
                }
                if let Some(rep) = observer_detect_rep.as_deref() {
                    println!(
                        "[LBA_OBSERVER_DETECT_RESOURCE_B] sample={} world_face={} local_face={} texture_id={} material_key={} template_quad_uv={}",
                        rep,
                        world_face,
                        face_name,
                        texture_id,
                        material.key,
                        final_uv
                            .as_ref()
                            .map(format_uv2)
                            .unwrap_or_else(|| "None".to_string()),
                    );
                }
                let alpha = material.alpha_mode;
                let rotated_cullface = face
                    .get("cullface")
                    .and_then(Value::as_str)
                    .map(|face| rotate_direction(face, model_ref.x, model_ref.y));
                let effective_cullface = if terrain_quad_uses_actual_world_face(
                    block_id,
                    &texture_id,
                    face.get("tintindex")
                        .and_then(Value::as_i64)
                        .map(|value| value as i32),
                ) {
                    Some(actual_world_face.to_string())
                } else {
                    rotated_cullface
                };
                trace_grass_quad(
                    block_id,
                    properties,
                    face_name,
                    actual_world_face,
                    &texture_id,
                    face.get("tintindex")
                        .and_then(Value::as_i64)
                        .map(|value| value as i32),
                    &material,
                    &vertices,
                    effective_cullface.as_deref(),
                );
                trace_terrain_stack_quad(
                    block_id,
                    properties,
                    face_name,
                    world_face.as_str(),
                    actual_world_face,
                    effective_cullface.as_deref(),
                    &texture_id,
                    &vertices,
                    &material,
                );
                trace_pane_quad(
                    block_id,
                    properties,
                    face_name,
                    world_face.as_str(),
                    actual_world_face,
                    effective_cullface.as_deref(),
                    &texture_id,
                    &vertices,
                    &material,
                );
                let double_sided = template_quad_should_be_double_sided(local_id(block_id), alpha)
                    || local_id(block_id).ends_with("_rail");
                trace_tbl_model_transform(
                    block_id,
                    properties,
                    model_ref,
                    face_name,
                    actual_world_face,
                    effective_cullface.as_deref(),
                    &texture_id,
                    &material,
                    double_sided,
                    &vertices,
                );
                trace_wall_attached_button_transform(
                    block_id,
                    properties,
                    model_ref,
                    face_name,
                    actual_world_face,
                    effective_cullface.as_deref(),
                    &vertices,
                );
                trace_transparent_glass_quad(
                    block_id,
                    properties,
                    face_name,
                    actual_world_face,
                    effective_cullface.as_deref(),
                    &texture_id,
                    &material,
                    double_sided,
                    &vertices,
                );
                quads.push(TemplateQuad {
                    vertices,
                    material,
                    uv: final_uv,
                    double_sided,
                    cullface: effective_cullface,
                });
            }
        }
        Ok(quads)
    }

    fn typed_model_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let refs = self.select_typed_model_refs(block_id, properties)?;
        if is_static_piston_family(local_id(block_id))
            && std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some()
        {
            let refs_summary = refs
                .iter()
                .map(|model_ref| format!("{}@x{}y{}", model_ref.model, model_ref.x, model_ref.y))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "[LBA_FULL_MODE_V2_PISTON] block={} state={} facing={} extended={} short={} type={} refs=[{}]",
                block_id,
                state_key(block_id, properties),
                properties
                    .get("facing")
                    .map(String::as_str)
                    .unwrap_or("north"),
                properties
                    .get("extended")
                    .map(String::as_str)
                    .unwrap_or("-"),
                properties.get("short").map(String::as_str).unwrap_or("-"),
                properties.get("type").map(String::as_str).unwrap_or("-"),
                refs_summary,
            );
        }
        let mut quads = Vec::new();
        for model_ref in refs {
            let model = if hopper_uses_xk_typed_model_base(local_id(block_id), &model_ref.model) {
                self.resolve_xk_typed_model(&model_ref.model)?
            } else {
                self.resolve_typed_model(&model_ref.model)?
            };
            if let Some(model) = model {
                let model_quads =
                    self.collect_typed_model_quads(block_id, properties, &model, &model_ref)?;
                if is_basic_rail_family(local_id(block_id))
                    && std::env::var_os("LBA_FULL_MODE_V2_RAIL_DEBUG").is_some()
                {
                    println!(
                        "[LBA_FULL_MODE_V2_RAIL] block={} state={} shape={} model={} x={} y={} applied_y={} truth={} mesh={} elements={} quads={}",
                        block_id,
                        state_key(block_id, properties),
                        properties
                            .get("shape")
                            .map(String::as_str)
                            .unwrap_or("north_south"),
                        model_ref.model,
                        model_ref.x,
                        model_ref.y,
                        model_y_rotation_angle_for(local_id(block_id), model_ref.y),
                        rail_shape_truth(properties),
                        rail_mesh_debug(&model_quads),
                        model.elements.len(),
                        model_quads.len()
                    );
                }
                if matches!(local_id(block_id), "repeater" | "hopper" | "barrel")
                    && std::env::var_os("LBA_FULL_MODE_V2_TYPED_DEBUG").is_some()
                {
                    println!(
                        "[LBA_FULL_MODE_V2_TYPED] block={} state={} model={} x={} y={} applied_y={} truth={} elements={} quads={} first={}",
                        block_id,
                        state_key(block_id, properties),
                        model_ref.model,
                        model_ref.x,
                        model_ref.y,
                        model_y_rotation_angle_for(local_id(block_id), model_ref.y),
                        typed_block_truth(block_id, properties),
                        model.elements.len(),
                        model_quads.len(),
                        typed_mesh_debug(&model_quads),
                    );
                }
                if local_id(block_id) == "redstone_wire"
                    && std::env::var_os("LBA_FULL_MODE_V2_MULTIPART_DEBUG").is_some()
                {
                    println!(
                        "[LBA_FULL_MODE_V2_MULTIPART] block={} state={} model={} x={} y={} applied_y={} truth={} quads={} first={}",
                        block_id,
                        state_key(block_id, properties),
                        model_ref.model,
                        model_ref.x,
                        model_ref.y,
                        model_y_rotation_angle_for(local_id(block_id), model_ref.y),
                        redstone_wire_truth(properties),
                        model_quads.len(),
                        typed_mesh_debug(&model_quads),
                    );
                }
                if is_static_piston_family(local_id(block_id))
                    && std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some()
                {
                    println!(
                        "[LBA_FULL_MODE_V2_PISTON_TYPED] block={} state={} model={} x={} y={} applied_x={} applied_y={} vertical_pair_branch={} pair_relation={} truth={} elements={} quads={} first={}",
                        block_id,
                        state_key(block_id, properties),
                        model_ref.model,
                        model_ref.x,
                        model_ref.y,
                        piston_model_x_rotation_angle(local_id(block_id), properties, &model_ref),
                        model_y_rotation_angle_for(local_id(block_id), model_ref.y),
                        piston_vertical_pair_orientation_branch(local_id(block_id), properties),
                        piston_pair_relation_debug(local_id(block_id), properties),
                        typed_block_truth(block_id, properties),
                        model.elements.len(),
                        model_quads.len(),
                        typed_mesh_debug(&model_quads),
                    );
                }
                quads.extend(model_quads);
            }
        }
        if quads.is_empty() {
            log_skipped_unknown_block(block_id, properties, "typed_no_model_quads");
        }
        if local_id(block_id) == "redstone_wire" {
            quads.extend(self.redstone_wire_power_overlay(block_id, properties)?);
        }
        Ok(quads)
    }

    fn select_typed_model_refs(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<OwnedModelRef>> {
        if local_id(block_id) == "hopper" {
            return Ok(hopper_typed_model_refs(properties));
        }
        let Some(payload) = self.read_typed_json::<TypedBlockState>(&blockstate_path(block_id))?
        else {
            return Ok(Vec::new());
        };
        if !payload.multipart.is_empty() {
            let mut refs = Vec::new();
            for part in &payload.multipart {
                if typed_multipart_when_matches(part.when.as_ref(), properties) {
                    refs.extend(typed_model_refs_from_apply(&part.apply));
                }
            }
            return Ok(refs);
        }
        let mut best_score = i32::MIN;
        let mut best_apply: Option<&TypedModelApply> = None;
        for (key, apply) in &payload.variants {
            if let Some(score) = variant_score(key, properties)
                && score > best_score
            {
                best_score = score;
                best_apply = Some(apply);
            }
        }
        Ok(best_apply
            .map(typed_model_refs_from_apply)
            .unwrap_or_default())
    }

    fn resolve_typed_model(&mut self, model_ref: &str) -> Result<Option<TypedResolvedModel>> {
        let path = model_path(model_ref);
        if let Some(cached) = self.typed_model_cache.get(&path) {
            return Ok(cached.clone());
        }
        let Some(payload) = self.read_typed_json::<TypedBlockModel>(&path)? else {
            self.typed_model_cache.insert(path, None);
            return Ok(None);
        };
        let mut textures = HashMap::<String, String>::new();
        let mut elements = Vec::<ResolvedElement>::new();
        if let Some(parent_ref) = payload.parent.as_deref()
            && let Some(parent) = self.resolve_typed_model(parent_ref)?
        {
            textures.extend(parent.textures);
            elements.extend(parent.elements);
        }
        textures.extend(
            payload
                .textures
                .into_iter()
                .map(|(key, value)| (key, value.texture_id())),
        );
        if !payload.elements.is_empty() {
            elements = payload
                .elements
                .into_iter()
                .map(ResolvedElement::from)
                .collect();
        }
        let resolved = TypedResolvedModel { textures, elements };
        self.typed_model_cache.insert(path, Some(resolved.clone()));
        Ok(Some(resolved))
    }

    fn resolve_xk_typed_model(&mut self, model_ref: &str) -> Result<Option<TypedResolvedModel>> {
        let path = model_path(model_ref);
        if let Some(cached) = self.xk_typed_model_cache.get(&path) {
            return Ok(cached.clone());
        }
        let full_path = self.xk_root.join(&path);
        let text = match std::fs::read_to_string(&full_path) {
            Ok(text) => text,
            Err(_) => {
                self.xk_typed_model_cache.insert(path, None);
                return Ok(None);
            }
        };
        let payload: TypedBlockModel = serde_json::from_str(&text)
            .with_context(|| format!("parse xk typed json failed: {}", full_path.display()))?;
        let mut textures = HashMap::<String, String>::new();
        let mut elements = Vec::<ResolvedElement>::new();
        if let Some(parent_ref) = payload.parent.as_deref()
            && let Some(parent) = self.resolve_xk_typed_model(parent_ref)?
        {
            textures.extend(parent.textures);
            elements.extend(parent.elements);
        }
        textures.extend(
            payload
                .textures
                .into_iter()
                .map(|(key, value)| (key, value.texture_id())),
        );
        if !payload.elements.is_empty() {
            elements = payload
                .elements
                .into_iter()
                .map(ResolvedElement::from)
                .collect();
        }
        let resolved = TypedResolvedModel { textures, elements };
        self.xk_typed_model_cache
            .insert(path, Some(resolved.clone()));
        Ok(Some(resolved))
    }

    fn collect_typed_model_quads(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        model: &TypedResolvedModel,
        model_ref: &OwnedModelRef,
    ) -> Result<Vec<TemplateQuad>> {
        let mut quads = Vec::new();
        let local = local_id(block_id);
        let xk_model = if local == "hopper" {
            self.resolve_xk_typed_model(&model_ref.model)?
        } else {
            None
        };
        for (element_index, element) in model.elements.iter().enumerate() {
            for (face_name, face) in &element.faces {
                if is_basic_rail_family(local_id(block_id)) && face_name == "down" {
                    continue;
                }
                let xk_face = xk_model
                    .as_ref()
                    .and_then(|override_model| override_model.elements.get(element_index))
                    .and_then(|override_element| override_element.faces.get(face_name));
                let use_real_material =
                    hopper_big_bottom_face(local, &model_ref.model, element_index, face_name);
                let texture_ref = hopper_texture_ref_override(
                    local,
                    &model_ref.model,
                    element_index,
                    face_name,
                    xk_face,
                    face,
                );
                let texture_textures = if use_real_material {
                    &model.textures
                } else {
                    xk_face
                        .map(|_| xk_model.as_ref().map(|model| &model.textures).unwrap())
                        .unwrap_or(&model.textures)
                };
                let Some(raw_texture_id) = resolve_texture_id(texture_ref, texture_textures) else {
                    continue;
                };
                let texture_id = if use_real_material {
                    hopper_real_material_texture_id(&raw_texture_id).to_string()
                } else {
                    raw_texture_id
                };
                let material_face = xk_face.unwrap_or(face);
                let face_uv = if local == "hopper" {
                    material_face
                        .uv
                        .or_else(|| hopper_default_face_uv(element, face_name))
                } else {
                    material_face.uv
                };
                let material = self.material_for_texture(
                    block_id,
                    properties,
                    &FaceSpec {
                        texture_id: &texture_id,
                        uv: face_uv,
                        rotation: material_face.rotation,
                        tint_index: material_face.tintindex,
                    },
                    face_name,
                    !use_real_material,
                )?;
                let geometry_face_name = if local == "barrel" && model_ref.x == 90 {
                    match face_name.as_str() {
                        "north" => "south",
                        "south" => "north",
                        other => other,
                    }
                } else {
                    face_name.as_str()
                };
                let x_rotation_angle =
                    piston_model_x_rotation_angle(local_id(block_id), properties, model_ref);
                let Some(vertices) = typed_element_face_quad(
                    element,
                    geometry_face_name,
                    model_ref,
                    local,
                    x_rotation_angle,
                ) else {
                    continue;
                };
                let alpha = material.alpha_mode;
                let final_world_face = dominant_face_from_vertices(&vertices);
                let rotated_cullface = face
                    .cullface
                    .as_deref()
                    .map(|face| rotate_direction(face, model_ref.x, model_ref.y));
                let effective_cullface = if local == "barrel" && model_ref.x == 90 {
                    None
                } else if is_static_piston_family(local) && face.cullface.is_some() {
                    Some(final_world_face.to_string())
                } else {
                    rotated_cullface.clone()
                };
                if local == "barrel" && std::env::var_os("LBA_FULL_MODE_V2_BARREL_DEBUG").is_some()
                {
                    println!(
                        "[LBA_FULL_MODE_V2_BARREL] state={} model={} x={} y={} src_face={} world_face={} cullface={:?} effective_cullface={:?} emitted=true",
                        state_key(block_id, properties),
                        model_ref.model,
                        model_ref.x,
                        model_ref.y,
                        face_name,
                        final_world_face,
                        rotated_cullface,
                        effective_cullface,
                    );
                }
                if is_static_piston_family(local)
                    && std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some()
                {
                    println!(
                        "[LBA_FULL_MODE_V2_PISTON_FACE] block_id={} state={} model={} facing={} extended={} short={} type={} vertical_pair_branch={} pair_relation={} src_face={} world_face={} rotated_cullface={:?} effective_cullface={:?}",
                        block_id,
                        state_key(block_id, properties),
                        model_ref.model,
                        properties
                            .get("facing")
                            .map(String::as_str)
                            .unwrap_or("north"),
                        properties
                            .get("extended")
                            .map(String::as_str)
                            .unwrap_or("-"),
                        properties.get("short").map(String::as_str).unwrap_or("-"),
                        properties.get("type").map(String::as_str).unwrap_or("-"),
                        piston_vertical_pair_orientation_branch(local, properties),
                        piston_pair_relation_debug(local, properties),
                        face_name,
                        final_world_face,
                        rotated_cullface,
                        effective_cullface,
                    );
                }
                let piston_head_local_side =
                    piston_body_side_head_local_side(local, properties, final_world_face);
                let auto_piston_body_uv = if is_piston_body_side_texture(local, &texture_id) {
                    auto_piston_body_side_uv(
                        face_name,
                        final_world_face,
                        &vertices,
                        piston_head_local_side,
                    )
                } else {
                    None
                };
                let uv = auto_piston_body_uv
                    .as_ref()
                    .map(|(uv, _)| *uv)
                    .unwrap_or_else(|| {
                        hopper_quad_uv_override(
                            local,
                            properties,
                            &model_ref.model,
                            element_index,
                            face_name,
                            typed_face_quad_uv(face_name),
                        )
                    });
                if local == "hopper"
                    && std::env::var_os("LBA_FULL_MODE_V2_HOPPER_DEBUG").is_some()
                    && hopper_debug_case_selected(properties)
                {
                    println!(
                        "[LBA_FULL_MODE_V2_HOPPER_FACE] state={} model={} element={} src_face={} world_face={} texture_slot={} source={} material_id={} uv_spec={:?} quad_uv={}",
                        state_key(block_id, properties),
                        model_ref.model,
                        element_index,
                        face_name,
                        final_world_face,
                        texture_id,
                        self.texture_source_trace(&texture_id, !use_real_material)?,
                        material.key,
                        face_uv,
                        format_uv2(&uv),
                    );
                }
                if let Some((_, debug)) = auto_piston_body_uv.as_ref() {
                    if std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some() {
                        println!(
                            "[LBA_FULL_MODE_V2_PISTON_UV] block_id={} state={} facing={} extended={} short={} type={} src_face={} world_face={} head_local={} final_uv={} final_normal={} local_u={} {} local_v={} {} rotation={} flip_u={} flip_v={} custom={} body_side_rotate180={}",
                            block_id,
                            state_key(block_id, properties),
                            properties
                                .get("facing")
                                .map(String::as_str)
                                .unwrap_or("north"),
                            properties
                                .get("extended")
                                .map(String::as_str)
                                .unwrap_or("-"),
                            properties.get("short").map(String::as_str).unwrap_or("-"),
                            properties.get("type").map(String::as_str).unwrap_or("-"),
                            face_name,
                            final_world_face,
                            piston_head_local_side.unwrap_or("-"),
                            format_uv2(&uv),
                            format_vec3(debug.normal),
                            dominant_axis(debug.u_dir),
                            format_vec3(debug.u_dir),
                            dominant_axis(debug.v_dir),
                            format_vec3(debug.v_dir),
                            debug.transform.rotation,
                            debug.transform.flip_u,
                            debug.transform.flip_v,
                            debug.transform.custom,
                            debug.body_side_rotate180,
                        );
                    }
                } else if is_static_piston_family(local)
                    && std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some()
                {
                    println!(
                        "[LBA_FULL_MODE_V2_PISTON_UV] block_id={} state={} facing={} extended={} short={} type={} src_face={} world_face={} final_uv={} rule=local_model_face",
                        block_id,
                        state_key(block_id, properties),
                        properties
                            .get("facing")
                            .map(String::as_str)
                            .unwrap_or("north"),
                        properties
                            .get("extended")
                            .map(String::as_str)
                            .unwrap_or("-"),
                        properties.get("short").map(String::as_str).unwrap_or("-"),
                        properties.get("type").map(String::as_str).unwrap_or("-"),
                        face_name,
                        final_world_face,
                        format_uv2(&uv),
                    );
                }
                let double_sided = template_quad_should_be_double_sided(local, alpha);
                trace_tbl_typed_transform(
                    block_id,
                    properties,
                    model_ref,
                    element_index,
                    face_name,
                    final_world_face,
                    effective_cullface.as_deref(),
                    &texture_id,
                    &material,
                    double_sided,
                    &vertices,
                );
                trace_transparent_glass_quad(
                    block_id,
                    properties,
                    face_name,
                    final_world_face,
                    effective_cullface.as_deref(),
                    &texture_id,
                    &material,
                    double_sided,
                    &vertices,
                );
                quads.push(TemplateQuad {
                    vertices,
                    material,
                    uv: Some(uv),
                    double_sided,
                    cullface: effective_cullface,
                });
            }
        }
        Ok(quads)
    }

    fn material_for_texture(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        spec: &FaceSpec<'_>,
        face_name: &str,
        allow_xk: bool,
    ) -> Result<MaterialImage> {
        let (image, key_prefix) = if allow_xk {
            if let Some(xk_id) = xk_texture_id_for_state(block_id, properties, spec.texture_id) {
                if let Some((image, _)) = self.load_texture(&xk_id)? {
                    (image, format!("xk:{xk_id}"))
                } else {
                    self.load_texture_with_alias(spec.texture_id, allow_xk)?
                }
            } else {
                self.load_texture_with_alias(spec.texture_id, allow_xk)?
            }
        } else {
            self.load_texture_with_alias(spec.texture_id, allow_xk)?
        };
        let mut image = bake_face_image(image, spec.uv, spec.rotation);
        let tint_class = classify_biome_tint(block_id, spec.texture_id, spec.tint_index);
        let key_prefix = if let Some(tint_color) = tint_class.fallback_rgb() {
            image = multiply_tint(&image, tint_color);
            format!("{key_prefix}@biome_tint={}", tint_class.label())
        } else if let Some(tinted) =
            redstone_wire_tinted_image(block_id, properties, spec.texture_id, &image)
        {
            image = tinted;
            format!("{key_prefix}@redstone_tint={}", redstone_power(properties))
        } else {
            key_prefix
        };
        let alpha_mode = detect_alpha_mode(&image);
        Ok(MaterialImage {
            key: format!(
                "{key_prefix}@uv={:?}@rot={}#{face_name}",
                spec.uv, spec.rotation
            ),
            image,
            alpha_mode,
        })
    }

    fn redstone_wire_power_overlay(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let power = redstone_power(properties);
        let texture_id = format!("minecraft:block/redstone_dust_p{power:02}");
        let material = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: &texture_id,
                uv: Some([0.0, 0.0, 16.0, 16.0]),
                rotation: 0,
                tint_index: None,
            },
            "wire_power_overlay",
            true,
        )?;
        Ok(vec![TemplateQuad {
            vertices: face_vertices(
                "up",
                [6.0 / 16.0, 0.30 / 16.0, 6.0 / 16.0],
                [10.0 / 16.0, 0.30 / 16.0, 10.0 / 16.0],
            )
            .unwrap(),
            material,
            uv: Some(typed_face_quad_uv("up")),
            double_sided: false,
            cullface: None,
        }])
    }

    fn water_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let (top_material, side_material) = self.water_materials(block_id, properties)?;
        let height = water_height(properties);
        let mut quads = Vec::new();
        quads.push(TemplateQuad {
            vertices: face_vertices("up", [0.0, 0.0, 0.0], [1.0, height, 1.0]).unwrap(),
            material: top_material.clone(),
            uv: None,
            double_sided: false,
            cullface: Some("up".to_string()),
        });
        quads.push(TemplateQuad {
            vertices: face_vertices("down", [0.0, 0.0, 0.0], [1.0, height, 1.0]).unwrap(),
            material: top_material,
            uv: None,
            double_sided: false,
            cullface: Some("down".to_string()),
        });
        for face in ["north", "south", "west", "east"] {
            quads.push(TemplateQuad {
                vertices: face_vertices(face, [0.0, 0.0, 0.0], [1.0, height, 1.0]).unwrap(),
                material: side_material.clone(),
                uv: None,
                double_sided: false,
                cullface: Some(face.to_string()),
            });
        }
        Ok(quads)
    }

    fn lava_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let (top_material, side_material) = self.lava_materials(block_id, properties)?;
        let height = water_height(properties);
        let mut quads = Vec::new();
        quads.push(TemplateQuad {
            vertices: face_vertices("up", [0.0, 0.0, 0.0], [1.0, height, 1.0]).unwrap(),
            material: top_material.clone(),
            uv: None,
            double_sided: false,
            cullface: Some("up".to_string()),
        });
        quads.push(TemplateQuad {
            vertices: face_vertices("down", [0.0, 0.0, 0.0], [1.0, height, 1.0]).unwrap(),
            material: top_material,
            uv: None,
            double_sided: false,
            cullface: Some("down".to_string()),
        });
        for face in ["north", "south", "west", "east"] {
            quads.push(TemplateQuad {
                vertices: face_vertices(face, [0.0, 0.0, 0.0], [1.0, height, 1.0]).unwrap(),
                material: side_material.clone(),
                uv: None,
                double_sided: false,
                cullface: Some(face.to_string()),
            });
        }
        Ok(quads)
    }

    fn glass_pane_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let quads = self.typed_model_template(block_id, properties)?;
        trace_pane_vanilla_template(block_id, properties, &quads);
        Ok(quads)
    }

    fn explicit_plant_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let texture_id = explicit_plant_texture_id(block_id, properties);
        let material = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: &texture_id,
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "explicit_plant",
            true,
        )?;
        let quads = crossed_plane_template(material);
        trace_plant_template_classification(
            block_id,
            properties,
            "crossed_planes_plant",
            quads.len(),
        );
        trace_basic_template(
            "explicit_plant_template",
            block_id,
            properties,
            quads.len(),
            "minimal explicit plant geometry for preserve-neighbor participation",
        );
        Ok(quads)
    }

    fn chain_typed_model_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let model_ref = chain_model_ref(properties);
        if let Some(model) = self.resolve_typed_model(&model_ref.model)? {
            let element_count = model.elements.len();
            let quads = self.collect_typed_model_quads(block_id, properties, &model, &model_ref)?;
            trace_chain_banner_chain_template(
                block_id,
                properties,
                "vanilla_typed_model",
                &model_ref,
                element_count,
                &quads,
            );
            if !quads.is_empty() {
                return Ok(quads);
            }
        }

        let quads = self.chain_fallback_template(block_id, properties, &model_ref)?;
        trace_chain_banner_chain_template(
            block_id,
            properties,
            "fallback_template_chain",
            &model_ref,
            2,
            &quads,
        );
        Ok(quads)
    }

    fn chain_fallback_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        model_ref: &OwnedModelRef,
    ) -> Result<Vec<TemplateQuad>> {
        let material = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: "minecraft:block/iron_chain",
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "chain_fallback",
            true,
        )?;
        let model = fallback_chain_resolved_model();
        Ok(self.collect_typed_model_quads(block_id, properties, &model, model_ref)?).map(
            |mut quads| {
                for quad in &mut quads {
                    quad.material = material.clone();
                }
                quads
            },
        )
    }

    fn banner_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let color = banner_color_local(local_id(block_id)).unwrap_or("white");
        let texture_id = format!("minecraft:block/{color}_wool");
        let material = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: &texture_id,
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "banner_base",
            true,
        )?;
        let quads = if local_id(block_id).ends_with("_wall_banner") {
            let facing = properties
                .get("facing")
                .map(String::as_str)
                .unwrap_or("north");
            wall_banner_quads(material, facing)
        } else {
            let rotation = properties
                .get("rotation")
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            standing_banner_quads(material, rotation)
        };
        trace_basic_template(
            "banner_template",
            block_id,
            properties,
            quads.len(),
            "base geometry only; pattern/NBT not applied",
        );
        trace_chain_banner_banner_template(block_id, properties, &quads);
        Ok(quads)
    }

    fn water_materials(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<(MaterialImage, MaterialImage)> {
        let mut top_image = self
            .load_texture("minecraft:block/water_still")?
            .map(|(image, _)| image)
            .unwrap_or_else(|| RgbaImage::from_pixel(16, 16, Rgba([52, 118, 255, 150])));
        if matches!(detect_alpha_mode(&top_image), FullModeAlphaMode::Opaque) {
            for pixel in top_image.pixels_mut() {
                pixel.0[3] = 150;
            }
        }
        let top_material = MaterialImage {
            key: format!(
                "water_top:{}@water_tint={}@textured_single_sided",
                state_key(block_id, properties),
                format_rgb(DEFAULT_WATER_TINT)
            ),
            image: multiply_tint(&crop_first_animation_frame(top_image), DEFAULT_WATER_TINT),
            alpha_mode: FullModeAlphaMode::Translucent,
        };
        let mut side_image = self
            .load_texture("minecraft:block/water_flow")?
            .map(|(image, _)| image)
            .unwrap_or_else(|| RgbaImage::from_pixel(16, 16, Rgba([52, 118, 255, 150])));
        if matches!(detect_alpha_mode(&side_image), FullModeAlphaMode::Opaque) {
            for pixel in side_image.pixels_mut() {
                pixel.0[3] = 150;
            }
        }
        let side_material = MaterialImage {
            key: format!(
                "water_side:{}@water_tint={}@textured_single_sided",
                state_key(block_id, properties),
                format_rgb(DEFAULT_WATER_TINT)
            ),
            image: multiply_tint(&crop_first_animation_frame(side_image), DEFAULT_WATER_TINT),
            alpha_mode: FullModeAlphaMode::Translucent,
        };
        Ok((top_material, side_material))
    }

    fn lava_materials(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<(MaterialImage, MaterialImage)> {
        let top_image = self
            .load_texture("minecraft:block/lava_still")?
            .map(|(image, _)| image)
            .unwrap_or_else(|| RgbaImage::from_pixel(16, 16, Rgba([255, 96, 0, 255])));
        let top_material = MaterialImage {
            key: format!(
                "lava_top:{}@textured_single_sided",
                state_key(block_id, properties)
            ),
            image: crop_first_animation_frame(top_image),
            alpha_mode: FullModeAlphaMode::Opaque,
        };
        let side_image = self
            .load_texture("minecraft:block/lava_flow")?
            .map(|(image, _)| image)
            .unwrap_or_else(|| RgbaImage::from_pixel(16, 16, Rgba([255, 96, 0, 255])));
        let side_material = MaterialImage {
            key: format!(
                "lava_side:{}@textured_single_sided",
                state_key(block_id, properties)
            ),
            image: crop_first_animation_frame(side_image),
            alpha_mode: FullModeAlphaMode::Opaque,
        };
        Ok((top_material, side_material))
    }

    fn cauldron_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let side = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: "minecraft:block/cauldron_side",
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "cauldron_side",
            true,
        )?;
        let top = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: "minecraft:block/cauldron_top",
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "cauldron_top",
            true,
        )?;
        let bottom = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: "minecraft:block/cauldron_bottom",
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "cauldron_bottom",
            true,
        )?;
        let inner = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: "minecraft:block/cauldron_inner",
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "cauldron_inner",
            true,
        )?;
        let wall = 0.125;
        let floor_y = 0.25;
        let mut quads = Vec::new();
        quads.extend(cuboid_template(
            side.clone(),
            [0.0, 0.0, 0.0],
            [1.0, floor_y, 1.0],
            false,
        ));
        quads.extend(cuboid_template(
            side.clone(),
            [0.0, floor_y, 0.0],
            [1.0, 1.0, wall],
            false,
        ));
        quads.extend(cuboid_template(
            side.clone(),
            [0.0, floor_y, 1.0 - wall],
            [1.0, 1.0, 1.0],
            false,
        ));
        quads.extend(cuboid_template(
            side.clone(),
            [0.0, floor_y, wall],
            [wall, 1.0, 1.0 - wall],
            false,
        ));
        quads.extend(cuboid_template(
            side,
            [1.0 - wall, floor_y, wall],
            [1.0, 1.0, 1.0 - wall],
            false,
        ));
        quads.push(TemplateQuad {
            vertices: face_vertices("down", [0.0, 0.0, 0.0], [1.0, 0.001, 1.0]).unwrap(),
            material: bottom,
            uv: None,
            double_sided: false,
            cullface: Some("down".to_string()),
        });
        quads.push(TemplateQuad {
            vertices: face_vertices(
                "up",
                [wall, floor_y, wall],
                [1.0 - wall, floor_y, 1.0 - wall],
            )
            .unwrap(),
            material: inner.clone(),
            uv: None,
            double_sided: false,
            cullface: None,
        });
        for (face, min, max) in [
            ("north", [wall, floor_y, wall], [1.0 - wall, 1.0, wall]),
            (
                "south",
                [wall, floor_y, 1.0 - wall],
                [1.0 - wall, 1.0, 1.0 - wall],
            ),
            ("west", [wall, floor_y, wall], [wall, 1.0, 1.0 - wall]),
            (
                "east",
                [1.0 - wall, floor_y, wall],
                [1.0 - wall, 1.0, 1.0 - wall],
            ),
        ] {
            quads.push(TemplateQuad {
                vertices: face_vertices(face, min, max).unwrap(),
                material: inner.clone(),
                uv: None,
                double_sided: false,
                cullface: None,
            });
        }
        quads.push(TemplateQuad {
            vertices: face_vertices("up", [0.0, 1.0 - 0.001, 0.0], [1.0, 1.0, 1.0]).unwrap(),
            material: top,
            uv: None,
            double_sided: false,
            cullface: None,
        });
        match local_id(block_id) {
            "water_cauldron" => {
                let level = properties
                    .get("level")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(1)
                    .clamp(1, 3);
                let fluid_y = [0.3125, 0.5, 0.6875][(level - 1) as usize];
                let fluid = self.material_for_texture(
                    block_id,
                    properties,
                    &FaceSpec {
                        texture_id: "minecraft:block/water_still",
                        uv: None,
                        rotation: 0,
                        tint_index: None,
                    },
                    "cauldron_water",
                    true,
                )?;
                quads.push(TemplateQuad {
                    vertices: face_vertices(
                        "up",
                        [wall + 0.01, fluid_y, wall + 0.01],
                        [1.0 - wall - 0.01, fluid_y, 1.0 - wall - 0.01],
                    )
                    .unwrap(),
                    material: fluid,
                    uv: None,
                    double_sided: false,
                    cullface: None,
                });
            }
            "lava_cauldron" => {
                let fluid = self.material_for_texture(
                    block_id,
                    properties,
                    &FaceSpec {
                        texture_id: "minecraft:block/lava_still",
                        uv: None,
                        rotation: 0,
                        tint_index: None,
                    },
                    "cauldron_lava",
                    true,
                )?;
                quads.push(TemplateQuad {
                    vertices: face_vertices(
                        "up",
                        [wall + 0.01, 0.6875, wall + 0.01],
                        [1.0 - wall - 0.01, 0.6875, 1.0 - wall - 0.01],
                    )
                    .unwrap(),
                    material: fluid,
                    uv: None,
                    double_sided: false,
                    cullface: None,
                });
            }
            "powder_snow_cauldron" => {
                let level = properties
                    .get("level")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(1)
                    .clamp(1, 3);
                let fill_y = [0.3125, 0.5, 0.6875][(level - 1) as usize];
                let snow = self.material_for_texture(
                    block_id,
                    properties,
                    &FaceSpec {
                        texture_id: "minecraft:block/powder_snow",
                        uv: None,
                        rotation: 0,
                        tint_index: None,
                    },
                    "cauldron_powder_snow",
                    true,
                )?;
                quads.push(TemplateQuad {
                    vertices: face_vertices(
                        "up",
                        [wall + 0.01, fill_y, wall + 0.01],
                        [1.0 - wall - 0.01, fill_y, 1.0 - wall - 0.01],
                    )
                    .unwrap(),
                    material: snow,
                    uv: None,
                    double_sided: false,
                    cullface: None,
                });
            }
            _ => {}
        }
        Ok(quads)
    }

    fn composter_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let side = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: "minecraft:block/composter_side",
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "composter_side",
            true,
        )?;
        let top = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: "minecraft:block/composter_top",
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "composter_top",
            true,
        )?;
        let bottom = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: "minecraft:block/composter_bottom",
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "composter_bottom",
            true,
        )?;
        let fill_texture = if properties
            .get("level")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0)
            >= 8
        {
            "minecraft:block/composter_ready"
        } else {
            "minecraft:block/composter_compost"
        };
        let fill = self.material_for_texture(
            block_id,
            properties,
            &FaceSpec {
                texture_id: fill_texture,
                uv: None,
                rotation: 0,
                tint_index: None,
            },
            "composter_fill",
            true,
        )?;
        let wall = 0.125;
        let floor_y = 0.125;
        let mut quads = Vec::new();
        quads.extend(cuboid_template(
            side.clone(),
            [0.0, 0.0, 0.0],
            [1.0, floor_y, 1.0],
            false,
        ));
        quads.extend(cuboid_template(
            side.clone(),
            [0.0, floor_y, 0.0],
            [1.0, 1.0, wall],
            false,
        ));
        quads.extend(cuboid_template(
            side.clone(),
            [0.0, floor_y, 1.0 - wall],
            [1.0, 1.0, 1.0],
            false,
        ));
        quads.extend(cuboid_template(
            side.clone(),
            [0.0, floor_y, wall],
            [wall, 1.0, 1.0 - wall],
            false,
        ));
        quads.extend(cuboid_template(
            side,
            [1.0 - wall, floor_y, wall],
            [1.0, 1.0, 1.0 - wall],
            false,
        ));
        quads.push(TemplateQuad {
            vertices: face_vertices("down", [0.0, 0.0, 0.0], [1.0, 0.001, 1.0]).unwrap(),
            material: bottom,
            uv: None,
            double_sided: false,
            cullface: Some("down".to_string()),
        });
        quads.push(TemplateQuad {
            vertices: face_vertices("up", [0.0, 1.0 - 0.001, 0.0], [1.0, 1.0, 1.0]).unwrap(),
            material: top.clone(),
            uv: None,
            double_sided: false,
            cullface: None,
        });
        let level = properties
            .get("level")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0)
            .clamp(0, 8);
        if level > 0 {
            let fill_y = 0.1875 + ((level - 1) as f32 * 0.09375);
            quads.push(TemplateQuad {
                vertices: face_vertices(
                    "up",
                    [wall + 0.02, fill_y, wall + 0.02],
                    [1.0 - wall - 0.02, fill_y, 1.0 - wall - 0.02],
                )
                .unwrap(),
                material: fill,
                uv: None,
                double_sided: false,
                cullface: None,
            });
        }
        Ok(quads)
    }

    fn chest_template(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let chest_type = properties
            .get("type")
            .map(String::as_str)
            .unwrap_or("single");
        let canonical_identity = chest_canonical_identity(chest_type);
        let facing = properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north");
        let template = chest_template_spec(canonical_identity);
        let mut base_quads = self.chest_textured_cuboid(
            block_id,
            properties,
            "base",
            template.base_min,
            template.base_max,
            template.base_size,
            [0.0, 19.0],
        )?;
        let mut lid_quads = self.chest_textured_cuboid(
            block_id,
            properties,
            "lid",
            template.lid_min,
            template.lid_max,
            template.lid_size,
            [0.0, 0.0],
        )?;
        let mut lock_quads = self.chest_textured_cuboid(
            block_id,
            properties,
            "lock",
            template.lock_min,
            template.lock_max,
            template.lock_size,
            [0.0, 0.0],
        )?;
        let rotation = chest_rotation_degrees(facing);
        log_chest_transform_debug(
            block_id,
            chest_type,
            canonical_identity.label(),
            facing,
            rotation,
        );
        let debug_records =
            self.chest_debug_bc_records(block_id, properties, canonical_identity, template);
        if rotation != 0.0 {
            for quad in &mut base_quads {
                for vertex in &mut quad.vertices {
                    *vertex = rotate_point(*vertex, [0.5, 0.5, 0.5], "y", rotation, false);
                }
            }
            for quad in &mut lid_quads {
                for vertex in &mut quad.vertices {
                    *vertex = rotate_point(*vertex, [0.5, 0.5, 0.5], "y", rotation, false);
                }
            }
            for quad in &mut lock_quads {
                for vertex in &mut quad.vertices {
                    *vertex = rotate_point(*vertex, [0.5, 0.5, 0.5], "y", rotation, false);
                }
            }
        }
        self.store_chest_debug_bc_records(
            block_id,
            properties,
            &base_quads,
            &lid_quads,
            &debug_records,
        );
        let mut quads = base_quads;
        quads.extend(lid_quads);
        quads.extend(lock_quads);
        Ok(quads)
    }

    fn chest_textured_cuboid(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        part: &str,
        min: [f32; 3],
        max: [f32; 3],
        size: [f32; 3],
        uv_origin: [f32; 2],
    ) -> Result<Vec<TemplateQuad>> {
        let [dx, dy, dz] = size;
        let [u, v] = uv_origin;
        let specs = ["west", "north", "east", "south", "up", "down"];
        let mut quads = Vec::new();
        for face in specs {
            let semantic_face = chest_open_semantic_face(part, face);
            let uv = chest_cuboid_face_uv(u, v, dx, dy, dz, semantic_face);
            let texture_id = chest_texture_id_for_owner_semantic_swap(
                block_id,
                properties,
                part,
                face,
                semantic_face,
            );
            let material = self.chest_face_material(
                block_id,
                properties,
                &texture_id,
                uv,
                &format!("{part}_{face}@semantic={semantic_face}"),
            )?;
            let vertices = face_vertices(face, min, max).unwrap();
            let quad_uv = typed_face_quad_uv(face);
            if let Some((world_face, face_role, pair_half, candidate_label)) =
                chest_debug_template_candidate(properties, part, face)
            {
                println!(
                    "[LBA_FULL_MODE_V2_CHEST_TEMPLATE] state={} part={} local_face={} semantic_face={} world_face={} face_role={} pair_half={} candidate={} texture_id={} quad_uv={} vertices={:?}",
                    state_key(block_id, properties),
                    part,
                    face,
                    semantic_face,
                    world_face,
                    face_role,
                    pair_half,
                    candidate_label,
                    texture_id,
                    format_uv2(&quad_uv),
                    vertices,
                );
            }
            quads.push(TemplateQuad {
                vertices,
                material,
                uv: Some(quad_uv),
                double_sided: false,
                cullface: None,
            });
        }
        Ok(quads)
    }

    fn chest_face_material(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        texture_id: &str,
        uv: [f32; 4],
        face_name: &str,
    ) -> Result<MaterialImage> {
        let debug_trace = chest_debug_trace_target(block_id, properties, face_name);
        let texture_source = if debug_trace {
            Some(self.texture_source_trace(texture_id, true)?)
        } else {
            None
        };
        let (image, key_prefix) = self.load_texture_with_alias(texture_id, true)?;
        let image = crop_texture_pixel_region(image, uv, [64.0, 64.0]);
        let alpha_mode = detect_alpha_mode(&image);
        if debug_trace {
            println!(
                "[LBA_FULL_MODE_V2_CHEST_MATERIAL] state={} face_name={} local_face={} world_face={} semantic_face={} texture_id={} texture_source={} uv={:?} crop={}x{} fingerprint={}",
                state_key(block_id, properties),
                face_name,
                chest_debug_local_face(face_name),
                chest_debug_world_face(properties, face_name),
                chest_debug_semantic_face(face_name),
                texture_id,
                texture_source.unwrap_or_else(|| "missing".to_string()),
                uv,
                image.width(),
                image.height(),
                image_fingerprint(&image),
            );
        }
        Ok(MaterialImage {
            key: format!("{key_prefix}@chest_uv={uv:?}#{face_name}"),
            image,
            alpha_mode,
        })
    }

    fn moving_piston_template(
        &mut self,
        properties: &BTreeMap<String, String>,
        progress: f32,
        moved_state: Option<(String, BTreeMap<String, String>)>,
        source: bool,
        extending: bool,
    ) -> Result<Vec<TemplateQuad>> {
        let facing = properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north");
        let displacement = if extending { progress } else { 1.0 - progress };
        let offset = facing_offset(facing, displacement);
        let mut quads = if let Some((moved_block_id, moved_properties)) = moved_state {
            let mut moved_quads = self.build_state_template(&moved_block_id, &moved_properties)?;
            translate_template_quads(&mut moved_quads, offset);
            if source && matches!(local_id(&moved_block_id), "piston" | "sticky_piston") {
                let mut head_props = BTreeMap::new();
                head_props.insert("facing".to_string(), facing.to_string());
                head_props.insert("short".to_string(), "true".to_string());
                head_props.insert(
                    "type".to_string(),
                    if local_id(&moved_block_id) == "sticky_piston" {
                        "sticky"
                    } else {
                        "normal"
                    }
                    .to_string(),
                );
                let mut head_quads =
                    self.build_state_template("minecraft:piston_head", &head_props)?;
                translate_template_quads(
                    &mut head_quads,
                    facing_offset(facing, 1.0 + displacement),
                );
                moved_quads.extend(head_quads);
            }
            moved_quads
        } else {
            let mut head_props = BTreeMap::new();
            head_props.insert("facing".to_string(), facing.to_string());
            head_props.insert("short".to_string(), "true".to_string());
            head_props.insert(
                "type".to_string(),
                properties
                    .get("type")
                    .cloned()
                    .unwrap_or_else(|| "normal".to_string()),
            );
            let mut head_quads = self.build_state_template("minecraft:piston_head", &head_props)?;
            translate_template_quads(&mut head_quads, offset);
            head_quads
        };
        if quads.is_empty() {
            let material = self.material_for_texture(
                "minecraft:moving_piston",
                properties,
                &FaceSpec {
                    texture_id: "minecraft:block/piston_side",
                    uv: None,
                    rotation: 0,
                    tint_index: None,
                },
                "north",
                true,
            )?;
            quads = cuboid_template(material, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], false);
            translate_template_quads(&mut quads, offset);
        }
        Ok(quads)
    }

    fn build_block_overrides(
        &mut self,
        _scene: &ChunkSceneIndex,
        litematic_path: &Path,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let root = crate::nbt::load_litematic_root(litematic_path)?;
        let Some(bounds) = scene_bounds(&root) else {
            return Ok(Vec::new());
        };
        let mut overrides = Vec::new();
        let mut palette_by_pos =
            HashMap::<(i32, i32, i32), (String, BTreeMap<String, String>)>::new();
        for region in root.regions.values() {
            fill_region_palette_lookup(region, bounds, &mut palette_by_pos)?;
        }
        overrides.extend(self.water_block_overrides(
            &palette_by_pos,
            material_order,
            material_index,
        )?);
        overrides.extend(self.lava_block_overrides(
            &palette_by_pos,
            material_order,
            material_index,
        )?);
        overrides.extend(self.waterlogged_block_overrides(
            &palette_by_pos,
            material_order,
            material_index,
        )?);
        if std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG").is_some() {
            overrides.extend(self.observer_debug_overlays(
                &palette_by_pos,
                material_order,
                material_index,
            )?);
        }
        if std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_some() {
            overrides.extend(self.chest_debug_overlays(
                &palette_by_pos,
                material_order,
                material_index,
            )?);
        }
        if std::env::var_os("LBA_FULL_MODE_V2_HOPPER_DEBUG").is_some() {
            overrides.extend(self.hopper_debug_overlays(
                &palette_by_pos,
                material_order,
                material_index,
            )?);
        }
        if std::env::var_os("LBA_FULL_MODE_V2_CHAIN_BANNER_DEBUG").is_some() {
            overrides.extend(self.chain_banner_debug_overlays(
                &palette_by_pos,
                material_order,
                material_index,
            )?);
        }
        if std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some() {
            overrides.extend(self.piston_debug_overlays(
                &palette_by_pos,
                material_order,
                material_index,
            )?);
        }
        if std::env::var_os("LBA_FULL_MODE_V2_TBL_DEBUG").is_some() {
            overrides.extend(self.trapdoor_banner_lever_debug_overlays(
                &palette_by_pos,
                material_order,
                material_index,
            )?);
        }
        overrides.extend(self.piston_standalone_vertical_base_overrides(
            &palette_by_pos,
            material_order,
            material_index,
        )?);
        overrides.extend(self.piston_short_head_pair_overrides(
            &palette_by_pos,
            material_order,
            material_index,
        )?);
        for region in root.regions.values() {
            for tile in &region.tile_entities {
                let Some(((x, y, z), (block_id, properties))) =
                    tile_block_state(tile, region, bounds, &palette_by_pos)
                else {
                    continue;
                };
                let local = local_id(&block_id);
                if local == "moving_piston" {
                    let progress = nbt_float_field(tile, &["Progress", "progress"])
                        .unwrap_or(0.0)
                        .clamp(0.0, 1.0);
                    let moved_state = nbt_block_state(
                        tile,
                        &["blockState", "block_state", "movedState", "MovedState"],
                    );
                    let source = nbt_bool_field(tile, &["source", "Source"]).unwrap_or(false);
                    let extending =
                        nbt_bool_field(tile, &["extending", "Extending"]).unwrap_or(true);
                    let template = self.moving_piston_template(
                        &properties,
                        progress,
                        moved_state,
                        source,
                        extending,
                    )?;
                    overrides.push(FullModeBlockModelQuads {
                        x,
                        y,
                        z,
                        replace: true,
                        quads: self.template_to_model_quads(
                            template,
                            material_order,
                            material_index,
                        ),
                    });
                } else if local.ends_with("_sign") {
                    let text_faces = sign_text_faces(tile);
                    if text_faces.front.iter().all(|line| line.is_empty())
                        && text_faces.back.iter().all(|line| line.is_empty())
                    {
                        continue;
                    }
                    let quads = sign_text_quads(self, &block_id, &properties, &text_faces)?;
                    overrides.push(FullModeBlockModelQuads {
                        x,
                        y,
                        z,
                        replace: false,
                        quads: self.template_to_model_quads(quads, material_order, material_index),
                    });
                }
            }
        }
        Ok(overrides)
    }

    fn water_block_overrides(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut waters = palette_by_pos
            .iter()
            .filter(|(_, (block_id, _))| local_id(block_id) == "water")
            .map(|(pos, state)| (*pos, state.clone()))
            .collect::<Vec<_>>();
        waters.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        let mut overrides = Vec::with_capacity(waters.len());
        for ((x, y, z), (block_id, properties)) in waters {
            let template =
                self.water_template_at((x, y, z), &block_id, &properties, palette_by_pos)?;
            overrides.push(FullModeBlockModelQuads {
                x,
                y,
                z,
                replace: true,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(overrides)
    }

    fn lava_block_overrides(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut lavas = palette_by_pos
            .iter()
            .filter(|(_, (block_id, _))| local_id(block_id) == "lava")
            .map(|(pos, state)| (*pos, state.clone()))
            .collect::<Vec<_>>();
        lavas.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        let mut overrides = Vec::with_capacity(lavas.len());
        for ((x, y, z), (block_id, properties)) in lavas {
            let template =
                self.lava_template_at((x, y, z), &block_id, &properties, palette_by_pos)?;
            overrides.push(FullModeBlockModelQuads {
                x,
                y,
                z,
                replace: true,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(overrides)
    }

    fn waterlogged_block_overrides(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut waterlogged = palette_by_pos
            .iter()
            .filter(|(_, (block_id, properties))| {
                local_id(block_id) != "water"
                    && properties.get("waterlogged").map(String::as_str) == Some("true")
            })
            .map(|(pos, state)| (*pos, state.clone()))
            .collect::<Vec<_>>();
        waterlogged.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        let water_props = BTreeMap::from([("level".to_string(), "0".to_string())]);
        let mut overlays = Vec::with_capacity(waterlogged.len());
        for ((x, y, z), (block_id, properties)) in waterlogged {
            let template =
                self.water_template_at((x, y, z), "minecraft:water", &water_props, palette_by_pos)?;
            trace_waterlogged_overlay(
                (x, y, z),
                &block_id,
                &properties,
                template.len(),
                !template.is_empty(),
            );
            if template.is_empty() {
                continue;
            }
            overlays.push(FullModeBlockModelQuads {
                x,
                y,
                z,
                replace: false,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(overlays)
    }

    fn observer_debug_overlays(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut observers = palette_by_pos
            .iter()
            .filter(|(_, (block_id, _))| local_id(block_id) == "observer")
            .map(|(pos, state)| (*pos, state.clone()))
            .collect::<Vec<_>>();
        observers.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        let mut overlays = Vec::with_capacity(observers.len());
        for (index, ((x, y, z), (block_id, properties))) in observers.iter().enumerate() {
            let label = format!("{:03}", index + 1);
            let template = self.observer_debug_label_quads(&label, block_id, properties)?;
            println!(
                "[LBA_FULL_MODE_V2_OBSERVER_VIS] id={} pos=({}, {}, {}) block_id={} state={} facing={} powered={} face_map=1:up,2:down,3:north,4:south,5:west,6:east",
                label,
                x,
                y,
                z,
                block_id,
                state_key(block_id, properties),
                properties.get("facing").map(String::as_str).unwrap_or("-"),
                properties.get("powered").map(String::as_str).unwrap_or("-"),
            );
            for (face_index, world_face) in OBSERVER_DEBUG_FACE_ORDER.iter().enumerate() {
                println!(
                    "[LBA_FULL_MODE_V2_OBSERVER_VIS_FACE] id={}-{} block_id={} state={} world_face={}",
                    label,
                    face_index + 1,
                    block_id,
                    state_key(block_id, properties),
                    world_face,
                );
            }
            overlays.push(FullModeBlockModelQuads {
                x: *x,
                y: *y,
                z: *z,
                replace: false,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(overlays)
    }

    fn chain_banner_debug_overlays(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut targets = palette_by_pos
            .iter()
            .filter(|(_, (block_id, _))| chain_banner_debug_target(local_id(block_id)))
            .map(|(pos, state)| (*pos, state.clone()))
            .collect::<Vec<_>>();
        targets.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        let mut overlays = Vec::with_capacity(targets.len());
        for (index, ((x, y, z), (block_id, properties))) in targets.iter().enumerate() {
            let label = format!("CB{:02}", index + 1);
            let template = self.chain_banner_debug_label_quads(&label, block_id, properties)?;
            println!(
                "[LBA_FULL_MODE_V2_CHAIN_BANNER_VIS] id={} pos=({}, {}, {}) block_id={} state={} note={}",
                label,
                x,
                y,
                z,
                block_id,
                state_key(block_id, properties),
                chain_banner_debug_note(block_id, properties),
            );
            overlays.push(FullModeBlockModelQuads {
                x: *x,
                y: *y,
                z: *z,
                replace: false,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(overlays)
    }

    fn piston_debug_overlays(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut pistons = palette_by_pos
            .iter()
            .filter(|(_, (block_id, _))| is_static_piston_family(local_id(block_id)))
            .map(|(pos, state)| (*pos, state.clone()))
            .collect::<Vec<_>>();
        pistons.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        let mut overlays = Vec::with_capacity(pistons.len());
        for (index, ((x, y, z), (block_id, properties))) in pistons.iter().enumerate() {
            let label = format!("P{:02}", index + 1);
            let template = self.piston_debug_label_quads(&label, block_id, properties)?;
            println!(
                "[LBA_FULL_MODE_V2_PISTON_VIS] id={} pos=({}, {}, {}) block_id={} state={} facing={} extended={} short={} type={} face_map=1:down,2:up,3:north,4:south,5:west,6:east",
                label,
                x,
                y,
                z,
                block_id,
                state_key(block_id, properties),
                properties.get("facing").map(String::as_str).unwrap_or("-"),
                properties
                    .get("extended")
                    .map(String::as_str)
                    .unwrap_or("-"),
                properties.get("short").map(String::as_str).unwrap_or("-"),
                properties.get("type").map(String::as_str).unwrap_or("-"),
            );
            for (face_index, world_face) in PISTON_DEBUG_FACE_ORDER.iter().enumerate() {
                println!(
                    "[LBA_FULL_MODE_V2_PISTON_VIS_FACE] id={}-{} block_id={} state={} world_face={}",
                    label,
                    face_index + 1,
                    block_id,
                    state_key(block_id, properties),
                    world_face,
                );
            }
            overlays.push(FullModeBlockModelQuads {
                x: *x,
                y: *y,
                z: *z,
                replace: false,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(overlays)
    }

    fn trapdoor_banner_lever_debug_overlays(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut targets = palette_by_pos
            .iter()
            .filter_map(|(pos, state)| {
                tbl_debug_family_prefix(local_id(&state.0))
                    .map(|prefix| (*pos, state.clone(), prefix))
            })
            .collect::<Vec<_>>();
        targets.sort_by_key(|((x, y, z), _, prefix)| (*prefix, *z, *y, *x));

        let mut overlays = Vec::with_capacity(targets.len());
        let mut counts = BTreeMap::<&'static str, usize>::new();
        for ((x, y, z), (block_id, properties), prefix) in targets {
            let next = counts.entry(prefix).or_insert(0);
            *next += 1;
            let label = format!("{prefix}{:02}", *next);
            let template = self.tbl_debug_label_quads(&label, &block_id, &properties)?;
            println!(
                "[LBA_FULL_MODE_V2_TBL_VIS] id={} pos=({}, {}, {}) block_id={} state={} family={} note={} overlay=renderer_side default_off=true env=LBA_FULL_MODE_V2_TBL_DEBUG",
                label,
                x,
                y,
                z,
                block_id,
                state_key(&block_id, &properties),
                tbl_debug_family_name(local_id(&block_id)),
                tbl_debug_note(&block_id, &properties),
            );
            overlays.push(FullModeBlockModelQuads {
                x,
                y,
                z,
                replace: false,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(overlays)
    }

    fn chest_debug_overlays(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut chests = palette_by_pos
            .iter()
            .filter(|(_, (block_id, _))| is_chest_debug_family(local_id(block_id)))
            .map(|(pos, state)| (*pos, state.clone()))
            .collect::<Vec<_>>();
        chests.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        let mut overlays = Vec::with_capacity(chests.len());
        for ((x, y, z), (block_id, properties)) in chests {
            let template = self.chest_debug_target_label_quads(&block_id, &properties)?;
            if template.is_empty() {
                continue;
            }
            println!(
                "[LBA_FULL_MODE_V2_CHEST_VIS] pos=({}, {}, {}) block_id={} state={} facing={} type={} waterlogged={} target_faces=front/back",
                x,
                y,
                z,
                block_id,
                state_key(&block_id, &properties),
                properties.get("facing").map(String::as_str).unwrap_or("-"),
                properties.get("type").map(String::as_str).unwrap_or("-"),
                properties
                    .get("waterlogged")
                    .map(String::as_str)
                    .unwrap_or("-"),
            );
            overlays.push(FullModeBlockModelQuads {
                x,
                y,
                z,
                replace: false,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(overlays)
    }

    fn hopper_debug_overlays(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut hoppers = palette_by_pos
            .iter()
            .filter(|(_, (block_id, _))| local_id(block_id) == "hopper")
            .map(|(pos, state)| (*pos, state.clone()))
            .collect::<Vec<_>>();
        hoppers.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        let mut overlays = Vec::with_capacity(hoppers.len());
        for (index, ((x, y, z), (block_id, properties))) in hoppers.into_iter().enumerate() {
            let block_label = format!("H{:02}", index + 1);
            let template = self.build_state_template(&block_id, &properties)?;
            let mut face_overlays = Vec::with_capacity(template.len());
            println!(
                "[LBA_FULL_MODE_V2_HOPPER_VIS] id={} pos=({}, {}, {}) block_id={} state={} facing={} enabled={} face_count={}",
                block_label,
                x,
                y,
                z,
                block_id,
                state_key(&block_id, &properties),
                properties.get("facing").map(String::as_str).unwrap_or("-"),
                properties.get("enabled").map(String::as_str).unwrap_or("-"),
                template.len(),
            );
            for (face_index, quad) in template.iter().enumerate() {
                let face_label = format!("{block_label}-{:02}", face_index + 1);
                let Some(vertices) = inset_debug_quad_vertices(&quad.vertices) else {
                    continue;
                };
                let world_face = dominant_face_from_vertices(&quad.vertices);
                let center = scale3(
                    quad.vertices
                        .iter()
                        .copied()
                        .reduce(add3)
                        .unwrap_or([0.0, 0.0, 0.0]),
                    0.25,
                );
                let normal = quad_normal(&quad.vertices).unwrap_or([0.0, 0.0, 0.0]);
                println!(
                    "[LBA_FULL_MODE_V2_HOPPER_VIS_FACE] id={} block_id={} state={} world_face={} cullface={:?} center={} normal={}",
                    face_label,
                    block_id,
                    state_key(&block_id, &properties),
                    world_face,
                    quad.cullface,
                    format_vec3(center),
                    format_vec3(normal),
                );
                face_overlays.push(TemplateQuad {
                    vertices,
                    material: self.hopper_debug_label_material(
                        &block_id,
                        &properties,
                        &face_label,
                        world_face,
                    )?,
                    uv: Some(debug_label_quad_uv()),
                    double_sided: false,
                    cullface: None,
                });
            }
            overlays.push(FullModeBlockModelQuads {
                x,
                y,
                z,
                replace: false,
                quads: self.template_to_model_quads(face_overlays, material_order, material_index),
            });
        }
        Ok(overlays)
    }

    fn water_template_at(
        &mut self,
        pos: (i32, i32, i32),
        block_id: &str,
        properties: &BTreeMap<String, String>,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
    ) -> Result<Vec<TemplateQuad>> {
        let (top_material, side_material) = self.water_materials(block_id, properties)?;
        let mut quads = Vec::new();
        let corner_heights = water_corner_heights(pos, properties, palette_by_pos);

        let up_neighbor = palette_by_pos.get(&(pos.0, pos.1 + 1, pos.2));
        let render_up = water_face_should_render(up_neighbor);
        trace_water_template_face(
            pos,
            block_id,
            properties,
            "up",
            (pos.0, pos.1 + 1, pos.2),
            up_neighbor,
            render_up,
            if render_up {
                "water_template_visible"
            } else {
                water_face_skip_reason(up_neighbor)
            },
        );
        if render_up {
            quads.push(TemplateQuad {
                vertices: [
                    [0.0, corner_heights[0], 0.0],
                    [0.0, corner_heights[1], 1.0],
                    [1.0, corner_heights[2], 1.0],
                    [1.0, corner_heights[3], 0.0],
                ],
                material: top_material.clone(),
                uv: Some(typed_face_quad_uv("up")),
                double_sided: false,
                cullface: None,
            });
        }

        let down_neighbor = palette_by_pos.get(&(pos.0, pos.1 - 1, pos.2));
        let render_down = water_face_should_render(down_neighbor);
        trace_water_template_face(
            pos,
            block_id,
            properties,
            "down",
            (pos.0, pos.1 - 1, pos.2),
            down_neighbor,
            render_down,
            if render_down {
                "water_template_visible"
            } else {
                water_face_skip_reason(down_neighbor)
            },
        );
        if render_down {
            quads.push(TemplateQuad {
                vertices: face_vertices("down", [0.0, 0.0, 0.0], [1.0, 0.0, 1.0]).unwrap(),
                material: top_material.clone(),
                uv: Some(typed_face_quad_uv("down")),
                double_sided: false,
                cullface: None,
            });
        }

        for face in ["north", "south", "west", "east"] {
            let neighbor_pos = adjacent_pos(pos, face);
            let neighbor = palette_by_pos.get(&neighbor_pos);
            let render_face = water_face_should_render(neighbor);
            trace_water_template_face(
                pos,
                block_id,
                properties,
                face,
                neighbor_pos,
                neighbor,
                render_face,
                if render_face {
                    "water_template_visible"
                } else {
                    water_face_skip_reason(neighbor)
                },
            );
            if !render_face {
                continue;
            }
            let (vertices, uv) = water_side_geometry(face, corner_heights);
            quads.push(TemplateQuad {
                vertices,
                material: side_material.clone(),
                uv: Some(uv),
                double_sided: false,
                cullface: None,
            });
        }

        Ok(quads)
    }

    fn lava_template_at(
        &mut self,
        pos: (i32, i32, i32),
        block_id: &str,
        properties: &BTreeMap<String, String>,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
    ) -> Result<Vec<TemplateQuad>> {
        let (top_material, side_material) = self.lava_materials(block_id, properties)?;
        let mut quads = Vec::new();
        let corner_heights = lava_corner_heights(pos, properties, palette_by_pos);

        let up_neighbor = palette_by_pos.get(&(pos.0, pos.1 + 1, pos.2));
        let render_up = lava_face_should_render(up_neighbor);
        trace_lava_template_face(
            pos,
            block_id,
            properties,
            "up",
            (pos.0, pos.1 + 1, pos.2),
            up_neighbor,
            render_up,
        );
        if render_up {
            quads.push(TemplateQuad {
                vertices: [
                    [0.0, corner_heights[0], 0.0],
                    [0.0, corner_heights[1], 1.0],
                    [1.0, corner_heights[2], 1.0],
                    [1.0, corner_heights[3], 0.0],
                ],
                material: top_material.clone(),
                uv: Some(typed_face_quad_uv("up")),
                double_sided: false,
                cullface: None,
            });
        }

        let down_neighbor = palette_by_pos.get(&(pos.0, pos.1 - 1, pos.2));
        let render_down = lava_face_should_render(down_neighbor);
        trace_lava_template_face(
            pos,
            block_id,
            properties,
            "down",
            (pos.0, pos.1 - 1, pos.2),
            down_neighbor,
            render_down,
        );
        if render_down {
            quads.push(TemplateQuad {
                vertices: face_vertices("down", [0.0, 0.0, 0.0], [1.0, 0.0, 1.0]).unwrap(),
                material: top_material.clone(),
                uv: Some(typed_face_quad_uv("down")),
                double_sided: false,
                cullface: None,
            });
        }

        for face in ["north", "south", "west", "east"] {
            let neighbor_pos = adjacent_pos(pos, face);
            let neighbor = palette_by_pos.get(&neighbor_pos);
            let render_face = lava_face_should_render(neighbor);
            trace_lava_template_face(
                pos,
                block_id,
                properties,
                face,
                neighbor_pos,
                neighbor,
                render_face,
            );
            if !render_face {
                continue;
            }
            let (vertices, uv) = water_side_geometry(face, corner_heights);
            quads.push(TemplateQuad {
                vertices,
                material: side_material.clone(),
                uv: Some(uv),
                double_sided: false,
                cullface: None,
            });
        }

        Ok(quads)
    }

    fn piston_short_head_pair_overrides(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut replacements = Vec::new();
        let piston_debug = std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some();
        let debug_labels = piston_debug.then(|| piston_debug_labels_by_pos(palette_by_pos));
        let mut heads = palette_by_pos
            .iter()
            .filter(|(_, (block_id, properties))| {
                local_id(block_id) == "piston_head"
                    && properties.get("short").map(String::as_str) == Some("true")
            })
            .collect::<Vec<_>>();
        heads.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        for ((x, y, z), (block_id, properties)) in heads {
            let facing = properties
                .get("facing")
                .map(String::as_str)
                .unwrap_or("north");
            let base_pos = offset_block_pos((*x, *y, *z), facing, -1);
            let Some((base_block_id, base_properties)) = palette_by_pos.get(&base_pos) else {
                continue;
            };
            let base_local = local_id(base_block_id);
            if !matches!(base_local, "piston" | "sticky_piston")
                || base_properties.get("extended").map(String::as_str) != Some("true")
                || base_properties.get("facing").map(String::as_str) != Some(facing)
            {
                continue;
            }
            let expected_type = if base_local == "sticky_piston" {
                "sticky"
            } else {
                "normal"
            };
            if properties
                .get("type")
                .map(String::as_str)
                .unwrap_or("normal")
                != expected_type
            {
                continue;
            }

            let mut render_properties = properties.clone();
            render_properties.insert("short".to_string(), "false".to_string());
            let mut template = self.build_state_template(block_id, &render_properties)?;
            if piston_debug {
                let label = debug_labels
                    .as_ref()
                    .and_then(|labels| labels.get(&(*x, *y, *z)))
                    .cloned()
                    .unwrap_or_else(|| "PAIR".to_string());
                template.extend(self.piston_debug_label_quads(&label, block_id, properties)?);
                println!(
                    "[LBA_FULL_MODE_V2_PISTON_SHORT_PAIR] block_id={} state={} facing={} short={} type={} head_pos=({}, {}, {}) base_pos=({}, {}, {}) base_block={} base_state={} base_head_block_distance=1.000 base_recess=0.250 source_head_reach=1.000 final_head_reach=1.250 replace_short_geometry=true",
                    block_id,
                    state_key(block_id, properties),
                    facing,
                    properties.get("short").map(String::as_str).unwrap_or("-"),
                    properties.get("type").map(String::as_str).unwrap_or("-"),
                    x,
                    y,
                    z,
                    base_pos.0,
                    base_pos.1,
                    base_pos.2,
                    base_block_id,
                    state_key(base_block_id, base_properties),
                );
            }
            replacements.push(FullModeBlockModelQuads {
                x: *x,
                y: *y,
                z: *z,
                replace: true,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(replacements)
    }

    fn piston_standalone_vertical_base_overrides(
        &mut self,
        palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Result<Vec<FullModeBlockModelQuads>> {
        let mut replacements = Vec::new();
        let piston_debug = std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some();
        let debug_labels = piston_debug.then(|| piston_debug_labels_by_pos(palette_by_pos));
        let mut bases = palette_by_pos
            .iter()
            .filter(|(_, (block_id, properties))| {
                matches!(local_id(block_id), "piston" | "sticky_piston")
                    && matches!(
                        properties.get("facing").map(String::as_str),
                        Some("up" | "down")
                    )
            })
            .collect::<Vec<_>>();
        bases.sort_by_key(|((x, y, z), _)| (*z, *y, *x));

        for ((x, y, z), (block_id, properties)) in bases {
            let local = local_id(block_id);
            let facing = properties
                .get("facing")
                .map(String::as_str)
                .unwrap_or("north");
            if properties.get("extended").map(String::as_str) == Some("true") {
                let head_pos = offset_block_pos((*x, *y, *z), facing, 1);
                if let Some((head_block_id, head_properties)) = palette_by_pos.get(&head_pos) {
                    let expected_type = if local == "sticky_piston" {
                        "sticky"
                    } else {
                        "normal"
                    };
                    let matching_head = local_id(head_block_id) == "piston_head"
                        && head_properties.get("facing").map(String::as_str) == Some(facing)
                        && head_properties
                            .get("type")
                            .map(String::as_str)
                            .unwrap_or("normal")
                            == expected_type;
                    if matching_head {
                        continue;
                    }
                }
            }

            let mut render_properties = properties.clone();
            render_properties.insert(
                "__lba_standalone_vertical_base".to_string(),
                "true".to_string(),
            );
            let mut template = self.build_state_template(block_id, &render_properties)?;
            if piston_debug {
                let label = debug_labels
                    .as_ref()
                    .and_then(|labels| labels.get(&(*x, *y, *z)))
                    .cloned()
                    .unwrap_or_else(|| "BASE".to_string());
                template.extend(self.piston_debug_label_quads(&label, block_id, properties)?);
                println!(
                    "[LBA_FULL_MODE_V2_PISTON_STANDALONE_BASE] block_id={} state={} facing={} extended={} pos=({}, {}, {}) matching_head=false replace_vertical_base_geometry=true",
                    block_id,
                    state_key(block_id, properties),
                    facing,
                    properties
                        .get("extended")
                        .map(String::as_str)
                        .unwrap_or("-"),
                    x,
                    y,
                    z,
                );
            }
            replacements.push(FullModeBlockModelQuads {
                x: *x,
                y: *y,
                z: *z,
                replace: true,
                quads: self.template_to_model_quads(template, material_order, material_index),
            });
        }
        Ok(replacements)
    }

    fn observer_debug_label_quads(
        &mut self,
        label: &str,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let facing = properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north");
        let mut quads = Vec::with_capacity(OBSERVER_DEBUG_FACE_ORDER.len());
        for face in OBSERVER_DEBUG_FACE_ORDER.iter() {
            let (semantic_key, semantic_text) = observer_debug_face_semantic(facing, face);
            let text = format!("{label}-{semantic_text}");
            if observer_debug_target_text(&text) {
                println!(
                    "[LBA_FULL_MODE_V2_OBSERVER_B] id={} state={} facing={} powered={} world_face={} vertices={:?}",
                    text,
                    state_key(block_id, properties),
                    properties.get("facing").map(String::as_str).unwrap_or("-"),
                    properties.get("powered").map(String::as_str).unwrap_or("-"),
                    face,
                    observer_debug_label_face_vertices(face),
                );
            }
            let material = self.observer_debug_label_material(
                block_id,
                properties,
                label,
                semantic_key,
                &text,
                face,
            )?;
            quads.push(TemplateQuad {
                vertices: observer_debug_label_face_vertices(face),
                material,
                uv: Some(observer_debug_label_face_uv(face)),
                double_sided: false,
                cullface: None,
            });
        }
        Ok(quads)
    }

    fn observer_debug_label_material(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        label: &str,
        semantic_key: &str,
        text: &str,
        face: &str,
    ) -> Result<MaterialImage> {
        let font = self.load_ascii_font()?;
        let mut image = RgbaImage::from_pixel(192, 48, Rgba([0, 0, 0, 0]));
        fill_debug_label_background(&mut image);
        draw_debug_text_at(
            &mut image,
            &font,
            format!("{label}-").as_str(),
            10,
            7,
            Rgba([0, 0, 0, 255]),
            4,
        );
        let word = load_observer_debug_semantic_image(semantic_key)?;
        let word_y = image.height().saturating_sub(word.height()) / 2;
        overlay(&mut image, &word, 82, i64::from(word_y));
        Ok(MaterialImage {
            key: format!(
                "generated:observer_debug_label:{}:{}:{}:{}:{}",
                state_key(block_id, properties),
                text,
                face,
                properties.get("facing").map(String::as_str).unwrap_or("-"),
                properties.get("powered").map(String::as_str).unwrap_or("-"),
            ),
            image,
            alpha_mode: FullModeAlphaMode::Translucent,
        })
    }

    fn chain_banner_debug_label_quads(
        &mut self,
        label: &str,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        Ok(vec![TemplateQuad {
            vertices: chain_banner_debug_label_vertices(),
            material: self.chain_banner_debug_label_material(block_id, properties, label)?,
            uv: Some(debug_label_quad_uv()),
            double_sided: false,
            cullface: None,
        }])
    }

    fn chain_banner_debug_label_material(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        label: &str,
    ) -> Result<MaterialImage> {
        let font = self.load_ascii_font()?;
        let mut image = RgbaImage::from_pixel(192, 48, Rgba([0, 0, 0, 0]));
        fill_debug_label_background(&mut image);
        draw_debug_text(&mut image, &font, label, 7, Rgba([0, 0, 0, 255]), 4);
        Ok(MaterialImage {
            key: format!(
                "generated:chain_banner_debug_label:{}:{}",
                state_key(block_id, properties),
                label,
            ),
            image,
            alpha_mode: FullModeAlphaMode::Translucent,
        })
    }

    fn tbl_debug_label_quads(
        &mut self,
        label: &str,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        Ok(vec![TemplateQuad {
            vertices: chain_banner_debug_label_vertices(),
            material: self.tbl_debug_label_material(block_id, properties, label)?,
            uv: Some(debug_label_quad_uv()),
            double_sided: false,
            cullface: None,
        }])
    }

    fn tbl_debug_label_material(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        label: &str,
    ) -> Result<MaterialImage> {
        let font = self.load_ascii_font()?;
        let mut image = RgbaImage::from_pixel(192, 48, Rgba([0, 0, 0, 0]));
        fill_debug_label_background(&mut image);
        draw_debug_text(&mut image, &font, label, 7, Rgba([0, 0, 0, 255]), 4);
        Ok(MaterialImage {
            key: format!(
                "generated:tbl_debug_label:{}:{}",
                state_key(block_id, properties),
                label,
            ),
            image,
            alpha_mode: FullModeAlphaMode::Translucent,
        })
    }

    fn piston_debug_label_quads(
        &mut self,
        label: &str,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let mut quads = Vec::with_capacity(PISTON_DEBUG_FACE_ORDER.len());
        for (index, face) in PISTON_DEBUG_FACE_ORDER.iter().enumerate() {
            let text = format!("{label}-{}", index + 1);
            let material = self.piston_debug_label_material(block_id, properties, &text, face)?;
            quads.push(TemplateQuad {
                vertices: piston_debug_label_face_vertices(face),
                material,
                uv: Some(typed_face_quad_uv(face)),
                double_sided: false,
                cullface: None,
            });
        }
        Ok(quads)
    }

    fn piston_debug_label_material(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        text: &str,
        face: &str,
    ) -> Result<MaterialImage> {
        let font = self.load_ascii_font()?;
        let mut image = RgbaImage::from_pixel(192, 48, Rgba([0, 0, 0, 0]));
        fill_debug_label_background(&mut image);
        draw_debug_text(&mut image, &font, text, 7, Rgba([0, 0, 0, 255]), 4);
        Ok(MaterialImage {
            key: format!(
                "generated:piston_debug_label:{}:{}:{}:{}",
                state_key(block_id, properties),
                text,
                face,
                properties.get("facing").map(String::as_str).unwrap_or("-")
            ),
            image,
            alpha_mode: FullModeAlphaMode::Translucent,
        })
    }

    fn chest_debug_label_material(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        text: &str,
        face: &str,
    ) -> Result<MaterialImage> {
        let font = self.load_ascii_font()?;
        let mut image = RgbaImage::from_pixel(192, 48, Rgba([0, 0, 0, 0]));
        fill_debug_label_background(&mut image);
        draw_debug_text(&mut image, &font, text, 7, Rgba([0, 0, 0, 255]), 4);
        Ok(MaterialImage {
            key: format!(
                "generated:chest_debug_label:{}:{}:{}:{}",
                state_key(block_id, properties),
                text,
                face,
                properties.get("facing").map(String::as_str).unwrap_or("-")
            ),
            image,
            alpha_mode: FullModeAlphaMode::Translucent,
        })
    }

    fn hopper_debug_label_material(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        text: &str,
        face: &str,
    ) -> Result<MaterialImage> {
        let font = self.load_ascii_font()?;
        let mut image = RgbaImage::from_pixel(192, 48, Rgba([0, 0, 0, 0]));
        fill_debug_label_background(&mut image);
        draw_debug_text(&mut image, &font, text, 7, Rgba([0, 0, 0, 255]), 4);
        Ok(MaterialImage {
            key: format!(
                "generated:hopper_debug_label:{}:{}:{}:{}",
                state_key(block_id, properties),
                text,
                face,
                properties.get("facing").map(String::as_str).unwrap_or("-")
            ),
            image,
            alpha_mode: FullModeAlphaMode::Translucent,
        })
    }

    fn chest_debug_target_label_quads(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<Vec<TemplateQuad>> {
        let facing = properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north");
        let chest_type = properties
            .get("type")
            .map(String::as_str)
            .unwrap_or("single");
        if !matches!(facing, "east" | "west") || !matches!(chest_type, "left" | "right") {
            return Ok(Vec::new());
        }

        let mut quads = Vec::with_capacity(2);
        for world_face in ["west", "east"] {
            let Some((face_role, pair_half)) =
                chest_debug_visible_pair_half(properties, world_face)
            else {
                continue;
            };
            let label = format!(
                "{}-{}:{}",
                face_role,
                pair_half,
                chest_debug_material_identity_label(block_id, properties, world_face)
            );
            println!(
                "[LBA_FULL_MODE_V2_CHEST_VIS_FACE] state={} world_face={} face_role={} pair_half={} label={}",
                state_key(block_id, properties),
                world_face,
                face_role,
                pair_half,
                label,
            );
            quads.push(TemplateQuad {
                vertices: chest_debug_label_face_vertices(world_face),
                material: self
                    .chest_debug_label_material(block_id, properties, &label, world_face)?,
                uv: Some(typed_face_quad_uv(world_face)),
                double_sided: false,
                cullface: None,
            });
        }
        Ok(quads)
    }

    fn template_to_model_quads(
        &self,
        template: Vec<TemplateQuad>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
    ) -> Vec<FullModeModelQuad> {
        self.template_to_model_quads_with_context(
            "",
            &BTreeMap::new(),
            template,
            material_order,
            material_index,
            None,
        )
    }

    fn template_to_model_quads_with_context(
        &self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        template: Vec<TemplateQuad>,
        material_order: &mut Vec<MaterialImage>,
        material_index: &mut HashMap<String, u32>,
        mut palette_material: Option<&mut FullModePaletteMaterial>,
    ) -> Vec<FullModeModelQuad> {
        template
            .into_iter()
            .map(|quad| {
                let slot =
                    self.material_slot(material_order, material_index, quad.material.clone());
                if let Some(palette_material) = palette_material.as_deref_mut() {
                    match quad.cullface.as_deref() {
                        Some("down") => palette_material.down = Some(slot),
                        Some("up") => palette_material.up = Some(slot),
                        Some("north") => palette_material.north = Some(slot),
                        Some("south") => palette_material.south = Some(slot),
                        Some("west") => palette_material.west = Some(slot),
                        Some("east") => palette_material.east = Some(slot),
                        _ => {
                            if palette_material.cross.is_none() {
                                palette_material.cross = Some(slot);
                            }
                        }
                    }
                }
                let world_face = dominant_face_from_vertices(&quad.vertices);
                if let Some(rep) = observer_semantic_trace_target(properties, world_face)
                    .or_else(|| observer_detect_model_target(properties, world_face))
                {
                    let family = observer_texture_family(&quad.material.key).unwrap_or("?");
                    println!(
                        "[LBA_OBSERVER_MODEL_20260423D] rep={} family={} material_slot={} material_key={} quad_uv={} world_face={}",
                        rep,
                        family,
                        slot,
                        quad.material.key,
                        quad.uv
                            .as_ref()
                            .map(format_uv2)
                            .unwrap_or_else(|| "None".to_string()),
                        world_face,
                    );
                }
                if observer_debug_target_material_key(&quad.material.key) {
                    println!(
                        "[LBA_FULL_MODE_V2_OBSERVER_C] material_key={} slot={} uv={:?} world_bounds={}",
                        quad.material.key,
                        slot,
                        quad.uv,
                        quantized_chest_debug_world_bounds(&quad.vertices),
                    );
                }
                self.log_chest_debug_model_quad(block_id, properties, &quad, slot);
                FullModeModelQuad {
                    vertices: quad.vertices,
                    material: slot,
                    uv: quad.uv,
                    double_sided: quad.double_sided,
                    cullface: quad.cullface,
                }
            })
            .collect()
    }

    fn log_chest_debug_model_quad(
        &self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        quad: &TemplateQuad,
        slot: u32,
    ) {
        if !chest_debug_palette_target(block_id, properties) {
            return;
        }
        let world_face = dominant_face_from_vertices(&quad.vertices);
        if let Some((face_role, pair_half)) = chest_debug_visible_pair_half(properties, world_face)
        {
            let center = scale3(
                quad.vertices
                    .iter()
                    .copied()
                    .reduce(add3)
                    .unwrap_or([0.0, 0.0, 0.0]),
                0.25,
            );
            let normal = quad_normal(&quad.vertices).unwrap_or([0.0, 0.0, 0.0]);
            let state = state_key(block_id, properties);
            let lookup_key = chest_debug_bc_lookup_key(&state, &quad.vertices);
            if let Some(provenance) = self.chest_debug_bc_provenance.get(&lookup_key) {
                println!(
                    "[LBA_FULL_MODE_V2_CHEST_B2C_C] sample={} state={} provenance_id={} facing={} type={} template_identity={} part={} local_face={} face_role={} pair_half={} material_slot={} quad_uv={} local_cuboid_bounds={} world_bounds={} candidate={} world_face={}",
                    provenance.sample_label,
                    state,
                    provenance.provenance_id,
                    provenance.facing,
                    provenance.chest_type,
                    provenance.template_identity,
                    provenance.part,
                    provenance.local_face,
                    provenance.face_role,
                    provenance.pair_half,
                    slot,
                    quad.uv
                        .as_ref()
                        .map(format_uv2)
                        .unwrap_or_else(|| "None".to_string()),
                    provenance.local_cuboid_bounds,
                    quantized_chest_debug_world_bounds(&quad.vertices),
                    provenance.candidate_label,
                    provenance.world_face,
                );
            }
            println!(
                "[LBA_FULL_MODE_V2_CHEST_MODEL] state={} world_face={} face_role={} pair_half={} material_slot={} quad_uv={} cullface={:?} center={} normal={} vertices={:?}",
                state,
                world_face,
                face_role,
                pair_half,
                slot,
                quad.uv
                    .as_ref()
                    .map(format_uv2)
                    .unwrap_or_else(|| "None".to_string()),
                quad.cullface,
                format_vec3(center),
                format_vec3(normal),
                quad.vertices,
            );
        }
    }

    fn chest_debug_bc_records(
        &self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        template_identity: ChestTemplateIdentity,
        template: ChestTemplateSpec,
    ) -> Vec<(String, ChestDebugBCProvenance)> {
        if !chest_debug_palette_target(block_id, properties) {
            return Vec::new();
        }
        let Some(sample_label) = chest_debug_sample_label(properties) else {
            return Vec::new();
        };
        let facing = properties
            .get("facing")
            .cloned()
            .unwrap_or_else(|| "north".to_string());
        let chest_type = properties
            .get("type")
            .cloned()
            .unwrap_or_else(|| "single".to_string());
        let targets = [
            ("base", "north", template.base_min, template.base_max),
            ("base", "south", template.base_min, template.base_max),
            ("lid", "north", template.lid_min, template.lid_max),
            ("lid", "south", template.lid_min, template.lid_max),
        ];
        let mut records = Vec::new();
        for (part, local_face, min, max) in targets {
            let world_face =
                rotate_direction(local_face, 0, chest_rotation_degrees(&facing) as i32);
            let Some((face_role, pair_half)) =
                chest_debug_visible_pair_half(properties, &world_face)
            else {
                continue;
            };
            let candidate_label = match (part, face_role) {
                ("base", "F") => "base_front_candidate_A",
                ("base", "B") => "base_back_candidate_A",
                ("lid", "F") => "lid_front_candidate_B",
                ("lid", "B") => "lid_back_candidate_B",
                _ => continue,
            };
            let provenance_id = format!(
                "{}|{}|{}|{}|{}|{}|{}",
                sample_label,
                template_identity.label(),
                part,
                local_face,
                world_face,
                face_role,
                pair_half
            );
            println!(
                "[LBA_FULL_MODE_V2_CHEST_B2C_B] sample={} state={} facing={} type={} template_identity={} part={} local_face={} world_face={} face_role={} pair_half={} local_cuboid_bounds={} provenance_id={}",
                sample_label,
                state_key(block_id, properties),
                facing,
                chest_type,
                template_identity.label(),
                part,
                local_face,
                world_face,
                face_role,
                pair_half,
                format_bounds3(min, max),
                provenance_id,
            );
            records.push((
                format!("{part}:{local_face}"),
                ChestDebugBCProvenance {
                    sample_label,
                    facing: facing.clone(),
                    chest_type: chest_type.clone(),
                    template_identity: template_identity.label(),
                    part,
                    local_face,
                    world_face,
                    face_role,
                    pair_half,
                    candidate_label,
                    local_cuboid_bounds: format_bounds3(min, max),
                    provenance_id,
                },
            ));
        }
        records
    }

    fn store_chest_debug_bc_records(
        &mut self,
        block_id: &str,
        properties: &BTreeMap<String, String>,
        base_quads: &[TemplateQuad],
        lid_quads: &[TemplateQuad],
        records: &[(String, ChestDebugBCProvenance)],
    ) {
        let state = state_key(block_id, properties);
        for (target_key, provenance) in records {
            let quad = match target_key.as_str() {
                "base:north" => base_quads.get(1),
                "base:south" => base_quads.get(3),
                "lid:north" => lid_quads.get(1),
                "lid:south" => lid_quads.get(3),
                _ => None,
            };
            let Some(quad) = quad else {
                continue;
            };
            let lookup_key = chest_debug_bc_lookup_key(&state, &quad.vertices);
            self.chest_debug_bc_provenance
                .insert(lookup_key, provenance.clone());
        }
    }

    fn read_json(&mut self, path: &str) -> Result<Option<Value>> {
        if let Some(cached) = self.json_cache.get(path) {
            return Ok(cached.clone());
        }
        let mut file = match self.vanilla.by_name(path) {
            Ok(file) => file,
            Err(_) => {
                self.json_cache.insert(path.to_string(), None);
                return Ok(None);
            }
        };
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        let value: Value =
            serde_json::from_str(&text).with_context(|| format!("parse json failed: {path}"))?;
        self.json_cache
            .insert(path.to_string(), Some(value.clone()));
        Ok(Some(value))
    }

    fn read_typed_json<T>(&mut self, path: &str) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut file = match self.vanilla.by_name(path) {
            Ok(file) => file,
            Err(_) => return Ok(None),
        };
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        let value = serde_json::from_str(&text)
            .with_context(|| format!("parse typed json failed: {path}"))?;
        Ok(Some(value))
    }

    fn load_texture(&mut self, texture_id: &str) -> Result<Option<(RgbaImage, ResourceFlavor)>> {
        if let Some(cached) = self.texture_cache.get(texture_id) {
            return Ok(cached.clone());
        }
        let candidates = self.texture_candidates(texture_id);
        for (path, flavor) in candidates {
            if let Some(image) = load_png_path(&path)? {
                let out = Some((crop_first_animation_frame(image), flavor));
                self.texture_cache
                    .insert(texture_id.to_string(), out.clone());
                return Ok(out);
            }
        }
        let asset_path = texture_asset_path(texture_id);
        if let Ok(mut file) = self.vanilla.by_name(&asset_path) {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let image = image::load_from_memory(&bytes)?.to_rgba8();
            let out = Some((crop_first_animation_frame(image), ResourceFlavor::Vanilla));
            self.texture_cache
                .insert(texture_id.to_string(), out.clone());
            return Ok(out);
        }
        let out = None;
        self.texture_cache
            .insert(texture_id.to_string(), out.clone());
        Ok(out)
    }

    fn load_texture_with_alias(
        &mut self,
        texture_id: &str,
        allow_xk: bool,
    ) -> Result<(RgbaImage, String)> {
        if let Some((image, flavor)) = self.load_texture_from_candidates(texture_id, allow_xk)? {
            return Ok((image, format!("{}:{}", flavor_label(flavor), texture_id)));
        }
        if let Some(alias) = texture_alias(texture_id)
            && let Some((image, flavor)) = self.load_texture_from_candidates(&alias, allow_xk)?
        {
            println!(
                "[LBA_FULL_MODE_V2] texture_alias original={} alias={} flavor={}",
                texture_id,
                alias,
                flavor_label(flavor)
            );
            return Ok((image, format!("{}:{}", flavor_label(flavor), alias)));
        }
        println!(
            "[LBA_FULL_MODE_V2] texture_missing_skip texture={} fallback=transparent",
            texture_id
        );
        Ok((
            transparent_missing_texture(),
            format!("transparent_missing:{texture_id}"),
        ))
    }

    fn load_texture_from_candidates(
        &mut self,
        texture_id: &str,
        allow_xk: bool,
    ) -> Result<Option<(RgbaImage, ResourceFlavor)>> {
        if allow_xk {
            return self.load_texture(texture_id);
        }
        for (path, flavor) in self.texture_candidates(texture_id) {
            if flavor == ResourceFlavor::Xk {
                continue;
            }
            if let Some(image) = load_png_path(&path)? {
                return Ok(Some((crop_first_animation_frame(image), flavor)));
            }
        }
        let asset_path = texture_asset_path(texture_id);
        if let Ok(mut file) = self.vanilla.by_name(&asset_path) {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let image = image::load_from_memory(&bytes)?.to_rgba8();
            return Ok(Some((
                crop_first_animation_frame(image),
                ResourceFlavor::Vanilla,
            )));
        }
        Ok(None)
    }

    fn texture_source_trace(&self, texture_id: &str, allow_xk: bool) -> Result<String> {
        for (path, flavor) in self.texture_candidates(texture_id) {
            if !allow_xk && flavor == ResourceFlavor::Xk {
                continue;
            }
            if path.is_file() {
                return Ok(format!("{}:{}", flavor_label(flavor), path.display()));
            }
        }
        let asset_path = texture_asset_path(texture_id);
        if self.vanilla.file_names().any(|name| name == asset_path) {
            return Ok(format!("vanilla-jar:{asset_path}"));
        }
        Ok(format!("missing:{texture_id}"))
    }

    fn load_ascii_font(&mut self) -> Result<RgbaImage> {
        if let Some(font) = &self.font_ascii {
            return Ok(font.clone());
        }
        let mut file = self
            .vanilla
            .by_name("assets/minecraft/textures/font/ascii.png")
            .context("missing vanilla ascii font")?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let image = image::load_from_memory(&bytes)?.to_rgba8();
        self.font_ascii = Some(image.clone());
        Ok(image)
    }

    fn texture_candidates(&self, texture_id: &str) -> Vec<(PathBuf, ResourceFlavor)> {
        let (namespace, texture_path) = texture_namespace_path(texture_id);
        let mut paths = Vec::new();
        let rel = format!("assets/{namespace}/textures/{texture_path}.png");
        paths.push((self.xk_root.join(&rel), ResourceFlavor::Xk));
        paths.push((self.faithful_root.join(&rel), ResourceFlavor::Faithful));
        paths.push((
            self.block_26_root
                .join(format!("{}.png", texture_path.trim_start_matches("block/"))),
            ResourceFlavor::Fallback,
        ));
        paths
    }
}

const FACE_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const OBSERVER_DEBUG_FACE_ORDER: [&str; 6] = ["up", "down", "north", "south", "west", "east"];
const PISTON_DEBUG_FACE_ORDER: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

fn chain_model_ref(properties: &BTreeMap<String, String>) -> OwnedModelRef {
    match properties.get("axis").map(String::as_str).unwrap_or("y") {
        "x" => OwnedModelRef {
            model: "minecraft:block/iron_chain".to_string(),
            x: 90,
            y: 90,
        },
        "z" => OwnedModelRef {
            model: "minecraft:block/iron_chain".to_string(),
            x: 90,
            y: 0,
        },
        _ => OwnedModelRef {
            model: "minecraft:block/iron_chain".to_string(),
            x: 0,
            y: 0,
        },
    }
}

fn fallback_chain_resolved_model() -> TypedResolvedModel {
    let mut textures = HashMap::new();
    textures.insert(
        "texture".to_string(),
        "minecraft:block/iron_chain".to_string(),
    );
    textures.insert("all".to_string(), "minecraft:block/iron_chain".to_string());
    textures.insert(
        "particle".to_string(),
        "minecraft:block/iron_chain".to_string(),
    );

    let mut north_south_faces = HashMap::new();
    north_south_faces.insert(
        "north".to_string(),
        ResolvedFace {
            texture: "#all".to_string(),
            uv: Some([3.0, 0.0, 0.0, 16.0]),
            rotation: 0,
            cullface: None,
            tintindex: None,
        },
    );
    north_south_faces.insert(
        "south".to_string(),
        ResolvedFace {
            texture: "#all".to_string(),
            uv: Some([0.0, 0.0, 3.0, 16.0]),
            rotation: 0,
            cullface: None,
            tintindex: None,
        },
    );

    let mut east_west_faces = HashMap::new();
    east_west_faces.insert(
        "west".to_string(),
        ResolvedFace {
            texture: "#all".to_string(),
            uv: Some([6.0, 0.0, 3.0, 16.0]),
            rotation: 0,
            cullface: None,
            tintindex: None,
        },
    );
    east_west_faces.insert(
        "east".to_string(),
        ResolvedFace {
            texture: "#all".to_string(),
            uv: Some([3.0, 0.0, 6.0, 16.0]),
            rotation: 0,
            cullface: None,
            tintindex: None,
        },
    );

    let rotation = Some(TypedElementRotation {
        origin: [8.0, 8.0, 8.0],
        axis: "y".to_string(),
        angle: 45.0,
        rescale: false,
    });
    TypedResolvedModel {
        textures,
        elements: vec![
            ResolvedElement {
                from: [6.5, 0.0, 8.0],
                to: [9.5, 16.0, 8.0],
                rotation: rotation.clone(),
                faces: north_south_faces,
            },
            ResolvedElement {
                from: [8.0, 0.0, 6.5],
                to: [8.0, 16.0, 9.5],
                rotation,
                faces: east_west_faces,
            },
        ],
    }
}

fn chain_banner_debug_target(local: &str) -> bool {
    local == "chain" || local.ends_with("_banner")
}

fn chain_banner_debug_note(block_id: &str, properties: &BTreeMap<String, String>) -> String {
    let local = local_id(block_id);
    if local == "chain" {
        return format!(
            "chain axis={}",
            properties.get("axis").map(String::as_str).unwrap_or("-")
        );
    }
    if local.ends_with("_wall_banner") {
        return format!(
            "wall banner facing={}",
            properties.get("facing").map(String::as_str).unwrap_or("-")
        );
    }
    if local.ends_with("_banner") {
        return format!(
            "standing banner rotation={}",
            properties
                .get("rotation")
                .map(String::as_str)
                .unwrap_or("-")
        );
    }
    local.to_string()
}

fn tbl_debug_family_prefix(local: &str) -> Option<&'static str> {
    if local.ends_with("_trapdoor") {
        Some("T")
    } else if local.ends_with("_banner") {
        Some("B")
    } else if local == "lever" {
        Some("L")
    } else {
        None
    }
}

fn tbl_debug_family_name(local: &str) -> &'static str {
    if local.ends_with("_trapdoor") {
        "trapdoor"
    } else if local.ends_with("_wall_banner") {
        "wall_banner"
    } else if local.ends_with("_banner") {
        "standing_banner"
    } else if local == "lever" {
        "lever"
    } else {
        "other"
    }
}

fn tbl_debug_note(block_id: &str, properties: &BTreeMap<String, String>) -> String {
    let local = local_id(block_id);
    if local.ends_with("_trapdoor") {
        return format!(
            "facing={} half={} open={} powered={} waterlogged={}",
            properties.get("facing").map(String::as_str).unwrap_or("-"),
            properties.get("half").map(String::as_str).unwrap_or("-"),
            properties.get("open").map(String::as_str).unwrap_or("-"),
            properties.get("powered").map(String::as_str).unwrap_or("-"),
            properties
                .get("waterlogged")
                .map(String::as_str)
                .unwrap_or("-"),
        );
    }
    if local.ends_with("_wall_banner") {
        return format!(
            "wall_banner facing={}",
            properties.get("facing").map(String::as_str).unwrap_or("-")
        );
    }
    if local.ends_with("_banner") {
        return format!(
            "standing_banner rotation={}",
            properties
                .get("rotation")
                .map(String::as_str)
                .unwrap_or("-")
        );
    }
    if local == "lever" {
        return format!(
            "face={} facing={} powered={}",
            properties.get("face").map(String::as_str).unwrap_or("-"),
            properties.get("facing").map(String::as_str).unwrap_or("-"),
            properties.get("powered").map(String::as_str).unwrap_or("-"),
        );
    }
    "-".to_string()
}

fn model_refs_from_apply(value: &Value) -> Vec<OwnedModelRef> {
    match model_ref_from_apply(value) {
        Some(model) => vec![model],
        None => Vec::new(),
    }
}

fn typed_model_refs_from_apply(value: &TypedModelApply) -> Vec<OwnedModelRef> {
    let item = match value {
        TypedModelApply::Single(item) => item,
        TypedModelApply::Weighted(items) => {
            match items.iter().max_by_key(|item| item.weight.unwrap_or(1)) {
                Some(item) => item,
                None => return Vec::new(),
            }
        }
    };
    vec![OwnedModelRef {
        model: item.model.clone(),
        x: item.x % 360,
        y: item.y % 360,
    }]
}

fn hopper_typed_model_refs(properties: &BTreeMap<String, String>) -> Vec<OwnedModelRef> {
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("down");
    let enabled = properties
        .get("enabled")
        .map(String::as_str)
        .unwrap_or("true");
    let powered_model = enabled == "false";
    let (model, y) = match facing {
        "down" => (
            if powered_model {
                "minecraft:block/hopper_on"
            } else {
                "minecraft:block/hopper"
            },
            0,
        ),
        "south" => (
            if powered_model {
                "minecraft:block/hopper_side_on"
            } else {
                "minecraft:block/hopper_side"
            },
            180,
        ),
        "west" => (
            if powered_model {
                "minecraft:block/hopper_side_on"
            } else {
                "minecraft:block/hopper_side"
            },
            270,
        ),
        "east" => (
            if powered_model {
                "minecraft:block/hopper_side_on"
            } else {
                "minecraft:block/hopper_side"
            },
            90,
        ),
        _ => (
            if powered_model {
                "minecraft:block/hopper_side_on"
            } else {
                "minecraft:block/hopper_side"
            },
            0,
        ),
    };
    vec![OwnedModelRef {
        model: model.to_string(),
        x: 0,
        y,
    }]
}

impl TypedTextureRef {
    fn texture_id(self) -> String {
        match self {
            TypedTextureRef::Id(id) => id,
            TypedTextureRef::Sprite {
                sprite,
                force_translucent,
            } => {
                let _ = force_translucent;
                sprite
            }
        }
    }
}

impl From<TypedModelElement> for ResolvedElement {
    fn from(value: TypedModelElement) -> Self {
        Self {
            from: value.from,
            to: value.to,
            rotation: value.rotation,
            faces: value
                .faces
                .into_iter()
                .map(|(name, face)| {
                    (
                        name,
                        ResolvedFace {
                            texture: face.texture,
                            uv: face.uv,
                            rotation: face.rotation,
                            cullface: face.cullface,
                            tintindex: face.tintindex,
                        },
                    )
                })
                .collect(),
        }
    }
}

fn model_ref_from_apply(value: &Value) -> Option<OwnedModelRef> {
    let item = value
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .max_by_key(|entry| entry.get("weight").and_then(Value::as_i64).unwrap_or(1))
        })
        .unwrap_or(value);
    let model = item.get("model")?.as_str()?.to_string();
    Some(OwnedModelRef {
        model,
        x: item.get("x").and_then(Value::as_i64).unwrap_or(0) as i32 % 360,
        y: item.get("y").and_then(Value::as_i64).unwrap_or(0) as i32 % 360,
    })
}

fn variant_score(key: &str, properties: &BTreeMap<String, String>) -> Option<i32> {
    if key.trim().is_empty() {
        return Some(0);
    }
    let mut score = 0;
    for part in key.split(',') {
        let (name, value) = part.split_once('=')?;
        let actual = properties.get(name.trim()).map(String::as_str)?;
        if !value.split('|').any(|expected| expected.trim() == actual) {
            return None;
        }
        score += 1;
    }
    Some(score)
}

fn log_skipped_unknown_block(block_id: &str, properties: &BTreeMap<String, String>, reason: &str) {
    if std::env::var_os("LBA_FULL_MODE_V2_SKIP_DEBUG").is_some() {
        println!(
            "[LBA_FULL_MODE_V2_SKIP] block={} state={} reason={}",
            block_id,
            state_key(block_id, properties),
            reason
        );
    }
}

#[derive(Clone, Copy)]
enum ChestTemplateIdentity {
    Single,
    Left,
    Right,
}

impl ChestTemplateIdentity {
    fn label(self) -> &'static str {
        match self {
            Self::Single => "single_template",
            Self::Left => "left_template",
            Self::Right => "right_template",
        }
    }
}

#[derive(Clone, Copy)]
struct ChestTemplateSpec {
    base_min: [f32; 3],
    base_max: [f32; 3],
    base_size: [f32; 3],
    lid_min: [f32; 3],
    lid_max: [f32; 3],
    lid_size: [f32; 3],
    lock_min: [f32; 3],
    lock_max: [f32; 3],
    lock_size: [f32; 3],
}

fn chest_canonical_identity(chest_type: &str) -> ChestTemplateIdentity {
    match chest_type {
        "left" => ChestTemplateIdentity::Right,
        "right" => ChestTemplateIdentity::Left,
        _ => ChestTemplateIdentity::Single,
    }
}

fn chest_template_spec(identity: ChestTemplateIdentity) -> ChestTemplateSpec {
    match identity {
        ChestTemplateIdentity::Single => ChestTemplateSpec {
            base_min: [1.0 / 16.0, 0.0, 1.0 / 16.0],
            base_max: [15.0 / 16.0, 10.0 / 16.0, 15.0 / 16.0],
            base_size: [14.0, 10.0, 14.0],
            lid_min: [1.0 / 16.0, 9.0 / 16.0, 1.0 / 16.0],
            lid_max: [15.0 / 16.0, 14.0 / 16.0, 15.0 / 16.0],
            lid_size: [14.0, 5.0, 14.0],
            lock_min: [7.0 / 16.0, 7.0 / 16.0, 0.0],
            lock_max: [9.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0],
            lock_size: [2.0, 4.0, 1.0],
        },
        ChestTemplateIdentity::Left => ChestTemplateSpec {
            base_min: [0.0, 0.0, 1.0 / 16.0],
            base_max: [15.0 / 16.0, 10.0 / 16.0, 15.0 / 16.0],
            base_size: [15.0, 10.0, 14.0],
            lid_min: [0.0, 9.0 / 16.0, 1.0 / 16.0],
            lid_max: [15.0 / 16.0, 14.0 / 16.0, 15.0 / 16.0],
            lid_size: [15.0, 5.0, 14.0],
            lock_min: [0.0, 7.0 / 16.0, 0.0],
            lock_max: [1.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0],
            lock_size: [1.0, 4.0, 1.0],
        },
        ChestTemplateIdentity::Right => ChestTemplateSpec {
            base_min: [1.0 / 16.0, 0.0, 1.0 / 16.0],
            base_max: [1.0, 10.0 / 16.0, 15.0 / 16.0],
            base_size: [15.0, 10.0, 14.0],
            lid_min: [1.0 / 16.0, 9.0 / 16.0, 1.0 / 16.0],
            lid_max: [1.0, 14.0 / 16.0, 15.0 / 16.0],
            lid_size: [15.0, 5.0, 14.0],
            lock_min: [15.0 / 16.0, 7.0 / 16.0, 0.0],
            lock_max: [1.0, 11.0 / 16.0, 1.0 / 16.0],
            lock_size: [1.0, 4.0, 1.0],
        },
    }
}

fn log_chest_transform_debug(
    block_id: &str,
    chest_type: &str,
    template_identity: &str,
    facing: &str,
    rotation: f32,
) {
    if std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_none() {
        return;
    }
    println!(
        "[LBA_FULL_MODE_V2_CHEST] block={} type={} template={} facing={} applied_rotation={} local_front=north final_anchor=block_origin",
        block_id, chest_type, template_identity, facing, rotation,
    );
}

fn chest_rotation_degrees(facing: &str) -> f32 {
    match facing {
        "south" => 180.0,
        "west" => 90.0,
        "east" => 270.0,
        _ => 0.0,
    }
}

fn typed_multipart_when_matches(
    when: Option<&TypedMultipartWhen>,
    properties: &BTreeMap<String, String>,
) -> bool {
    let Some(when) = when else {
        return true;
    };
    let base_matches = multipart_property_map_matches(&when.properties, properties);
    let or_matches = when.or.is_empty()
        || when
            .or
            .iter()
            .any(|part| multipart_property_map_matches(part, properties));
    let and_matches = when
        .and
        .iter()
        .all(|part| multipart_property_map_matches(part, properties));
    base_matches && or_matches && and_matches
}

fn multipart_property_map_matches(
    expected: &BTreeMap<String, String>,
    properties: &BTreeMap<String, String>,
) -> bool {
    expected.iter().all(|(key, value)| {
        value
            .split('|')
            .any(|candidate| properties.get(key).map(String::as_str) == Some(candidate.trim()))
    })
}

fn when_matches(when: &Value, properties: &BTreeMap<String, String>) -> bool {
    let Some(object) = when.as_object() else {
        return true;
    };
    if let Some(or_values) = object.get("OR").and_then(Value::as_array) {
        return or_values
            .iter()
            .any(|value| when_matches(value, properties));
    }
    if let Some(and_values) = object.get("AND").and_then(Value::as_array) {
        return and_values
            .iter()
            .all(|value| when_matches(value, properties));
    }
    object.iter().all(|(key, expected)| {
        let expected = expected.as_str().unwrap_or_default();
        expected
            .split('|')
            .any(|value| properties.get(key).map(String::as_str) == Some(value.trim()))
    })
}

fn resolve_texture_id(value: &str, textures: &HashMap<String, String>) -> Option<String> {
    if !value.starts_with('#') {
        return Some(value.to_string());
    }
    let mut key = value.trim_start_matches('#');
    let mut seen = HashSet::new();
    while seen.insert(key.to_string()) {
        let next = textures.get(key)?;
        if !next.starts_with('#') {
            return Some(next.clone());
        }
        key = next.trim_start_matches('#');
    }
    None
}

fn element_face_quad(
    element: &Value,
    face_name: &str,
    model_ref: &OwnedModelRef,
    local: &str,
) -> Option<[[f32; 3]; 4]> {
    let from = json_f32x3(element.get("from")?)?;
    let to = json_f32x3(element.get("to")?)?;
    let mut vertices = face_vertices(face_name, from.map(|v| v / 16.0), to.map(|v| v / 16.0))?;
    if let Some(rotation) = element.get("rotation").and_then(Value::as_object) {
        let origin = rotation
            .get("origin")
            .and_then(json_f32x3)
            .map(|v| v.map(|n| n / 16.0))?;
        let axis = rotation.get("axis").and_then(Value::as_str).unwrap_or("");
        let angle = rotation.get("angle").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let rescale = rotation
            .get("rescale")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        for vertex in &mut vertices {
            *vertex = rotate_point(*vertex, origin, axis, angle, rescale);
        }
    }
    if model_ref.x != 0 {
        for vertex in &mut vertices {
            *vertex = rotate_point(*vertex, [0.5, 0.5, 0.5], "x", model_ref.x as f32, false);
        }
    }
    if model_ref.y != 0 {
        for vertex in &mut vertices {
            *vertex = rotate_point(
                *vertex,
                [0.5, 0.5, 0.5],
                "y",
                model_y_rotation_angle_for(local, model_ref.y),
                false,
            );
        }
    }
    Some(vertices.map(|v| v.map(|n| (n * 1_000_000.0).round() / 1_000_000.0)))
}

fn typed_element_face_quad(
    element: &ResolvedElement,
    face_name: &str,
    model_ref: &OwnedModelRef,
    local: &str,
    x_rotation_angle: f32,
) -> Option<[[f32; 3]; 4]> {
    let mut vertices = face_vertices(
        face_name,
        element.from.map(|v| v / 16.0),
        element.to.map(|v| v / 16.0),
    )?;
    if let Some(rotation) = &element.rotation {
        let origin = rotation.origin.map(|n| n / 16.0);
        for vertex in &mut vertices {
            *vertex = rotate_point(
                *vertex,
                origin,
                &rotation.axis,
                rotation.angle,
                rotation.rescale,
            );
        }
    }
    if model_ref.x != 0 {
        for vertex in &mut vertices {
            *vertex = rotate_point(*vertex, [0.5, 0.5, 0.5], "x", x_rotation_angle, false);
        }
    }
    if model_ref.y != 0 {
        for vertex in &mut vertices {
            *vertex = rotate_point(
                *vertex,
                [0.5, 0.5, 0.5],
                "y",
                model_y_rotation_angle_for(local, model_ref.y),
                false,
            );
        }
    }
    Some(vertices.map(|v| v.map(|n| (n * 1_000_000.0).round() / 1_000_000.0)))
}

fn face_vertices(face: &str, from: [f32; 3], to: [f32; 3]) -> Option<[[f32; 3]; 4]> {
    let [x0, y0, z0] = from;
    let [x1, y1, z1] = to;
    Some(match face {
        "down" => [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
        "up" => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        "north" => [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
        "south" => [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
        "west" => [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
        "east" => [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
        _ => return None,
    })
}

fn cuboid_template(
    material: MaterialImage,
    min: [f32; 3],
    max: [f32; 3],
    cullface: bool,
) -> Vec<TemplateQuad> {
    FACE_NAMES
        .iter()
        .filter_map(|face| {
            Some(TemplateQuad {
                vertices: face_vertices(face, min, max)?,
                material: material.clone(),
                uv: None,
                double_sided: false,
                cullface: cullface.then(|| (*face).to_string()),
            })
        })
        .collect()
}

fn template_quad_should_be_double_sided(local: &str, alpha: FullModeAlphaMode) -> bool {
    matches!(
        alpha,
        FullModeAlphaMode::Cutout | FullModeAlphaMode::Translucent
    ) && !is_single_sided_transparent_local(local)
}

fn is_single_sided_transparent_local(local: &str) -> bool {
    is_full_glass_block(local) || local.ends_with("_pane") || local.ends_with("_trapdoor")
}

fn crossed_plane_template(material: MaterialImage) -> Vec<TemplateQuad> {
    let planes = [
        [
            [0.95, 0.0, 0.05],
            [0.05, 0.0, 0.95],
            [0.05, 1.0, 0.95],
            [0.95, 1.0, 0.05],
        ],
        [
            [0.05, 0.0, 0.05],
            [0.95, 0.0, 0.95],
            [0.95, 1.0, 0.95],
            [0.05, 1.0, 0.05],
        ],
    ];
    planes
        .into_iter()
        .map(|vertices| TemplateQuad {
            vertices,
            material: material.clone(),
            uv: None,
            double_sided: true,
            cullface: None,
        })
        .collect()
}

fn full_mode_explicit_plant_template_local(local: &str) -> bool {
    matches!(local, "cave_vines" | "cave_vines_plant")
}

fn full_mode_special_plant_template_local(local: &str) -> bool {
    matches!(
        local,
        "big_dripleaf" | "big_dripleaf_stem" | "small_dripleaf" | "azalea" | "flowering_azalea"
    )
}

fn explicit_plant_texture_id(block_id: &str, properties: &BTreeMap<String, String>) -> String {
    let local = local_id(block_id);
    let texture_local = match local {
        "cave_vines" if properties.get("berries").map(String::as_str) == Some("true") => {
            "cave_vines_lit"
        }
        "cave_vines_plant" if properties.get("berries").map(String::as_str) == Some("true") => {
            "cave_vines_plant_lit"
        }
        "big_dripleaf" => "big_dripleaf_top",
        "small_dripleaf" => "small_dripleaf_top",
        "azalea" => "azalea_plant",
        "flowering_azalea" => "flowering_azalea_top",
        other => other,
    };
    format!("minecraft:block/{texture_local}")
}

fn standing_banner_quads(material: MaterialImage, rotation: i32) -> Vec<TemplateQuad> {
    let mut quads = Vec::new();
    quads.extend(cuboid_template(
        material.clone(),
        [7.25 / 16.0, -1.0, 7.25 / 16.0],
        [8.75 / 16.0, 1.0, 8.75 / 16.0],
        false,
    ));
    quads.extend(cuboid_template(
        material,
        [0.0, -1.0, 7.5 / 16.0],
        [1.0, 1.0, 8.5 / 16.0],
        false,
    ));
    let angle = (rotation.rem_euclid(16) as f32) * 22.5;
    if angle != 0.0 {
        for quad in &mut quads {
            for vertex in &mut quad.vertices {
                *vertex = rotate_point(*vertex, [0.5, 0.5, 0.5], "y", angle, false);
            }
        }
    }
    quads
}

fn wall_banner_quads(material: MaterialImage, facing: &str) -> Vec<TemplateQuad> {
    let (min, max) = match facing {
        "south" => ([0.0, -1.0, 15.0 / 16.0], [1.0, 1.0, 1.0]),
        "west" => ([0.0, -1.0, 0.0], [1.0 / 16.0, 1.0, 1.0]),
        "east" => ([15.0 / 16.0, -1.0, 0.0], [1.0, 1.0, 1.0]),
        _ => ([0.0, -1.0, 0.0], [1.0, 1.0, 1.0 / 16.0]),
    };
    cuboid_template(material, min, max, false)
}

fn banner_color_local(local: &str) -> Option<&str> {
    let base = local
        .strip_suffix("_wall_banner")
        .or_else(|| local.strip_suffix("_banner"))?;
    Some(if base.is_empty() { "white" } else { base })
}

fn trace_chain_banner_chain_template(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    source: &str,
    model_ref: &OwnedModelRef,
    element_count: usize,
    quads: &[TemplateQuad],
) {
    if !env_flag_enabled("LBA_CHAIN_BANNER_TRACE")
        && std::env::var_os("LBA_FULL_MODE_V2_CHAIN_BANNER_DEBUG").is_none()
        && std::env::var_os("LBA_FULL_MODE_V2_TBL_DEBUG").is_none()
    {
        return;
    }
    let axis = properties.get("axis").map(String::as_str).unwrap_or("y");
    let sample_id = match axis {
        "x" => "CB02",
        "z" => "CB03",
        _ => "CB01",
    };
    let bounds = template_quads_bounds(quads)
        .map(|(min, max)| format_bounds3(min, max))
        .unwrap_or_else(|| "<empty>".to_string());
    println!(
        "[LBA_CHAIN_BANNER_TRACE_20260424A] stage=chain_template sample_id={} state={} axis={} source={} model={} model_x={} model_y={} element_count={} quad_count={} bounds={} final_culled=false return_reason=template_generated",
        sample_id,
        state_key(block_id, properties),
        axis,
        source,
        model_ref.model,
        model_ref.x,
        model_ref.y,
        element_count,
        quads.len(),
        bounds,
    );
    for (index, quad) in quads.iter().enumerate() {
        let (min, max) = quad_bounds(&quad.vertices);
        println!(
            "[LBA_CHAIN_BANNER_TRACE_20260424A] stage=chain_quad sample_id={} quad={} cullface={} bounds={} vertices={:?} final_culled=false return_reason=template_generated",
            sample_id,
            index,
            quad.cullface.as_deref().unwrap_or("<none>"),
            format_bounds3(min, max),
            quad.vertices,
        );
    }
}

fn trace_chain_banner_banner_template(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    quads: &[TemplateQuad],
) {
    if !env_flag_enabled("LBA_CHAIN_BANNER_TRACE")
        && std::env::var_os("LBA_FULL_MODE_V2_CHAIN_BANNER_DEBUG").is_none()
    {
        return;
    }
    let local = local_id(block_id);
    let (sample_id, kind, pose) = if local.ends_with("_wall_banner") {
        let facing = properties.get("facing").map(String::as_str).unwrap_or("-");
        let sample_id = match facing {
            "east" => "CB07",
            "south" => "CB08",
            "west" => "CB09",
            _ => "CB06",
        };
        (sample_id, "wall", format!("facing={facing}"))
    } else if local == "red_banner" {
        (
            "CB05",
            "standing",
            format!(
                "rotation={}",
                properties
                    .get("rotation")
                    .map(String::as_str)
                    .unwrap_or("-")
            ),
        )
    } else {
        (
            "CB04",
            "standing",
            format!(
                "rotation={}",
                properties
                    .get("rotation")
                    .map(String::as_str)
                    .unwrap_or("-")
            ),
        )
    };
    let bounds = template_quads_bounds(quads)
        .map(|(min, max)| format_bounds3(min, max))
        .unwrap_or_else(|| "<empty>".to_string());
    println!(
        "[LBA_CHAIN_BANNER_TRACE_20260424A] stage=banner_template sample_id={} state={} family=banner banner_kind={} {} offset=block_local_surface transform=manual_template quad_count={} bounds={} pattern_nbt_supported=false final_culled=false return_reason=template_generated",
        sample_id,
        state_key(block_id, properties),
        kind,
        pose,
        quads.len(),
        bounds,
    );
}

fn trace_plant_template_classification(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    class: &str,
    quad_count: usize,
) {
    if !env_flag_enabled("LBA_PLANT_TRACE") && !env_flag_enabled("LBA_TEMPLATE_TRACE") {
        return;
    }
    println!(
        "[LBA_PLANT_TRACE_20260424A] stage=build_state_template state={} family={} template_path={} quad_count={} waterlogged={} final_culled=false return_reason=template_generated",
        state_key(block_id, properties),
        class,
        if class == "special_plant_typed_model" {
            "typed_model_template"
        } else {
            "explicit_crossed_planes_template"
        },
        quad_count,
        properties
            .get("waterlogged")
            .map(String::as_str)
            .unwrap_or("false"),
    );
}

fn template_quads_bounds(quads: &[TemplateQuad]) -> Option<([f32; 3], [f32; 3])> {
    let first = quads.first()?;
    let (mut min, mut max) = quad_bounds(&first.vertices);
    for quad in quads.iter().skip(1) {
        let (quad_min, quad_max) = quad_bounds(&quad.vertices);
        for index in 0..3 {
            min[index] = min[index].min(quad_min[index]);
            max[index] = max[index].max(quad_max[index]);
        }
    }
    Some((min, max))
}

fn quad_bounds(vertices: &[[f32; 3]; 4]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in vertices {
        for index in 0..3 {
            min[index] = min[index].min(vertex[index]);
            max[index] = max[index].max(vertex[index]);
        }
    }
    (min, max)
}

fn piston_debug_label_face_vertices(face: &str) -> [[f32; 3]; 4] {
    let x0 = 0.19;
    let x1 = 0.81;
    let y0 = 0.39;
    let y1 = 0.61;
    let z0 = 0.19;
    let z1 = 0.81;
    let eps = 0.025;
    match face {
        "down" => face_vertices("down", [x0, -eps, y0], [x1, -eps, y1]),
        "up" => face_vertices("up", [x0, 1.0 + eps, y0], [x1, 1.0 + eps, y1]),
        "north" => face_vertices("north", [x0, y0, -eps], [x1, y1, -eps]),
        "south" => face_vertices("south", [x0, y0, 1.0 + eps], [x1, y1, 1.0 + eps]),
        "west" => face_vertices("west", [-eps, y0, z0], [-eps, y1, z1]),
        "east" => face_vertices("east", [1.0 + eps, y0, z0], [1.0 + eps, y1, z1]),
        _ => None,
    }
    .expect("piston debug face is valid")
}

fn observer_debug_label_face_vertices(face: &str) -> [[f32; 3]; 4] {
    let x0 = 0.19;
    let x1 = 0.81;
    let y0 = 0.39;
    let y1 = 0.61;
    let z0 = 0.19;
    let z1 = 0.81;
    let eps = 0.025;
    match face {
        "down" => face_vertices("down", [x0, -eps, y0], [x1, -eps, y1]),
        "up" => face_vertices("up", [x0, 1.0 + eps, y0], [x1, 1.0 + eps, y1]),
        "north" => face_vertices("north", [x0, y0, -eps], [x1, y1, -eps]),
        "south" => face_vertices("south", [x0, y0, 1.0 + eps], [x1, y1, 1.0 + eps]),
        "west" => face_vertices("west", [-eps, y0, z0], [-eps, y1, z1]),
        "east" => face_vertices("east", [1.0 + eps, y0, z0], [1.0 + eps, y1, z1]),
        _ => None,
    }
    .expect("observer debug face is valid")
}

fn chain_banner_debug_label_vertices() -> [[f32; 3]; 4] {
    face_vertices("up", [0.12, 1.055, 0.12], [0.88, 1.055, 0.88])
        .expect("chain/banner debug label face is valid")
}

fn observer_debug_label_face_uv(face: &str) -> [[f32; 2]; 4] {
    let base = typed_face_quad_uv(face);
    match face {
        "up" | "down" => apply_quad_uv_transform(base, 270, false, false),
        "west" | "east" => apply_quad_uv_transform(base, 0, true, false),
        _ => base,
    }
}

fn observer_model_base_quad_uv(
    face: &str,
    texture_id: &str,
    face_uv: Option<[f32; 4]>,
    rotation: i32,
) -> [[f32; 2]; 4] {
    let _ = texture_id;
    let _ = face_uv;
    let _ = rotation;
    typed_face_quad_uv(face)
}

fn observer_real_quad_uv(_face: &str, texture_id: &str, base: [[f32; 2]; 4]) -> [[f32; 2]; 4] {
    match observer_texture_family(texture_id) {
        Some("observer_front") => apply_quad_uv_transform(base, 360, false, false),
        _ => base,
    }
}

fn observer_effective_texture_id<'a>(
    properties: &BTreeMap<String, String>,
    face_name: &str,
    texture_id: &'a str,
) -> &'a str {
    if properties.get("powered").map(String::as_str) == Some("true")
        && matches!(face_name, "west" | "east")
    {
        match texture_id {
            "block/observer_side" | "minecraft:block/observer_side" => {
                return "minecraft:block/observer_side_on_lba_mirror";
            }
            "block/observer_side_on" | "minecraft:block/observer_side_on" => {
                return "minecraft:block/observer_side_on_lba_mirror";
            }
            _ => {}
        }
    }
    observer_patched_texture_id(texture_id)
}

fn observer_patched_texture_id(texture_id: &str) -> &str {
    match texture_id {
        "block/observer_top" | "minecraft:block/observer_top" => {
            "minecraft:block/observer_top_lba_rot90"
        }
        "block/observer_top_on" | "minecraft:block/observer_top_on" => {
            "minecraft:block/observer_top_on_lba_rot90"
        }
        "block/observer_side" | "minecraft:block/observer_side" => {
            "minecraft:block/observer_side_lba_mirror"
        }
        "block/observer_side_on" | "minecraft:block/observer_side_on" => {
            "minecraft:block/observer_side_on_lba_mirror"
        }
        "block/observer_front" | "minecraft:block/observer_front" => {
            "minecraft:block/observer_front_lba_rot90"
        }
        "block/observer_front_on" | "minecraft:block/observer_front_on" => {
            "minecraft:block/observer_front_on_lba_rot90"
        }
        _ => texture_id,
    }
}

fn observer_texture_family(value: &str) -> Option<&'static str> {
    if value.contains("observer_top_on") {
        Some("observer_top_on")
    } else if value.contains("observer_top") {
        Some("observer_top")
    } else if value.contains("observer_side_on") {
        Some("observer_side_on")
    } else if value.contains("observer_side") {
        Some("observer_side")
    } else if value.contains("observer_front") {
        Some("observer_front")
    } else if value.contains("observer_back_on") {
        Some("observer_back_on")
    } else if value.contains("observer_back") {
        Some("observer_back")
    } else {
        None
    }
}

fn observer_family_probe_requested(family: &str) -> bool {
    if std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG").is_none() {
        return false;
    }
    let Some(requested) = std::env::var("LBA_OBSERVER_FAMILY_PROBE").ok() else {
        return false;
    };
    let requested = requested.trim().to_ascii_lowercase();
    requested == family
}

fn observer_family_probe_material(rep: &str, family: &str) -> MaterialImage {
    let (a, b) = match family {
        "observer_top" => (Rgba([255, 0, 0, 255]), Rgba([255, 255, 255, 255])),
        "observer_top_on" => (Rgba([255, 128, 0, 255]), Rgba([255, 255, 255, 255])),
        "observer_side" => (Rgba([0, 255, 0, 255]), Rgba([0, 0, 0, 255])),
        "observer_side_on" => (Rgba([0, 255, 255, 255]), Rgba([0, 0, 0, 255])),
        "observer_front" => (Rgba([255, 0, 255, 255]), Rgba([255, 255, 255, 255])),
        "observer_back" => (Rgba([255, 255, 0, 255]), Rgba([0, 0, 0, 255])),
        "observer_back_on" => (Rgba([0, 128, 255, 255]), Rgba([255, 255, 255, 255])),
        _ => (Rgba([255, 0, 255, 255]), Rgba([0, 0, 0, 255])),
    };
    let mut image = RgbaImage::from_pixel(16, 16, a);
    for y in 0..16 {
        for x in 0..16 {
            if ((x / 4) + (y / 4)) % 2 == 1 {
                image.put_pixel(x, y, b);
            }
        }
    }
    MaterialImage {
        key: format!("generated:observer_family_probe:{rep}:{family}"),
        image,
        alpha_mode: FullModeAlphaMode::Opaque,
    }
}

fn observer_owner_probe_target(
    properties: &BTreeMap<String, String>,
    world_face: &str,
) -> Option<String> {
    std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG")?;
    let requested = std::env::var("LBA_OBSERVER_OWNER_PROBE").ok()?;
    let sample = observer_fixture_sample_code(properties)?;
    let face_index = observer_world_face_debug_index(world_face)?;
    let probe = format!("{sample}-{face_index}");
    observer_owner_probe_matches(&requested, &probe).then_some(probe)
}

fn observer_owner_probe_matches(requested: &str, probe: &str) -> bool {
    normalize_observer_probe_id(requested) == normalize_observer_probe_id(probe)
}

fn normalize_observer_probe_id(value: &str) -> String {
    let trimmed = value.trim().to_ascii_uppercase();
    let trimmed = trimmed
        .strip_prefix('O')
        .map(str::to_string)
        .unwrap_or(trimmed);
    let mut parts = trimmed.splitn(2, '-');
    let sample = parts.next().unwrap_or_default();
    let face = parts.next().unwrap_or_default();
    let sample_num = sample.parse::<usize>().ok().unwrap_or(0);
    let face_num = face.parse::<usize>().ok().unwrap_or(0);
    format!("{sample_num:03}-{face_num}")
}

fn observer_fixture_sample_code(properties: &BTreeMap<String, String>) -> Option<&'static str> {
    match (
        properties.get("facing").map(String::as_str).unwrap_or(""),
        properties.get("powered").map(String::as_str).unwrap_or(""),
    ) {
        ("north", "false") => Some("001"),
        ("east", "false") => Some("002"),
        ("south", "false") => Some("003"),
        ("west", "false") => Some("004"),
        ("up", "false") => Some("005"),
        ("down", "false") => Some("006"),
        ("north", "true") => Some("007"),
        ("east", "true") => Some("008"),
        ("south", "true") => Some("009"),
        ("west", "true") => Some("010"),
        ("up", "true") => Some("011"),
        ("down", "true") => Some("012"),
        _ => None,
    }
}

fn observer_world_face_debug_index(world_face: &str) -> Option<usize> {
    OBSERVER_DEBUG_FACE_ORDER
        .iter()
        .position(|candidate| *candidate == world_face)
        .map(|index| index + 1)
}

fn observer_owner_probe_material(
    probe_id: &str,
    state: &str,
    local_face: &str,
    world_face: &str,
) -> MaterialImage {
    let color = match normalize_observer_probe_id(probe_id).as_str() {
        "001-1" => Rgba([255, 0, 0, 255]),
        "001-6" => Rgba([0, 255, 0, 255]),
        "005-3" => Rgba([0, 128, 255, 255]),
        "005-6" => Rgba([255, 255, 0, 255]),
        _ => Rgba([255, 0, 255, 255]),
    };
    MaterialImage {
        key: format!(
            "generated:observer_owner_probe:{}:{}:{}:{}",
            probe_id, state, local_face, world_face
        ),
        image: RgbaImage::from_pixel(16, 16, color),
        alpha_mode: FullModeAlphaMode::Opaque,
    }
}

fn observer_front_probe_requested(family: &str) -> bool {
    if !matches!(family, "observer_front") {
        return false;
    }
    std::env::var("LBA_OBSERVER_FRONT_PROBE")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false)
}

fn observer_front_probe_material(
    state: &str,
    local_face: &str,
    world_face: &str,
    family: &str,
) -> MaterialImage {
    let mut image = RgbaImage::from_pixel(64, 64, Rgba([24, 24, 24, 255]));

    for y in 0..64 {
        for x in 0..64 {
            let pixel = if x < 16 && y < 16 {
                Rgba([255, 215, 0, 255])
            } else if x >= 48 && y < 16 {
                Rgba([255, 64, 64, 255])
            } else if x < 16 && y >= 48 {
                Rgba([64, 160, 255, 255])
            } else if x >= 48 && y >= 48 {
                Rgba([64, 255, 128, 255])
            } else if y < 8 {
                Rgba([255, 255, 255, 255])
            } else if x >= 56 {
                Rgba([255, 64, 64, 255])
            } else if y >= 56 {
                Rgba([64, 160, 255, 255])
            } else if x < 8 {
                Rgba([255, 215, 0, 255])
            } else {
                Rgba([24, 24, 24, 255])
            };
            image.put_pixel(x, y, pixel);
        }
    }

    // Large upward arrow so the final visual direction is unambiguous.
    for y in 10..50 {
        for dx in -4i32..=4 {
            let x = 32i32 + dx;
            image.put_pixel(x as u32, y as u32, Rgba([255, 255, 255, 255]));
        }
    }
    for row in 0..18 {
        let y = 10 + row;
        let half = 14 - row.min(14);
        let start = 32i32 - half;
        let end = 32i32 + half;
        for x in start..=end {
            if (0..64).contains(&x) {
                image.put_pixel(x as u32, y as u32, Rgba([255, 255, 255, 255]));
            }
        }
    }

    // Extra asymmetric markers on the right and bottom to distinguish rotation from mirroring.
    for y in 22..42 {
        for x in 46..54 {
            image.put_pixel(x, y, Rgba([255, 64, 64, 255]));
        }
    }
    for y in 46..54 {
        for x in 22..42 {
            image.put_pixel(x, y, Rgba([64, 160, 255, 255]));
        }
    }

    MaterialImage {
        key: format!(
            "generated:observer_front_probe:{}:{}:{}:{}",
            state, local_face, world_face, family
        ),
        image,
        alpha_mode: FullModeAlphaMode::Opaque,
    }
}

fn observer_semantic_trace_target(
    properties: &BTreeMap<String, String>,
    world_face: &str,
) -> Option<String> {
    std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG")?;
    let probe = observer_owner_probe_target(properties, world_face).or_else(|| {
        let sample = observer_fixture_sample_code(properties)?;
        let face_index = observer_world_face_debug_index(world_face)?;
        Some(format!("{sample}-{face_index}"))
    })?;
    matches!(probe.as_str(), "001-1" | "001-6" | "005-3" | "005-6").then_some(probe)
}

fn observer_detect_resource_target(
    properties: &BTreeMap<String, String>,
    local_face: &str,
    world_face: &str,
) -> Option<String> {
    std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG")?;
    let sample = observer_fixture_sample_code(properties)?;
    if !matches!(sample, "001" | "003" | "005" | "006") {
        return None;
    }
    let facing = properties.get("facing").map(String::as_str).unwrap_or("");
    (local_face == "north" && world_face == facing).then_some(sample.to_string())
}

fn observer_detect_model_target(
    properties: &BTreeMap<String, String>,
    world_face: &str,
) -> Option<String> {
    std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG")?;
    let sample = observer_fixture_sample_code(properties)?;
    if !matches!(sample, "001" | "003" | "005" | "006") {
        return None;
    }
    let facing = properties.get("facing").map(String::as_str).unwrap_or("");
    (world_face == facing).then_some(sample.to_string())
}

fn observer_debug_target_text(text: &str) -> bool {
    matches!(text, "O05-1" | "O06-1" | "O11-1" | "O12-1")
}

fn observer_debug_target_material_key(key: &str) -> bool {
    key.contains(":O05-1:")
        || key.contains(":O06-1:")
        || key.contains(":O11-1:")
        || key.contains(":O12-1:")
}

fn observer_debug_face_semantic(facing: &str, world_face: &str) -> (&'static str, &'static str) {
    match observer_debug_local_face_for_world_face(facing, world_face).unwrap_or(world_face) {
        "north" => ("detect", "侦测"),
        "south" => ("output", "输出"),
        "west" | "east" => ("side", "侧边"),
        "up" => ("top", "顶部"),
        "down" => ("bottom", "底部"),
        _ => ("side", "侧边"),
    }
}

fn observer_debug_local_face_for_world_face(
    facing: &str,
    world_face: &str,
) -> Option<&'static str> {
    let (x, y) = observer_debug_model_rotation(facing);
    FACE_NAMES
        .iter()
        .copied()
        .find(|local_face| rotate_direction(local_face, x, y) == world_face)
}

fn observer_debug_model_rotation(facing: &str) -> (i32, i32) {
    match facing {
        "down" => (270, 0),
        "up" => (90, 0),
        "north" => (0, 0),
        "south" => (0, 180),
        "west" => (0, 90),
        "east" => (0, 270),
        _ => (0, 0),
    }
}

fn chest_debug_label_face_vertices(face: &str) -> [[f32; 3]; 4] {
    let x0 = 0.19;
    let x1 = 0.81;
    let y0 = 0.39;
    let y1 = 0.61;
    let z0 = 0.19;
    let z1 = 0.81;
    let eps = 0.025;
    match face {
        "down" => face_vertices("down", [x0, -eps, y0], [x1, -eps, y1]),
        "up" => face_vertices("up", [x0, 1.0 + eps, y0], [x1, 1.0 + eps, y1]),
        "north" => face_vertices("north", [x0, y0, -eps], [x1, y1, -eps]),
        "south" => face_vertices("south", [x0, y0, 1.0 + eps], [x1, y1, 1.0 + eps]),
        "west" => face_vertices("west", [-eps, y0, z0], [-eps, y1, z1]),
        "east" => face_vertices("east", [1.0 + eps, y0, z0], [1.0 + eps, y1, z1]),
        _ => None,
    }
    .expect("chest debug face is valid")
}

fn rotate_point(
    point: [f32; 3],
    origin: [f32; 3],
    axis: &str,
    angle: f32,
    rescale: bool,
) -> [f32; 3] {
    let radians = angle.to_radians();
    let (sin_a, cos_a) = radians.sin_cos();
    let mut x = point[0] - origin[0];
    let mut y = point[1] - origin[1];
    let mut z = point[2] - origin[2];
    if rescale && cos_a.abs() > 0.0001 {
        let scale = 1.0 / cos_a.abs();
        match axis {
            "x" => {
                y *= scale;
                z *= scale;
            }
            "y" => {
                x *= scale;
                z *= scale;
            }
            "z" => {
                x *= scale;
                y *= scale;
            }
            _ => {}
        }
    }
    let rotated = match axis {
        "x" => [x, y * cos_a - z * sin_a, y * sin_a + z * cos_a],
        "y" => [x * cos_a + z * sin_a, y, -x * sin_a + z * cos_a],
        "z" => [x * cos_a - y * sin_a, x * sin_a + y * cos_a, z],
        _ => [x, y, z],
    };
    [
        rotated[0] + origin[0],
        rotated[1] + origin[1],
        rotated[2] + origin[2],
    ]
}

fn model_y_rotation_angle_for(local: &str, y: i32) -> f32 {
    if full_mode_positive_wall_attachment_rotation_local(local) {
        y as f32
    } else {
        -(y as f32)
    }
}

fn full_mode_positive_wall_attachment_rotation_local(local: &str) -> bool {
    local == "lever" || local.ends_with("_trapdoor")
}

fn full_mode_wall_attached_sign_local(local: &str) -> bool {
    local.ends_with("_wall_sign") || local.ends_with("_wall_hanging_sign")
}

fn full_mode_wall_sign_local(local: &str) -> bool {
    local.ends_with("_wall_sign")
}

fn full_mode_stripped_log_or_wood_local(local: &str) -> bool {
    local.starts_with("stripped_") && (local.ends_with("_log") || local.ends_with("_wood"))
}

fn is_basic_rail_family(local: &str) -> bool {
    matches!(
        local,
        "rail" | "powered_rail" | "detector_rail" | "activator_rail"
    )
}

fn is_static_piston_family(local: &str) -> bool {
    matches!(local, "piston" | "sticky_piston" | "piston_head")
}

fn is_chest_debug_family(local: &str) -> bool {
    matches!(local, "chest" | "trapped_chest" | "ender_chest")
}

fn is_piston_body_side_texture(local: &str, texture_id: &str) -> bool {
    matches!(local, "piston" | "sticky_piston")
        && texture_id
            .rsplit(':')
            .next()
            .unwrap_or(texture_id)
            .contains("piston_side")
}

#[derive(Clone, Copy)]
struct QuadUvTransform {
    rotation: i32,
    flip_u: bool,
    flip_v: bool,
    custom: bool,
}

#[derive(Clone, Copy)]
struct PistonBodySideUvDebug {
    normal: [f32; 3],
    u_dir: [f32; 3],
    v_dir: [f32; 3],
    transform: QuadUvTransform,
    body_side_rotate180: bool,
}

fn auto_piston_body_side_uv(
    face_name: &str,
    world_face: &str,
    vertices: &[[f32; 3]; 4],
    head_local_side: Option<&'static str>,
) -> Option<([[f32; 2]; 4], PistonBodySideUvDebug)> {
    let source_uv = typed_face_quad_uv(face_name);
    let normal = quad_normal(vertices)?;
    let u_dir = quad_uv_axis_direction(vertices, &source_uv, 0)?;
    let v_dir = quad_uv_axis_direction(vertices, &source_uv, 1)?;
    let mut uv = project_quad_uv(vertices, u_dir, v_dir)?;
    let body_side_rotate180 = matches!(
        head_local_side,
        Some("head_local_down" | "head_local_right")
    );
    if body_side_rotate180 {
        uv = apply_quad_uv_transform(uv, 180, false, false);
    }
    let transform = classify_quad_uv_transform(&typed_face_quad_uv(world_face), &uv);
    Some((
        uv,
        PistonBodySideUvDebug {
            normal,
            u_dir,
            v_dir,
            transform,
            body_side_rotate180,
        },
    ))
}

fn piston_head_local_side(facing: &str, world_face: &str) -> Option<&'static str> {
    let (head_local_down, head_local_right, head_local_up, head_local_left) = match facing {
        "north" => ("down", "west", "up", "east"),
        "east" => ("down", "north", "up", "south"),
        "south" => ("down", "east", "up", "west"),
        "west" => ("down", "south", "up", "north"),
        "up" => ("south", "west", "north", "east"),
        "down" => ("north", "west", "south", "east"),
        _ => return None,
    };
    if world_face == head_local_down {
        Some("head_local_down")
    } else if world_face == head_local_right {
        Some("head_local_right")
    } else if world_face == head_local_up {
        Some("head_local_up")
    } else if world_face == head_local_left {
        Some("head_local_left")
    } else {
        None
    }
}

fn piston_body_side_head_local_side(
    local: &str,
    properties: &BTreeMap<String, String>,
    world_face: &str,
) -> Option<&'static str> {
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("north");
    if matches!(local, "piston" | "sticky_piston")
        && properties
            .get("__lba_standalone_vertical_base")
            .map(String::as_str)
            == Some("true")
        && matches!(facing, "up" | "down")
    {
        let extended = properties.get("extended").map(String::as_str) == Some("true");
        let standalone_vertical_mapping = match (facing, extended) {
            ("up", false) => Some(("north", "west", "south", "east")),
            ("down", false) => Some(("south", "west", "north", "east")),
            ("up", true) => Some(("north", "west", "south", "east")),
            ("down", true) => Some(("south", "west", "north", "east")),
            _ => None,
        };
        if let Some((head_local_down, head_local_right, head_local_up, head_local_left)) =
            standalone_vertical_mapping
        {
            return piston_head_local_side_from_faces(
                world_face,
                head_local_down,
                head_local_right,
                head_local_up,
                head_local_left,
            );
        }
    }
    if matches!(local, "piston" | "sticky_piston")
        && properties.get("extended").map(String::as_str) == Some("true")
        && properties
            .get("__lba_standalone_vertical_base")
            .map(String::as_str)
            != Some("true")
    {
        let vertical_extended_base_mapping = match facing {
            "up" => Some(("north", "west", "south", "east")),
            "down" => Some(("south", "west", "north", "east")),
            _ => None,
        };
        if let Some((head_local_down, head_local_right, head_local_up, head_local_left)) =
            vertical_extended_base_mapping
        {
            return piston_head_local_side_from_faces(
                world_face,
                head_local_down,
                head_local_right,
                head_local_up,
                head_local_left,
            );
        }
    }
    piston_head_local_side(facing, world_face)
}

fn piston_head_local_side_from_faces(
    world_face: &str,
    head_local_down: &'static str,
    head_local_right: &'static str,
    head_local_up: &'static str,
    head_local_left: &'static str,
) -> Option<&'static str> {
    if world_face == head_local_down {
        Some("head_local_down")
    } else if world_face == head_local_right {
        Some("head_local_right")
    } else if world_face == head_local_up {
        Some("head_local_up")
    } else if world_face == head_local_left {
        Some("head_local_left")
    } else {
        None
    }
}

fn piston_vertical_pair_orientation_branch(
    local: &str,
    properties: &BTreeMap<String, String>,
) -> bool {
    let vertical = matches!(
        properties.get("facing").map(String::as_str),
        Some("up" | "down")
    );
    if !vertical {
        return false;
    }
    local == "piston_head"
        || (matches!(local, "piston" | "sticky_piston")
            && properties.get("extended").map(String::as_str) == Some("true"))
}

fn piston_model_x_rotation_angle(
    local: &str,
    properties: &BTreeMap<String, String>,
    model_ref: &OwnedModelRef,
) -> f32 {
    if matches!(local, "piston" | "sticky_piston")
        && properties
            .get("__lba_standalone_vertical_base")
            .map(String::as_str)
            == Some("true")
        && matches!(
            properties.get("facing").map(String::as_str),
            Some("up" | "down")
        )
    {
        return -(model_ref.x as f32);
    }
    if piston_vertical_pair_orientation_branch(local, properties) {
        -(model_ref.x as f32)
    } else {
        model_ref.x as f32
    }
}

fn piston_pair_relation_debug(local: &str, properties: &BTreeMap<String, String>) -> &'static str {
    if !piston_vertical_pair_orientation_branch(local, properties) {
        return "-";
    }
    match (
        local,
        properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north"),
    ) {
        ("piston_head", "up") => "head_front=up,base_expected=down",
        ("piston_head", "down") => "head_front=down,base_expected=up",
        ("piston" | "sticky_piston", "up") => "base_front=up,head_expected=up",
        ("piston" | "sticky_piston", "down") => "base_front=down,head_expected=down",
        _ => "-",
    }
}

fn is_full_glass_block(local: &str) -> bool {
    matches!(
        local,
        "glass"
            | "white_stained_glass"
            | "light_gray_stained_glass"
            | "gray_stained_glass"
            | "black_stained_glass"
            | "brown_stained_glass"
            | "red_stained_glass"
            | "orange_stained_glass"
            | "yellow_stained_glass"
            | "lime_stained_glass"
            | "green_stained_glass"
            | "cyan_stained_glass"
            | "light_blue_stained_glass"
            | "blue_stained_glass"
            | "purple_stained_glass"
            | "magenta_stained_glass"
            | "pink_stained_glass"
            | "tinted_glass"
    )
}

fn typed_face_quad_uv(face: &str) -> [[f32; 2]; 4] {
    match face {
        "down" => [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        "up" => [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        "north" => [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
        "south" => [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        "west" => [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        "east" => [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
        _ => [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    }
}

fn hopper_texture_ref_override<'a>(
    local: &str,
    model_name: &str,
    element_index: usize,
    face_name: &str,
    xk_face: Option<&'a ResolvedFace>,
    face: &'a ResolvedFace,
) -> &'a str {
    if hopper_big_bottom_face(local, model_name, element_index, face_name) {
        &face.texture
    } else {
        xk_face
            .map(|candidate| candidate.texture.as_str())
            .unwrap_or(&face.texture)
    }
}

fn hopper_big_bottom_face(
    local: &str,
    model_name: &str,
    element_index: usize,
    face_name: &str,
) -> bool {
    local == "hopper"
        && matches!(
            model_name,
            "minecraft:block/hopper"
                | "minecraft:block/hopper_side"
                | "minecraft:block/hopper_on"
                | "minecraft:block/hopper_side_on"
        )
        && element_index == 0
        && face_name == "down"
}

fn hopper_uses_xk_typed_model_base(local: &str, model_name: &str) -> bool {
    local == "hopper"
        && matches!(
            model_name,
            "minecraft:block/hopper_on" | "minecraft:block/hopper_side_on"
        )
}

fn hopper_real_material_texture_id(texture_id: &str) -> &str {
    match texture_id {
        "block/hopper_inside_on"
        | "block/hopper_inside2"
        | "block/hopper_inside2_on"
        | "minecraft:block/hopper_inside_on"
        | "minecraft:block/hopper_inside2"
        | "minecraft:block/hopper_inside2_on" => "minecraft:block/hopper_inside",
        _ => texture_id,
    }
}

fn hopper_quad_uv_override(
    local: &str,
    properties: &BTreeMap<String, String>,
    model_name: &str,
    element_index: usize,
    face_name: &str,
    uv: [[f32; 2]; 4],
) -> [[f32; 2]; 4] {
    let facing = properties.get("facing").map(String::as_str);
    let enabled = properties
        .get("enabled")
        .map(String::as_str)
        .unwrap_or("true");
    if local == "hopper"
        && ((enabled == "true"
            && (model_name == "minecraft:block/hopper"
                && matches!(
                    (element_index, face_name, facing),
                    (2, "east", Some("down"))
                )))
            || (enabled == "true"
                && (model_name == "minecraft:block/hopper_side"
                    && matches!(
                        (element_index, face_name, facing),
                        (
                            2,
                            "east" | "south" | "north",
                            Some("north" | "south" | "east" | "west")
                        )
                    )))
            || (enabled == "false"
                && ((model_name == "minecraft:block/hopper_on"
                    && matches!(
                        (element_index, face_name, facing),
                        (2, "east", Some("down"))
                    ))
                    || (model_name == "minecraft:block/hopper_side_on"
                        && matches!(
                            (element_index, face_name, facing),
                            (2, "east", Some("north" | "south" | "east" | "west"))
                        )))))
    {
        return apply_quad_uv_transform(uv, 0, true, false);
    }
    if local == "hopper"
        && model_name == "minecraft:block/hopper_side"
        && matches!(facing, Some("north" | "south"))
        && matches!((element_index, face_name), (1, "south"))
    {
        return apply_quad_uv_transform(uv, 180, false, false);
    }
    if local == "hopper"
        && enabled == "true"
        && ((model_name == "minecraft:block/hopper"
            && matches!(
                (element_index, face_name, facing),
                (2, "south", Some("down"))
            ))
            || (model_name == "minecraft:block/hopper_side"
                && matches!(
                    (element_index, face_name, facing),
                    (2, "up", Some("north" | "south" | "east" | "west"))
                )))
    {
        return apply_quad_uv_transform(uv, 180, false, false);
    }
    if local == "hopper"
        && enabled == "false"
        && ((model_name == "minecraft:block/hopper_on"
            && matches!(
                (element_index, face_name, facing),
                (2, "east", Some("down"))
            ))
            || (model_name == "minecraft:block/hopper_side_on"
                && matches!(
                    (element_index, face_name, facing),
                    (2, "north", Some("north" | "south" | "east" | "west"))
                )))
    {
        return apply_quad_uv_transform(uv, 180, false, false);
    }
    uv
}

fn hopper_default_face_uv(element: &ResolvedElement, face_name: &str) -> Option<[f32; 4]> {
    let [x0, y0, z0] = element.from;
    let [x1, y1, z1] = element.to;
    Some(match face_name {
        "down" => [x0, 16.0 - z1, x1, 16.0 - z0],
        "up" => [x0, z0, x1, z1],
        "north" => [16.0 - x1, 16.0 - y1, 16.0 - x0, 16.0 - y0],
        "south" => [x0, 16.0 - y1, x1, 16.0 - y0],
        "west" => [z0, 16.0 - y1, z1, 16.0 - y0],
        "east" => [16.0 - z1, 16.0 - y1, 16.0 - z0, 16.0 - y0],
        _ => return None,
    })
}

fn quad_uv_axis_direction(
    vertices: &[[f32; 3]; 4],
    uv: &[[f32; 2]; 4],
    axis: usize,
) -> Option<[f32; 3]> {
    let mut low = [0.0_f32; 3];
    let mut high = [0.0_f32; 3];
    let mut low_count = 0.0_f32;
    let mut high_count = 0.0_f32;
    for (vertex, coord) in vertices.iter().zip(uv.iter()) {
        if coord[axis] >= 0.5 {
            high = add3(high, *vertex);
            high_count += 1.0;
        } else {
            low = add3(low, *vertex);
            low_count += 1.0;
        }
    }
    if low_count == 0.0 || high_count == 0.0 {
        return None;
    }
    normalize3(sub3(
        scale3(high, 1.0 / high_count),
        scale3(low, 1.0 / low_count),
    ))
}

fn project_quad_uv(
    vertices: &[[f32; 3]; 4],
    u_dir: [f32; 3],
    v_dir: [f32; 3],
) -> Option<[[f32; 2]; 4]> {
    let mut u_values = [0.0_f32; 4];
    let mut v_values = [0.0_f32; 4];
    for (index, vertex) in vertices.iter().enumerate() {
        u_values[index] = dot3(*vertex, u_dir);
        v_values[index] = dot3(*vertex, v_dir);
    }
    let u_min = u_values.into_iter().fold(f32::INFINITY, f32::min);
    let u_max = u_values.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let v_min = v_values.into_iter().fold(f32::INFINITY, f32::min);
    let v_max = v_values.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let u_span = u_max - u_min;
    let v_span = v_max - v_min;
    if u_span.abs() <= 0.0001 || v_span.abs() <= 0.0001 {
        return None;
    }
    let mut uv = [[0.0_f32; 2]; 4];
    for index in 0..4 {
        uv[index] = [
            ((u_values[index] - u_min) / u_span).clamp(0.0, 1.0),
            ((v_values[index] - v_min) / v_span).clamp(0.0, 1.0),
        ];
    }
    Some(uv)
}

fn classify_quad_uv_transform(
    reference: &[[f32; 2]; 4],
    derived: &[[f32; 2]; 4],
) -> QuadUvTransform {
    for flip_u in [false, true] {
        for flip_v in [false, true] {
            for rotation in [0, 90, 180, 270] {
                let candidate = apply_quad_uv_transform(*reference, rotation, flip_u, flip_v);
                if quad_uvs_approx_eq(&candidate, derived) {
                    return QuadUvTransform {
                        rotation,
                        flip_u,
                        flip_v,
                        custom: false,
                    };
                }
            }
        }
    }
    QuadUvTransform {
        rotation: 0,
        flip_u: false,
        flip_v: false,
        custom: true,
    }
}

fn apply_quad_uv_transform(
    uv: [[f32; 2]; 4],
    rotation: i32,
    flip_u: bool,
    flip_v: bool,
) -> [[f32; 2]; 4] {
    uv.map(|coord| {
        let mut u = coord[0];
        let mut v = coord[1];
        if flip_u {
            u = 1.0 - u;
        }
        if flip_v {
            v = 1.0 - v;
        }
        match rotation.rem_euclid(360) {
            90 => [1.0 - v, u],
            180 => [1.0 - u, 1.0 - v],
            270 => [v, 1.0 - u],
            _ => [u, v],
        }
    })
}

fn quad_uvs_approx_eq(a: &[[f32; 2]; 4], b: &[[f32; 2]; 4]) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(lhs, rhs)| approx_eq(lhs[0], rhs[0]) && approx_eq(lhs[1], rhs[1]))
}

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.0001
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(v: [f32; 3], scale: f32) -> [f32; 3] {
    [v[0] * scale, v[1] * scale, v[2] * scale]
}

fn normalize3(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot3(v, v).sqrt();
    if length <= 0.0001 {
        None
    } else {
        Some(scale3(v, 1.0 / length))
    }
}

fn quad_normal(vertices: &[[f32; 3]; 4]) -> Option<[f32; 3]> {
    let ab = sub3(vertices[1], vertices[0]);
    let bc = sub3(vertices[2], vertices[1]);
    normalize3(cross3(ab, bc))
}

fn inset_debug_quad_vertices(vertices: &[[f32; 3]; 4]) -> Option<[[f32; 3]; 4]> {
    let center = scale3(
        vertices
            .iter()
            .copied()
            .reduce(add3)
            .unwrap_or([0.0, 0.0, 0.0]),
        0.25,
    );
    let u_dir = normalize3(sub3(vertices[1], vertices[0]))?;
    let v_dir = normalize3(sub3(vertices[3], vertices[0]))?;
    let normal = quad_normal(vertices)?;
    let width = ((sub3(vertices[1], vertices[0])
        .iter()
        .map(|v| v * v)
        .sum::<f32>())
    .sqrt()
        + (sub3(vertices[2], vertices[3])
            .iter()
            .map(|v| v * v)
            .sum::<f32>())
        .sqrt())
        * 0.5;
    let height = ((sub3(vertices[3], vertices[0])
        .iter()
        .map(|v| v * v)
        .sum::<f32>())
    .sqrt()
        + (sub3(vertices[2], vertices[1])
            .iter()
            .map(|v| v * v)
            .sum::<f32>())
        .sqrt())
        * 0.5;
    let half_w = (width * 0.72).clamp(0.16, 0.58) * 0.5;
    let half_h = (height * 0.72).clamp(0.12, 0.28) * 0.5;
    let lift = scale3(normal, -0.3);
    Some([
        add3(
            add3(center, scale3(u_dir, -half_w)),
            add3(scale3(v_dir, half_h), lift),
        ),
        add3(
            add3(center, scale3(u_dir, half_w)),
            add3(scale3(v_dir, half_h), lift),
        ),
        add3(
            add3(center, scale3(u_dir, half_w)),
            add3(scale3(v_dir, -half_h), lift),
        ),
        add3(
            add3(center, scale3(u_dir, -half_w)),
            add3(scale3(v_dir, -half_h), lift),
        ),
    ])
}

fn debug_label_quad_uv() -> [[f32; 2]; 4] {
    [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]
}

fn format_vec3(v: [f32; 3]) -> String {
    format!("[{:.3},{:.3},{:.3}]", v[0], v[1], v[2])
}

fn format_uv2(uv: &[[f32; 2]; 4]) -> String {
    format!(
        "[[{:.3},{:.3}],[{:.3},{:.3}],[{:.3},{:.3}],[{:.3},{:.3}]]",
        uv[0][0], uv[0][1], uv[1][0], uv[1][1], uv[2][0], uv[2][1], uv[3][0], uv[3][1]
    )
}

fn bake_face_image(image: RgbaImage, uv: Option<[f32; 4]>, rotation: i32) -> RgbaImage {
    let mut dyn_image = DynamicImage::ImageRgba8(image);
    if let Some([left, top, right, bottom]) = uv {
        let width = dyn_image.width() as f32;
        let height = dyn_image.height() as f32;
        let x0 = ((left.min(right) / 16.0) * width).round().clamp(0.0, width) as u32;
        let y0 = ((top.min(bottom) / 16.0) * height)
            .round()
            .clamp(0.0, height) as u32;
        let x1 = ((left.max(right) / 16.0) * width).round().clamp(0.0, width) as u32;
        let y1 = ((top.max(bottom) / 16.0) * height)
            .round()
            .clamp(0.0, height) as u32;
        if x1 > x0 && y1 > y0 {
            dyn_image = dyn_image.crop_imm(x0, y0, x1 - x0, y1 - y0);
        }
    }
    let turns = rotation.rem_euclid(360) / 90;
    match turns {
        1 => dyn_image.rotate270().to_rgba8(),
        2 => dyn_image.rotate180().to_rgba8(),
        3 => dyn_image.rotate90().to_rgba8(),
        _ => dyn_image.to_rgba8(),
    }
}

fn crop_texture_pixel_region(
    image: RgbaImage,
    uv: [f32; 4],
    reference_size: [f32; 2],
) -> RgbaImage {
    let [left, top, right, bottom] = uv;
    let width = image.width() as f32;
    let height = image.height() as f32;
    let scale_x = width / reference_size[0];
    let scale_y = height / reference_size[1];
    let x0 = (left.min(right) * scale_x).round().clamp(0.0, width) as u32;
    let y0 = (top.min(bottom) * scale_y).round().clamp(0.0, height) as u32;
    let x1 = (left.max(right) * scale_x).round().clamp(0.0, width) as u32;
    let y1 = (top.max(bottom) * scale_y).round().clamp(0.0, height) as u32;
    if x1 > x0 && y1 > y0 {
        DynamicImage::ImageRgba8(image)
            .crop_imm(x0, y0, x1 - x0, y1 - y0)
            .to_rgba8()
    } else {
        transparent_missing_texture()
    }
}

fn image_fingerprint(image: &RgbaImage) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    image.width().hash(&mut hasher);
    image.height().hash(&mut hasher);
    image.as_raw().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn pack_materials(materials: &[MaterialImage]) -> (RgbaImage, Vec<FullModeMaterialSlot>) {
    let padding = 1;
    let tile = materials
        .iter()
        .map(|m| m.image.width().max(m.image.height()) + padding * 2)
        .max()
        .unwrap_or(16)
        .max(16);
    let columns = ((materials.len().max(1) as f32).sqrt().ceil() as u32).max(1);
    let rows = (materials.len() as u32).div_ceil(columns).max(1);
    let mut atlas = RgbaImage::from_pixel(columns * tile, rows * tile, Rgba([0, 0, 0, 0]));
    let mut slots = Vec::with_capacity(materials.len());
    for (index, material) in materials.iter().enumerate() {
        let col = index as u32 % columns;
        let row = index as u32 / columns;
        let x = col * tile + padding;
        let y = row * tile + padding;
        blit_with_edge_padding(&mut atlas, &material.image, x, y, padding);
        if std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG").is_some()
            && observer_texture_family(&material.key).is_some()
            && !material.key.contains("observer_debug_label")
        {
            let family = observer_texture_family(&material.key).unwrap_or("?");
            println!(
                "[LBA_OBSERVER_ATLAS_20260423D] slot={} family={} key={} uv_rect=[{:.6},{:.6},{:.6},{:.6}] image={}x{}",
                index,
                family,
                material.key,
                x as f32 / atlas.width() as f32,
                y as f32 / atlas.height() as f32,
                (x + material.image.width()) as f32 / atlas.width() as f32,
                (y + material.image.height()) as f32 / atlas.height() as f32,
                material.image.width(),
                material.image.height(),
            );
        }
        slots.push(FullModeMaterialSlot {
            key: material.key.clone(),
            uv_rect: [
                x as f32 / atlas.width() as f32,
                y as f32 / atlas.height() as f32,
                (x + material.image.width()) as f32 / atlas.width() as f32,
                (y + material.image.height()) as f32 / atlas.height() as f32,
            ],
            alpha_mode: material.alpha_mode,
        });
    }
    (atlas, slots)
}

fn blit_with_edge_padding(atlas: &mut RgbaImage, image: &RgbaImage, x: u32, y: u32, padding: u32) {
    let width = image.width();
    let height = image.height();
    let padded_width = width + padding * 2;
    let padded_height = height + padding * 2;
    let origin_x = x.saturating_sub(padding);
    let origin_y = y.saturating_sub(padding);
    for py in 0..padded_height {
        for px in 0..padded_width {
            let sx = (px as i32 - padding as i32).clamp(0, width as i32 - 1) as u32;
            let sy = (py as i32 - padding as i32).clamp(0, height as i32 - 1) as u32;
            let dst_x = origin_x + px;
            let dst_y = origin_y + py;
            atlas.put_pixel(dst_x, dst_y, *image.get_pixel(sx, sy));
        }
    }
}

fn detect_alpha_mode(image: &RgbaImage) -> FullModeAlphaMode {
    let mut any_transparent = false;
    let mut any_partial = false;
    for pixel in image.pixels() {
        let alpha = pixel.0[3];
        if alpha < 255 {
            any_transparent = true;
        }
        if alpha != 0 && alpha != 255 {
            any_partial = true;
        }
    }
    if !any_transparent {
        FullModeAlphaMode::Opaque
    } else if any_partial {
        FullModeAlphaMode::Translucent
    } else {
        FullModeAlphaMode::Cutout
    }
}

fn redstone_wire_tinted_image(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    texture_id: &str,
    image: &RgbaImage,
) -> Option<RgbaImage> {
    if local_id(block_id) != "redstone_wire" {
        return None;
    }
    let texture_name = texture_id.rsplit('/').next().unwrap_or(texture_id);
    if !matches!(
        texture_name,
        "redstone_dust_dot" | "redstone_dust_line0" | "redstone_dust_line1"
    ) {
        return None;
    }
    let [r, g, b] = redstone_power_color(redstone_power(properties));
    let mut tinted = image.clone();
    for pixel in tinted.pixels_mut() {
        if pixel.0[3] == 0 {
            continue;
        }
        let mask = pixel.0[0] as u16;
        pixel.0[0] = ((u16::from(r) * mask) / 255) as u8;
        pixel.0[1] = ((u16::from(g) * mask) / 255) as u8;
        pixel.0[2] = ((u16::from(b) * mask) / 255) as u8;
    }
    Some(tinted)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BiomeTintClass {
    None,
    Grass,
    Foliage,
}

impl BiomeTintClass {
    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Grass => "grass",
            Self::Foliage => "foliage",
        }
    }

    fn fallback_rgb(self) -> Option<[u8; 3]> {
        match self {
            Self::None => None,
            Self::Grass => Some(DEFAULT_GRASS_TINT),
            Self::Foliage => Some(DEFAULT_FOLIAGE_TINT),
        }
    }
}

fn classify_biome_tint(
    block_id: &str,
    texture_id: &str,
    tint_index: Option<i32>,
) -> BiomeTintClass {
    if tint_index != Some(0) {
        return BiomeTintClass::None;
    }
    let local = local_id(block_id);
    if is_no_biome_tint_local(local) {
        return BiomeTintClass::None;
    }
    if is_foliage_tint_local(local) {
        return BiomeTintClass::Foliage;
    }
    if is_grass_tint_local(local) {
        return BiomeTintClass::Grass;
    }
    let texture_name = texture_id.rsplit('/').next().unwrap_or(texture_id);
    if is_foliage_tint_texture(texture_name) {
        BiomeTintClass::Foliage
    } else if is_grass_tint_texture(texture_name) {
        BiomeTintClass::Grass
    } else {
        BiomeTintClass::None
    }
}

fn is_no_biome_tint_local(local: &str) -> bool {
    matches!(local, "dirt" | "coarse_dirt" | "rooted_dirt")
}

fn is_grass_tint_local(local: &str) -> bool {
    matches!(
        local,
        "grass_block"
            | "grass"
            | "short_grass"
            | "tall_grass"
            | "fern"
            | "large_fern"
            | "sugar_cane"
    )
}

fn is_foliage_tint_local(local: &str) -> bool {
    local.ends_with("_leaves") || matches!(local, "vine")
}

fn is_grass_tint_texture(texture_name: &str) -> bool {
    matches!(
        texture_name,
        "grass_block_top"
            | "grass_block_side_overlay"
            | "grass"
            | "short_grass"
            | "tall_grass_bottom"
            | "tall_grass_top"
            | "fern"
            | "large_fern_bottom"
            | "large_fern_top"
            | "sugar_cane"
    )
}

fn is_foliage_tint_texture(texture_name: &str) -> bool {
    texture_name.ends_with("_leaves") || matches!(texture_name, "vine")
}

fn terrain_quad_uses_actual_world_face(
    block_id: &str,
    texture_id: &str,
    _tint_index: Option<i32>,
) -> bool {
    let local = local_id(block_id);
    let texture_name = texture_id.rsplit('/').next().unwrap_or(texture_id);
    match local {
        "grass_block" => matches!(
            texture_name,
            "dirt" | "grass_block_top" | "grass_block_side" | "grass_block_side_overlay"
        ),
        "dirt" | "coarse_dirt" | "rooted_dirt" | "podzol" | "mycelium" => true,
        _ => false,
    }
}

fn grass_trace_enabled() -> bool {
    env_flag_enabled("LBA_TINT_TRACE") || env_flag_enabled("LBA_GRASS_TRACE")
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false)
}

fn trace_grass_quad(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    local_face: &str,
    world_face: &str,
    texture_id: &str,
    tint_index: Option<i32>,
    material: &MaterialImage,
    vertices: &[[f32; 3]; 4],
    effective_cullface: Option<&str>,
) {
    if !grass_trace_enabled() {
        return;
    }
    let local = local_id(block_id);
    if !tint_trace_target_local(local) {
        return;
    }
    let texture_name = texture_id.rsplit('/').next().unwrap_or(texture_id);
    let is_top = local == "grass_block" && texture_name == "grass_block_top";
    let is_side = local == "grass_block" && texture_name == "grass_block_side";
    let is_side_overlay = local == "grass_block" && texture_name == "grass_block_side_overlay";
    let tint_class = classify_biome_tint(block_id, texture_id, tint_index);
    let tint_applied = material.key.contains("@biome_tint=");
    println!(
        "[LBA_TINT_TRACE_20260424A] state={} local_face={} world_face={} cullface={} texture_id={} is_grass_block_top={} is_grass_block_side={} is_grass_block_side_overlay={} tintindex={} tint_class={} fallback_tint_rgb={} final_tint_applied={} material_key={} vertices={:?} overlay_on_outer_face={}",
        state_key(block_id, properties),
        local_face,
        world_face,
        effective_cullface.unwrap_or("<none>"),
        texture_id,
        is_top,
        is_side,
        is_side_overlay,
        tint_index
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        tint_class.label(),
        tint_class
            .fallback_rgb()
            .map(format_rgb)
            .unwrap_or_else(|| "<none>".to_string()),
        tint_applied,
        material.key,
        vertices,
        if is_side_overlay {
            grass_overlay_vertices_on_outer_face(world_face, vertices).to_string()
        } else {
            "n/a".to_string()
        },
    );
}

fn terrain_stack_trace_enabled() -> bool {
    env_flag_enabled("LBA_TERRAIN_STACK_TRACE")
}

fn terrain_stack_trace_target_local(local: &str) -> bool {
    matches!(
        local,
        "dirt" | "coarse_dirt" | "rooted_dirt" | "grass_block" | "podzol" | "mycelium"
    )
}

fn trace_terrain_stack_quad(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    local_face: &str,
    rotated_world_face: &str,
    actual_world_face: &str,
    effective_cullface: Option<&str>,
    texture_id: &str,
    vertices: &[[f32; 3]; 4],
    material: &MaterialImage,
) {
    if !terrain_stack_trace_enabled() || !terrain_stack_trace_target_local(local_id(block_id)) {
        return;
    }
    println!(
        "[LBA_TERRAIN_STACK_TRACE_20260424A] stage=template_quad state={} local_face={} rotated_world_face={} actual_world_face={} cullface={} texture_id={} material_key={} generated=true local_vertices={:?}",
        state_key(block_id, properties),
        local_face,
        rotated_world_face,
        actual_world_face,
        effective_cullface.unwrap_or("<none>"),
        texture_id,
        material.key,
        vertices,
    );
}

fn trace_pane_quad(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    local_face: &str,
    rotated_world_face: &str,
    actual_world_face: &str,
    effective_cullface: Option<&str>,
    texture_id: &str,
    vertices: &[[f32; 3]; 4],
    material: &MaterialImage,
) {
    if !env_flag_enabled("LBA_PANE_TRACE") || !local_id(block_id).ends_with("_pane") {
        return;
    }
    println!(
        "[LBA_PANE_TRACE_20260424A] stage=template_quad state={} connections=north:{} south:{} west:{} east:{} local_face={} rotated_world_face={} actual_world_face={} cullface={} texture_id={} material_key={} double_sided=false backface_strategy=single_sided template_generated=true entered_mesh=pending final_culled=<mesh_trace> local_vertices={:?}",
        state_key(block_id, properties),
        properties
            .get("north")
            .map(String::as_str)
            .unwrap_or("<missing>"),
        properties
            .get("south")
            .map(String::as_str)
            .unwrap_or("<missing>"),
        properties
            .get("west")
            .map(String::as_str)
            .unwrap_or("<missing>"),
        properties
            .get("east")
            .map(String::as_str)
            .unwrap_or("<missing>"),
        local_face,
        rotated_world_face,
        actual_world_face,
        effective_cullface.unwrap_or("<none>"),
        texture_id,
        material.key,
        vertices,
    );
}

fn trace_pane_vanilla_template(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    quads: &[TemplateQuad],
) {
    if !env_flag_enabled("LBA_PANE_TRACE") {
        return;
    }
    for (index, quad) in quads.iter().enumerate() {
        println!(
            "[LBA_PANE_TRACE_20260424B] stage=glass_pane_vanilla_template state={} connections=north:{} south:{} west:{} east:{} quad={} texture_key={} uv={} double_sided={} backface_strategy=single_sided cullface={} vertices={:?} template_generated=true entered_mesh=pending",
            state_key(block_id, properties),
            properties
                .get("north")
                .map(String::as_str)
                .unwrap_or("<missing>"),
            properties
                .get("south")
                .map(String::as_str)
                .unwrap_or("<missing>"),
            properties
                .get("west")
                .map(String::as_str)
                .unwrap_or("<missing>"),
            properties
                .get("east")
                .map(String::as_str)
                .unwrap_or("<missing>"),
            index,
            quad.material.key,
            quad.uv
                .as_ref()
                .map(format_uv2)
                .unwrap_or_else(|| "None".to_string()),
            quad.double_sided,
            quad.cullface.as_deref().unwrap_or("<none>"),
            quad.vertices,
        );
    }
}

fn trace_tbl_typed_transform(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    model_ref: &OwnedModelRef,
    element_index: usize,
    face_name: &str,
    world_face: &str,
    cullface: Option<&str>,
    texture_id: &str,
    material: &MaterialImage,
    double_sided: bool,
    vertices: &[[f32; 3]; 4],
) {
    if std::env::var_os("LBA_FULL_MODE_V2_TBL_DEBUG").is_none() {
        return;
    }
    let local = local_id(block_id);
    if tbl_debug_family_prefix(local).is_none() {
        return;
    }
    println!(
        "[LBA_FULL_MODE_V2_TBL_TRANSFORM] state={} family={} model={} model_x={} model_y={} y_angle={} element={} local_face={} world_face={} cullface={} texture_id={} material_key={} double_sided={} backface_strategy={} vertices={:?}",
        state_key(block_id, properties),
        tbl_debug_family_name(local),
        model_ref.model,
        model_ref.x,
        model_ref.y,
        model_y_rotation_angle_for(local, model_ref.y),
        element_index,
        face_name,
        world_face,
        cullface.unwrap_or("<none>"),
        texture_id,
        material.key,
        double_sided,
        if double_sided {
            "double_sided"
        } else {
            "single_sided"
        },
        vertices,
    );
}

fn trace_tbl_model_transform(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    model_ref: &OwnedModelRef,
    face_name: &str,
    world_face: &str,
    cullface: Option<&str>,
    texture_id: &str,
    material: &MaterialImage,
    double_sided: bool,
    vertices: &[[f32; 3]; 4],
) {
    if std::env::var_os("LBA_FULL_MODE_V2_TBL_DEBUG").is_none() {
        return;
    }
    let local = local_id(block_id);
    if tbl_debug_family_prefix(local).is_none() {
        return;
    }
    println!(
        "[LBA_FULL_MODE_V2_TBL_TRANSFORM] state={} family={} model={} model_x={} model_y={} y_angle={} local_face={} world_face={} cullface={} texture_id={} material_key={} double_sided={} backface_strategy={} vertices={:?}",
        state_key(block_id, properties),
        tbl_debug_family_name(local),
        model_ref.model,
        model_ref.x,
        model_ref.y,
        model_y_rotation_angle_for(local, model_ref.y),
        face_name,
        world_face,
        cullface.unwrap_or("<none>"),
        texture_id,
        material.key,
        double_sided,
        if double_sided {
            "double_sided"
        } else {
            "single_sided"
        },
        vertices,
    );
}

fn trace_wall_attached_sign_transform(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    final_yaw: f32,
    attachment_offset: &str,
    board_min: [f32; 3],
    board_max: [f32; 3],
    quads: &[TemplateQuad],
) {
    if std::env::var_os("LBA_WALL_ATTACHED_TRACE").is_none() {
        return;
    }
    let local = local_id(block_id);
    if !local.ends_with("_wall_sign") {
        return;
    }
    let bounds = template_quads_bounds(quads)
        .map(|(min, max)| format_bounds3(min, max))
        .unwrap_or_else(|| "<empty>".to_string());
    println!(
        "[LBA_WALL_ATTACHED_TRACE_20260424A] family=wall_sign state={} facing={} transform_path=local_sign_template family_specific_yaw=true base_yaw=wall_sign_north final_yaw={} attachment_offset={} board_local_min={:?} board_local_max={:?} final_bounds={} quad_count={}",
        state_key(block_id, properties),
        properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north"),
        final_yaw,
        attachment_offset,
        board_min,
        board_max,
        bounds,
        quads.len(),
    );
}

fn trace_wall_attached_button_transform(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    model_ref: &OwnedModelRef,
    face_name: &str,
    world_face: &str,
    cullface: Option<&str>,
    vertices: &[[f32; 3]; 4],
) {
    if std::env::var_os("LBA_WALL_ATTACHED_TRACE").is_none() {
        return;
    }
    let local = local_id(block_id);
    if !local.ends_with("_button") || properties.get("face").map(String::as_str) != Some("wall") {
        return;
    }
    println!(
        "[LBA_WALL_ATTACHED_TRACE_20260424A] family=button state={} facing={} transform_path=vanilla_model_ref family_specific_yaw=true base_yaw=model_ref_y model_y={} final_yaw={} local_face={} world_face={} cullface={} vertices={:?}",
        state_key(block_id, properties),
        properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north"),
        model_ref.y,
        model_y_rotation_angle_for(local, model_ref.y),
        face_name,
        world_face,
        cullface.unwrap_or("<none>"),
        vertices,
    );
}

fn trace_stripped_log_template(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    quads: &[TemplateQuad],
) {
    if std::env::var_os("LBA_STRIPPED_LOG_TRACE").is_none() {
        return;
    }
    println!(
        "[LBA_STRIPPED_LOG_TRACE_20260424A] stage=stripped_log_template state={} axis={} path=vanilla_typed_model quad_count={} template_generated={} final_culled=false return_reason=template_generated",
        state_key(block_id, properties),
        properties.get("axis").map(String::as_str).unwrap_or("y"),
        quads.len(),
        !quads.is_empty(),
    );
    for (index, quad) in quads.iter().enumerate() {
        println!(
            "[LBA_STRIPPED_LOG_TRACE_20260424A] stage=stripped_log_quad state={} quad={} material_key={} cullface={} double_sided={} vertices={:?}",
            state_key(block_id, properties),
            index,
            quad.material.key,
            quad.cullface.as_deref().unwrap_or("<none>"),
            quad.double_sided,
            quad.vertices,
        );
    }
}

fn trace_transparent_glass_quad(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    local_face: &str,
    world_face: &str,
    cullface: Option<&str>,
    texture_id: &str,
    material: &MaterialImage,
    double_sided: bool,
    vertices: &[[f32; 3]; 4],
) {
    if !env_flag_enabled("LBA_GLASS_TRACE") {
        return;
    }
    let local = local_id(block_id);
    if !is_single_sided_transparent_local(local) {
        return;
    }
    println!(
        "[LBA_GLASS_TRACE_20260424A] stage=template_quad state={} local_face={} world_face={} cullface={} texture_id={} material_key={} alpha_mode={:?} double_sided={} backface_strategy={} final_culled=<mesh_trace> vertices={:?}",
        state_key(block_id, properties),
        local_face,
        world_face,
        cullface.unwrap_or("<none>"),
        texture_id,
        material.key,
        material.alpha_mode,
        double_sided,
        if double_sided {
            "double_sided"
        } else {
            "single_sided"
        },
        vertices,
    );
}

fn trace_basic_template(
    stage: &str,
    block_id: &str,
    properties: &BTreeMap<String, String>,
    quad_count: usize,
    note: &str,
) {
    if !env_flag_enabled("LBA_TEMPLATE_TRACE") {
        return;
    }
    println!(
        "[LBA_TEMPLATE_TRACE_20260424A] stage={} state={} template_generated={} quad_count={} entered_mesh=pending note={}",
        stage,
        state_key(block_id, properties),
        quad_count > 0,
        quad_count,
        note,
    );
}

fn tint_trace_target_local(local: &str) -> bool {
    is_grass_tint_local(local)
        || is_foliage_tint_local(local)
        || is_no_biome_tint_local(local)
        || matches!(local, "podzol" | "mycelium")
}

fn grass_overlay_vertices_on_outer_face(world_face: &str, vertices: &[[f32; 3]; 4]) -> bool {
    let epsilon = 0.0001;
    match world_face {
        "north" => vertices.iter().all(|vertex| vertex[2].abs() <= epsilon),
        "south" => vertices
            .iter()
            .all(|vertex| (vertex[2] - 1.0).abs() <= epsilon),
        "west" => vertices.iter().all(|vertex| vertex[0].abs() <= epsilon),
        "east" => vertices
            .iter()
            .all(|vertex| (vertex[0] - 1.0).abs() <= epsilon),
        "up" => vertices
            .iter()
            .all(|vertex| (vertex[1] - 1.0).abs() <= epsilon),
        "down" => vertices.iter().all(|vertex| vertex[1].abs() <= epsilon),
        _ => false,
    }
}

const DEFAULT_GRASS_TINT: [u8; 3] = [0x79, 0xC0, 0x5A];
const DEFAULT_FOLIAGE_TINT: [u8; 3] = [0x59, 0x9B, 0x33];
const DEFAULT_WATER_TINT: [u8; 3] = [0x3F, 0x76, 0xE4];

fn format_rgb(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn multiply_tint(image: &RgbaImage, tint: [u8; 3]) -> RgbaImage {
    let mut tinted = image.clone();
    for pixel in tinted.pixels_mut() {
        if pixel.0[3] == 0 {
            continue;
        }
        pixel.0[0] = ((u16::from(pixel.0[0]) * u16::from(tint[0])) / 255) as u8;
        pixel.0[1] = ((u16::from(pixel.0[1]) * u16::from(tint[1])) / 255) as u8;
        pixel.0[2] = ((u16::from(pixel.0[2]) * u16::from(tint[2])) / 255) as u8;
    }
    tinted
}

fn redstone_power(properties: &BTreeMap<String, String>) -> i32 {
    properties
        .get("power")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(0)
        .clamp(0, 15)
}

fn redstone_power_color(power: i32) -> [u8; 3] {
    let strength = power as f32 / 15.0;
    let red = (strength * 0.6 + if power > 0 { 0.4 } else { 0.3 }) * 255.0;
    let green = (strength * strength * 0.7 - 0.5).max(0.0) * 255.0;
    let blue = (strength * strength * 0.6 - 0.7).max(0.0) * 255.0;
    [
        red.clamp(0.0, 255.0) as u8,
        green.clamp(0.0, 255.0) as u8,
        blue.clamp(0.0, 255.0) as u8,
    ]
}

fn crop_first_animation_frame(image: RgbaImage) -> RgbaImage {
    if image.height() > image.width() && image.height().is_multiple_of(image.width()) {
        let width = image.width();
        DynamicImage::ImageRgba8(image)
            .crop_imm(0, 0, width, width)
            .to_rgba8()
    } else {
        image
    }
}

fn xk_texture_id_for_state(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    texture_id: &str,
) -> Option<String> {
    let local = local_id(block_id);
    let texture_name = texture_id.rsplit('/').next().unwrap_or(texture_id);
    if local == "repeater" && texture_name.starts_with("repeater") {
        let delay = properties
            .get("delay")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(1)
            .clamp(1, 4);
        let prefix = if properties.get("powered").map(String::as_str) == Some("true") {
            "repeater_on"
        } else {
            "repeater"
        };
        return Some(format!("minecraft:block/{prefix}{delay}"));
    }
    if local == "comparator" && texture_name.starts_with("comparator") {
        let suffix = if properties.get("mode").map(String::as_str) == Some("subtract") {
            "2"
        } else {
            ""
        };
        let on = if properties.get("powered").map(String::as_str) == Some("true") {
            "_on"
        } else {
            ""
        };
        return Some(format!("minecraft:block/comparator{suffix}{on}"));
    }
    if matches!(
        local,
        "piston" | "sticky_piston" | "piston_head" | "moving_piston"
    ) && texture_name.starts_with("piston")
    {
        let mut name = texture_name.to_string();
        if (local == "sticky_piston"
            || (local == "moving_piston"
                && properties.get("type").map(String::as_str) == Some("sticky")))
            && matches!(
                name.as_str(),
                "piston_top" | "piston_side" | "piston_bottom"
            )
        {
            name.push_str("_sticky");
        }
        if properties.get("extended").map(String::as_str) == Some("true")
            && matches!(texture_name, "piston_top" | "piston_side" | "piston_bottom")
        {
            name.push_str("_on");
        }
        return Some(format!("minecraft:block/{name}"));
    }
    if local == "redstone_wire" {
        if matches!(
            texture_name,
            "redstone_dust_dot"
                | "redstone_dust_line0"
                | "redstone_dust_line1"
                | "redstone_dust_overlay"
        ) {
            return Some(format!("minecraft:block/{texture_name}"));
        }
        let power = properties
            .get("power")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0)
            .clamp(0, 15);
        return Some(format!("minecraft:block/redstone_dust_p{power:02}"));
    }
    if local.contains("redstone") || texture_id.contains("redstone") {
        return Some(format!("minecraft:block/{texture_name}"));
    }
    None
}

fn locate_workspace_root() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_exe) = env::current_exe() {
        candidates.push(current_exe);
    }
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir);
    }
    for candidate in candidates {
        for dir in candidate.ancestors() {
            if dir
                .join("tools")
                .join("viewer-core")
                .join("Cargo.toml")
                .is_file()
            {
                return Ok(dir.to_path_buf());
            }
        }
    }
    Err(anyhow::anyhow!(
        "failed to locate workspace root from current executable or current directory"
    ))
}

fn load_png_path(path: &Path) -> Result<Option<RgbaImage>> {
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(
        image::open(path)
            .with_context(|| format!("load png failed: {}", path.display()))?
            .to_rgba8(),
    ))
}

fn load_observer_debug_semantic_image(name: &str) -> Result<RgbaImage> {
    let path = locate_workspace_root()?
        .join("tools")
        .join("viewer-core")
        .join("assets")
        .join("observer-debug-labels")
        .join(format!("{name}.png"));
    load_png_path(&path)?
        .with_context(|| format!("missing observer debug semantic image: {}", path.display()))
}

fn blockstate_path(block_id: &str) -> String {
    let (namespace, local) = split_namespace(block_id);
    format!("assets/{namespace}/blockstates/{local}.json")
}

fn model_path(model_ref: &str) -> String {
    let (namespace, path) = namespace_path(model_ref, "block/");
    format!("assets/{namespace}/models/{path}.json")
}

fn texture_asset_path(texture_id: &str) -> String {
    let (namespace, path) = texture_namespace_path(texture_id);
    format!("assets/{namespace}/textures/{path}.png")
}

fn texture_alias(texture_id: &str) -> Option<String> {
    let local = texture_id.rsplit('/').next().unwrap_or(texture_id);
    if local == "bubble_column" {
        return Some("minecraft:block/water_still".to_string());
    }
    if local == "lava" {
        return Some("minecraft:block/lava_still".to_string());
    }
    if local == "chain" {
        return Some("minecraft:block/iron_chain".to_string());
    }
    if let Some(base) = local.strip_suffix("_pane_top") {
        return Some(format!("minecraft:block/{base}"));
    }
    if let Some(base) = local.strip_suffix("_pane") {
        return Some(format!("minecraft:block/{base}"));
    }
    None
}

fn transparent_missing_texture() -> RgbaImage {
    RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0]))
}

fn namespace_path(value: &str, default_prefix: &str) -> (String, String) {
    let (namespace, mut path) = if let Some((namespace, path)) = value.split_once(':') {
        (namespace.to_string(), path.to_string())
    } else {
        ("minecraft".to_string(), value.to_string())
    };
    if !path.starts_with(default_prefix) {
        path = format!("{default_prefix}{path}");
    }
    (namespace, path)
}

fn texture_namespace_path(value: &str) -> (String, String) {
    let (namespace, mut path) = if let Some((namespace, path)) = value.split_once(':') {
        (namespace.to_string(), path.to_string())
    } else {
        ("minecraft".to_string(), value.to_string())
    };
    if !path.contains('/') {
        path = format!("block/{path}");
    }
    (namespace, path)
}

fn split_namespace(block_id: &str) -> (&str, &str) {
    block_id.split_once(':').unwrap_or(("minecraft", block_id))
}

fn local_id(block_id: &str) -> &str {
    block_id
        .split_once(':')
        .map(|(_, local)| local)
        .unwrap_or(block_id)
}

fn state_key(block_id: &str, properties: &BTreeMap<String, String>) -> String {
    if properties.is_empty() {
        return block_id.to_string();
    }
    let pairs = properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{block_id}[{pairs}]")
}

fn full_mode_v2_critical_default_properties(
    block_id: &str,
    properties: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let local = local_id(block_id);
    let mut normalized = properties.clone();
    let mut add = |key: &str, value: &str| {
        normalized
            .entry(key.to_string())
            .or_insert_with(|| value.to_string());
    };

    if local == "piston" || local == "sticky_piston" {
        add("extended", "false");
        add("facing", "north");
    } else if local == "observer" {
        add("powered", "false");
        add("facing", "north");
    } else if local == "repeater" {
        add("delay", "1");
        add("facing", "north");
        add("locked", "false");
        add("powered", "false");
    } else if local == "comparator" {
        add("facing", "north");
        add("mode", "compare");
        add("powered", "false");
    } else if local == "dispenser" || local == "dropper" {
        add("facing", "north");
        add("triggered", "false");
    } else if local == "hopper" {
        add("enabled", "true");
        add("facing", "down");
    } else if local == "lever" {
        add("face", "wall");
        add("facing", "north");
        add("powered", "false");
    } else if local.ends_with("_button")
        || local == "stone_button"
        || local == "polished_blackstone_button"
    {
        add("face", "wall");
        add("facing", "north");
        add("powered", "false");
    } else if local.ends_with("_trapdoor") || local == "iron_trapdoor" {
        add("facing", "north");
        add("half", "bottom");
        add("open", "false");
        add("powered", "false");
        add("waterlogged", "false");
    } else if local.ends_with("_door") || local == "iron_door" {
        add("facing", "north");
        add("half", "lower");
        add("hinge", "left");
        add("open", "false");
        add("powered", "false");
    } else if local.ends_with("_slab") {
        add("type", "bottom");
        add("waterlogged", "false");
    } else if local.ends_with("_stairs") {
        add("facing", "north");
        add("half", "bottom");
        add("shape", "straight");
        add("waterlogged", "false");
    } else if local.ends_with("_wall") || local.ends_with("_pane") || local == "iron_bars" {
        add("waterlogged", "false");
    }

    normalized
}

fn json_f32x3(value: &Value) -> Option<[f32; 3]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()? as f32,
        values.get(1)?.as_f64()? as f32,
        values.get(2)?.as_f64()? as f32,
    ])
}

fn json_f32x4(value: &Value) -> Option<[f32; 4]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()? as f32,
        values.get(1)?.as_f64()? as f32,
        values.get(2)?.as_f64()? as f32,
        values.get(3)?.as_f64()? as f32,
    ])
}

fn rotate_direction(direction: &str, x: i32, y: i32) -> String {
    let mut direction = direction.to_string();
    for _ in 0..(x.div_euclid(90).rem_euclid(4)) {
        direction = match direction.as_str() {
            "down" => "south",
            "south" => "up",
            "up" => "north",
            "north" => "down",
            other => other,
        }
        .to_string();
    }
    for _ in 0..(y.div_euclid(90).rem_euclid(4)) {
        direction = match direction.as_str() {
            "north" => "east",
            "east" => "south",
            "south" => "west",
            "west" => "north",
            other => other,
        }
        .to_string();
    }
    direction
}

fn rail_shape_truth(properties: &BTreeMap<String, String>) -> String {
    let shape = properties
        .get("shape")
        .map(String::as_str)
        .unwrap_or("north_south");
    match shape {
        "north_south" => "connects=north+south,flat",
        "east_west" => "connects=east+west,flat",
        "north_east" => "connects=north+east,flat_corner",
        "north_west" => "connects=north+west,flat_corner",
        "south_west" => "connects=south+west,flat_corner",
        "south_east" => "connects=south+east,flat_corner",
        "ascending_north" => "connects=north_high+south_low,ascending",
        "ascending_south" => "connects=south_high+north_low,ascending",
        "ascending_west" => "connects=west_high+east_low,ascending",
        "ascending_east" => "connects=east_high+west_low,ascending",
        _ => "connects=unknown",
    }
    .to_string()
}

fn rail_mesh_debug(quads: &[TemplateQuad]) -> String {
    let Some(quad) = quads.first() else {
        return "mesh=none".to_string();
    };
    let u_axis = dominant_axis(delta3(quad.vertices[1], quad.vertices[0]));
    let v_axis = dominant_axis(delta3(quad.vertices[2], quad.vertices[1]));
    let y_min = quad
        .vertices
        .iter()
        .map(|v| v[1])
        .fold(f32::INFINITY, f32::min);
    let y_max = quad
        .vertices
        .iter()
        .map(|v| v[1])
        .fold(f32::NEG_INFINITY, f32::max);
    format!(
        "quad_count={},geom_edge01={},geom_edge12={},rail_uv=[00,01,11,10],y_span={:.3}-{:.3}",
        quads.len(),
        u_axis,
        v_axis,
        y_min,
        y_max
    )
}

fn typed_block_truth(block_id: &str, properties: &BTreeMap<String, String>) -> String {
    match local_id(block_id) {
        "repeater" => format!(
            "facing={},delay={},locked={},powered={}",
            properties
                .get("facing")
                .map(String::as_str)
                .unwrap_or("south"),
            properties.get("delay").map(String::as_str).unwrap_or("1"),
            properties
                .get("locked")
                .map(String::as_str)
                .unwrap_or("false"),
            properties
                .get("powered")
                .map(String::as_str)
                .unwrap_or("false")
        ),
        "barrel" => format!(
            "facing={},open={}",
            properties.get("facing").map(String::as_str).unwrap_or("up"),
            properties
                .get("open")
                .map(String::as_str)
                .unwrap_or("false"),
        ),
        "hopper" => format!(
            "facing={},enabled={}",
            properties
                .get("facing")
                .map(String::as_str)
                .unwrap_or("down"),
            properties
                .get("enabled")
                .map(String::as_str)
                .unwrap_or("true")
        ),
        _ => "n/a".to_string(),
    }
}

fn hopper_debug_case_selected(properties: &BTreeMap<String, String>) -> bool {
    matches!(
        properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("down"),
        "down" | "north" | "south" | "east" | "west"
    ) && matches!(
        properties
            .get("enabled")
            .map(String::as_str)
            .unwrap_or("true"),
        "true" | "false"
    )
}

fn redstone_wire_truth(properties: &BTreeMap<String, String>) -> String {
    format!(
        "north={},east={},south={},west={},power={}",
        properties
            .get("north")
            .map(String::as_str)
            .unwrap_or("none"),
        properties.get("east").map(String::as_str).unwrap_or("none"),
        properties
            .get("south")
            .map(String::as_str)
            .unwrap_or("none"),
        properties.get("west").map(String::as_str).unwrap_or("none"),
        properties.get("power").map(String::as_str).unwrap_or("0")
    )
}

fn typed_mesh_debug(quads: &[TemplateQuad]) -> String {
    let Some(quad) = quads.first() else {
        return "mesh=none".to_string();
    };
    let edge01 = dominant_axis(delta3(quad.vertices[1], quad.vertices[0]));
    let edge12 = dominant_axis(delta3(quad.vertices[2], quad.vertices[1]));
    let uv = quad.uv.unwrap_or_else(|| typed_face_quad_uv("up"));
    format!("edge01={edge01},edge12={edge12},uv={uv:?}")
}

fn delta3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dominant_axis(delta: [f32; 3]) -> &'static str {
    let abs = delta.map(f32::abs);
    if abs[0] >= abs[1] && abs[0] >= abs[2] {
        if delta[0] >= 0.0 {
            "+x/east"
        } else {
            "-x/west"
        }
    } else if abs[2] >= abs[1] {
        if delta[2] >= 0.0 {
            "+z/south"
        } else {
            "-z/north"
        }
    } else if delta[1] >= 0.0 {
        "+y/up"
    } else {
        "-y/down"
    }
}

fn dominant_face_from_vertices(vertices: &[[f32; 3]; 4]) -> &'static str {
    let a = vertices[0];
    let b = vertices[1];
    let c = vertices[2];
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let bc = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
    let normal = [
        ab[1] * bc[2] - ab[2] * bc[1],
        ab[2] * bc[0] - ab[0] * bc[2],
        ab[0] * bc[1] - ab[1] * bc[0],
    ];
    let abs = [normal[0].abs(), normal[1].abs(), normal[2].abs()];
    if abs[1] >= abs[0] && abs[1] >= abs[2] {
        if normal[1] >= 0.0 { "up" } else { "down" }
    } else if abs[0] >= abs[2] {
        if normal[0] >= 0.0 { "east" } else { "west" }
    } else if normal[2] >= 0.0 {
        "south"
    } else {
        "north"
    }
}

fn chest_debug_visible_pair_half(
    properties: &BTreeMap<String, String>,
    world_face: &str,
) -> Option<(&'static str, &'static str)> {
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("north");
    let chest_type = properties
        .get("type")
        .map(String::as_str)
        .unwrap_or("single");
    if !matches!(facing, "east" | "west") || !matches!(chest_type, "left" | "right") {
        return None;
    }
    match (chest_world_face_class(facing, world_face), chest_type) {
        (ChestWorldFaceClass::Front, "left") => Some(("F", "R")),
        (ChestWorldFaceClass::Front, "right") => Some(("F", "L")),
        (ChestWorldFaceClass::Back, "left") => Some(("B", "L")),
        (ChestWorldFaceClass::Back, "right") => Some(("B", "R")),
        _ => None,
    }
}

fn chest_debug_template_candidate(
    properties: &BTreeMap<String, String>,
    part: &str,
    local_face: &str,
) -> Option<(String, &'static str, &'static str, &'static str)> {
    if !matches!(part, "base" | "lid") {
        return None;
    }
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("north");
    if !matches!(facing, "east" | "west") {
        return None;
    }
    let world_face = rotate_direction(local_face, 0, chest_rotation_degrees(facing) as i32);
    let (face_role, pair_half) = chest_debug_visible_pair_half(properties, &world_face)?;
    let candidate = match (part, face_role) {
        ("base", "F") => "base_front_candidate_A",
        ("base", "B") => "base_back_candidate_A",
        ("lid", "F") => "lid_front_candidate_B",
        ("lid", "B") => "lid_back_candidate_B",
        _ => return None,
    };
    Some((world_face, face_role, pair_half, candidate))
}

fn chest_debug_material_identity_label(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    world_face: &str,
) -> &'static str {
    let chest_type = properties
        .get("type")
        .map(String::as_str)
        .unwrap_or("single");
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("north");
    match chest_texture_identity_for_world_face(chest_type, facing, world_face) {
        ChestTemplateIdentity::Left => "L",
        ChestTemplateIdentity::Right => "R",
        ChestTemplateIdentity::Single => {
            let _ = block_id;
            "S"
        }
    }
}

fn flavor_label(flavor: ResourceFlavor) -> &'static str {
    match flavor {
        ResourceFlavor::Vanilla => "vanilla",
        ResourceFlavor::Faithful => "faithful",
        ResourceFlavor::Fallback => "fallback",
        ResourceFlavor::Xk => "xk",
    }
}

fn scene_bounds(root: &crate::nbt::LitematicRoot) -> Option<RegionBounds> {
    root.regions
        .values()
        .map(region_bounds)
        .reduce(|acc, item| RegionBounds {
            min_x: acc.min_x.min(item.min_x),
            max_x: acc.max_x.max(item.max_x),
            min_y: acc.min_y.min(item.min_y),
            max_y: acc.max_y.max(item.max_y),
            min_z: acc.min_z.min(item.min_z),
            max_z: acc.max_z.max(item.max_z),
        })
}

fn fill_region_palette_lookup(
    region: &crate::nbt::RegionNbt,
    bounds: RegionBounds,
    out: &mut HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> Result<()> {
    let volume = region_volume(&region.size)?;
    let width = i64::from(region.size.x).unsigned_abs() as usize;
    let length = i64::from(region.size.z).unsigned_abs() as usize;
    let layer_stride = width * length;
    let nbits = bits_for_palette(region.block_state_palette.len());
    for y in 0..i64::from(region.size.y).unsigned_abs() as i32 {
        for z in 0..i64::from(region.size.z).unsigned_abs() as i32 {
            for x in 0..i64::from(region.size.x).unsigned_abs() as i32 {
                let store_y = region_coord_to_storage_coord(y, region.size.y).unwrap_or(y as usize);
                let store_z = region_coord_to_storage_coord(z, region.size.z).unwrap_or(z as usize);
                let store_x = region_coord_to_storage_coord(x, region.size.x).unwrap_or(x as usize);
                let index = store_y * layer_stride + store_z * width + store_x;
                let palette_index = palette_index_at(&region.block_states, volume, nbits, index)?;
                let Some(state) = region.block_state_palette.get(palette_index) else {
                    continue;
                };
                let scene_pos = (
                    region.position.x + x - bounds.min_x,
                    region.position.y + y - bounds.min_y,
                    region.position.z + z - bounds.min_z,
                );
                out.insert(scene_pos, (state.name.clone(), state.properties.clone()));
            }
        }
    }
    Ok(())
}

fn tile_block_state(
    tile: &NbtValue,
    region: &crate::nbt::RegionNbt,
    bounds: RegionBounds,
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> Option<((i32, i32, i32), (String, BTreeMap<String, String>))> {
    for pos in tile_candidate_positions(tile, region, bounds) {
        if let Some(state) = palette_by_pos.get(&pos).cloned() {
            return Some((pos, state));
        }
    }
    None
}

fn tile_candidate_positions(
    tile: &NbtValue,
    region: &crate::nbt::RegionNbt,
    bounds: RegionBounds,
) -> Vec<(i32, i32, i32)> {
    let Some(compound) = nbt_compound(tile) else {
        return Vec::new();
    };
    let mut positions = Vec::new();
    if let (Some(x), Some(y), Some(z)) = (
        compound.get("x").and_then(nbt_i32),
        compound.get("y").and_then(nbt_i32),
        compound.get("z").and_then(nbt_i32),
    ) {
        positions.push((
            region.position.x + x - bounds.min_x,
            region.position.y + y - bounds.min_y,
            region.position.z + z - bounds.min_z,
        ));
        positions.push((x - bounds.min_x, y - bounds.min_y, z - bounds.min_z));
        let region_box = region_bounds(region);
        if point_in_region_bounds((x, y, z), region_box) {
            positions.push((x - bounds.min_x, y - bounds.min_y, z - bounds.min_z));
        }
    }
    if let Some(NbtValue::List(values)) = compound.get("Pos")
        && values.len() >= 3
    {
        positions.push((
            nbt_i32(&values[0]).unwrap_or_default() - bounds.min_x,
            nbt_i32(&values[1]).unwrap_or_default() - bounds.min_y,
            nbt_i32(&values[2]).unwrap_or_default() - bounds.min_z,
        ));
    }
    if let (Some(x), Some(y), Some(z)) = (
        compound.get("LocalX").and_then(nbt_i32),
        compound.get("LocalY").and_then(nbt_i32),
        compound.get("LocalZ").and_then(nbt_i32),
    ) {
        positions.push((
            region.position.x + x - bounds.min_x,
            region.position.y + y - bounds.min_y,
            region.position.z + z - bounds.min_z,
        ));
    }
    positions.sort_unstable();
    positions.dedup();
    positions
}

fn point_in_region_bounds(point: (i32, i32, i32), bounds: RegionBounds) -> bool {
    point.0 >= bounds.min_x
        && point.0 <= bounds.max_x
        && point.1 >= bounds.min_y
        && point.1 <= bounds.max_y
        && point.2 >= bounds.min_z
        && point.2 <= bounds.max_z
}

fn nbt_compound(value: &NbtValue) -> Option<&HashMap<String, NbtValue>> {
    if let NbtValue::Compound(map) = value {
        Some(map)
    } else {
        None
    }
}

fn nbt_i32(value: &NbtValue) -> Option<i32> {
    match value {
        NbtValue::Byte(v) => Some(*v as i32),
        NbtValue::Short(v) => Some(*v as i32),
        NbtValue::Int(v) => Some(*v),
        NbtValue::Long(v) => Some(*v as i32),
        _ => None,
    }
}

fn nbt_float_field(value: &NbtValue, keys: &[&str]) -> Option<f32> {
    let compound = nbt_compound(value)?;
    for key in keys {
        let Some(value) = compound.get(*key) else {
            continue;
        };
        match value {
            NbtValue::Float(v) => return Some(*v),
            NbtValue::Double(v) => return Some(*v as f32),
            NbtValue::Int(v) => return Some(*v as f32),
            _ => {}
        }
    }
    None
}

fn nbt_bool_field(value: &NbtValue, keys: &[&str]) -> Option<bool> {
    let compound = nbt_compound(value)?;
    for key in keys {
        let Some(value) = compound.get(*key) else {
            continue;
        };
        match value {
            NbtValue::Byte(v) => return Some(*v != 0),
            NbtValue::Int(v) => return Some(*v != 0),
            NbtValue::String(v) => return Some(matches!(v.as_str(), "true" | "1")),
            _ => {}
        }
    }
    None
}

fn nbt_string(value: &NbtValue) -> Option<String> {
    match value {
        NbtValue::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn nbt_block_state(value: &NbtValue, keys: &[&str]) -> Option<(String, BTreeMap<String, String>)> {
    let compound = nbt_compound(value)?;
    for key in keys {
        let Some(state) = compound.get(*key).and_then(nbt_compound) else {
            continue;
        };
        let name = state.get("Name").and_then(nbt_string)?;
        let mut properties = BTreeMap::new();
        if let Some(prop_map) = state.get("Properties").and_then(nbt_compound) {
            for (prop_key, prop_value) in prop_map {
                if let Some(prop_value) = nbt_string(prop_value) {
                    properties.insert(prop_key.clone(), prop_value);
                }
            }
        }
        return Some((name, properties));
    }
    None
}

#[derive(Default)]
struct SignTextFaces {
    front: [String; 4],
    back: [String; 4],
}

fn sign_text_faces(tile: &NbtValue) -> SignTextFaces {
    let Some(compound) = nbt_compound(tile) else {
        return SignTextFaces::default();
    };
    let mut faces = SignTextFaces::default();
    if let Some(front) = compound
        .get("front_text")
        .or_else(|| compound.get("FrontText"))
        .and_then(nbt_compound)
    {
        faces.front = sign_messages(front.get("messages"));
    }
    if let Some(back) = compound
        .get("back_text")
        .or_else(|| compound.get("BackText"))
        .and_then(nbt_compound)
    {
        faces.back = sign_messages(back.get("messages"));
    }
    if faces.front.iter().all(|line| line.is_empty()) {
        faces.front = [
            compound
                .get("Text1")
                .and_then(nbt_string)
                .map(|v| plain_text_from_component(&v))
                .unwrap_or_default(),
            compound
                .get("Text2")
                .and_then(nbt_string)
                .map(|v| plain_text_from_component(&v))
                .unwrap_or_default(),
            compound
                .get("Text3")
                .and_then(nbt_string)
                .map(|v| plain_text_from_component(&v))
                .unwrap_or_default(),
            compound
                .get("Text4")
                .and_then(nbt_string)
                .map(|v| plain_text_from_component(&v))
                .unwrap_or_default(),
        ];
    }
    faces
}

fn sign_messages(messages: Option<&NbtValue>) -> [String; 4] {
    let Some(NbtValue::List(messages)) = messages else {
        return Default::default();
    };
    let mut out: [String; 4] = Default::default();
    for (index, message) in messages.iter().take(4).enumerate() {
        out[index] = plain_text_from_component(&nbt_string(message).unwrap_or_default());
    }
    out
}

fn plain_text_from_component(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return text.trim_matches('"').to_string();
    };
    component_to_text(&value)
}

fn component_to_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(items) => items.iter().map(component_to_text).collect(),
        Value::Object(map) => {
            let mut out = map
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if let Some(extra) = map.get("extra").and_then(Value::as_array) {
                for item in extra {
                    out.push_str(&component_to_text(item));
                }
            }
            out
        }
        _ => String::new(),
    }
}

fn sign_text_material(
    builder: &mut FullModeV2Builder,
    block_id: &str,
    properties: &BTreeMap<String, String>,
    lines: &[String; 4],
    side: &str,
) -> Result<MaterialImage> {
    let mut image = RgbaImage::from_pixel(128, 64, Rgba([0, 0, 0, 0]));
    let font = builder.load_ascii_font()?;
    for (row, line) in lines.iter().enumerate() {
        draw_sign_text(&mut image, &font, line, 6 + row as u32 * 14);
    }
    Ok(MaterialImage {
        key: format!(
            "generated:sign_text:{}:{}:{}",
            state_key(block_id, properties),
            side,
            lines.join("|")
        ),
        image,
        alpha_mode: FullModeAlphaMode::Cutout,
    })
}

fn draw_sign_text(image: &mut RgbaImage, font: &RgbaImage, text: &str, y: u32) {
    let text = text.chars().take(15).collect::<String>();
    let width = text
        .chars()
        .map(|ch| sign_glyph_advance(font, ch))
        .sum::<u32>()
        .saturating_sub((!text.is_empty()) as u32);
    let mut x = image.width().saturating_sub(width) / 2;
    for ch in text.chars() {
        x += draw_sign_char(image, font, ch, x, y);
    }
}

fn draw_sign_char(image: &mut RgbaImage, font: &RgbaImage, ch: char, x: u32, y: u32) -> u32 {
    let glyph_index = (ch as u32).min(255);
    let cell_x = (glyph_index % 16) * 8;
    let cell_y = (glyph_index / 16) * 8;
    let width = sign_glyph_width(font, ch).max(1);
    for row in 0..8 {
        for col in 0..width {
            let src = font.get_pixel(cell_x + col, cell_y + row);
            if src.0[3] <= 8 {
                continue;
            }
            let px = x + col;
            let py = y + row;
            if px < image.width() && py < image.height() {
                image.put_pixel(px, py, Rgba([38, 24, 12, src.0[3].max(180)]));
            }
        }
    }
    width + 1
}

fn sign_glyph_width(font: &RgbaImage, ch: char) -> u32 {
    let glyph_index = (ch as u32).min(255);
    let cell_x = (glyph_index % 16) * 8;
    let cell_y = (glyph_index / 16) * 8;
    let mut width = 0_u32;
    for col in 0..8 {
        let non_empty = (0..8).any(|row| font.get_pixel(cell_x + col, cell_y + row).0[3] > 8);
        if non_empty {
            width = col + 1;
        }
    }
    if ch == ' ' { 4 } else { width.clamp(2, 8) }
}

fn sign_glyph_advance(font: &RgbaImage, ch: char) -> u32 {
    sign_glyph_width(font, ch) + 1
}

fn fill_debug_label_background(image: &mut RgbaImage) {
    let width = image.width();
    let height = image.height();
    for y in 3..height.saturating_sub(3) {
        for x in 3..width.saturating_sub(3) {
            let border =
                x < 6 || y < 6 || x >= width.saturating_sub(6) || y >= height.saturating_sub(6);
            image.put_pixel(
                x,
                y,
                if border {
                    Rgba([0, 0, 0, 205])
                } else {
                    Rgba([255, 236, 72, 188])
                },
            );
        }
    }
}

fn draw_debug_text(
    image: &mut RgbaImage,
    font: &RgbaImage,
    text: &str,
    y: u32,
    color: Rgba<u8>,
    scale: u32,
) {
    let text = text.chars().take(10).collect::<String>();
    let width = text
        .chars()
        .map(|ch| sign_glyph_advance(font, ch) * scale)
        .sum::<u32>()
        .saturating_sub((!text.is_empty()) as u32 * scale);
    let mut x = image.width().saturating_sub(width) / 2;
    for ch in text.chars() {
        x += draw_debug_char(image, font, ch, x, y, color, scale);
    }
}

fn draw_debug_text_at(
    image: &mut RgbaImage,
    font: &RgbaImage,
    text: &str,
    x: u32,
    y: u32,
    color: Rgba<u8>,
    scale: u32,
) {
    let text = text.chars().take(10).collect::<String>();
    let mut cursor_x = x;
    for ch in text.chars() {
        cursor_x += draw_debug_char(image, font, ch, cursor_x, y, color, scale);
    }
}

fn draw_debug_char(
    image: &mut RgbaImage,
    font: &RgbaImage,
    ch: char,
    x: u32,
    y: u32,
    color: Rgba<u8>,
    scale: u32,
) -> u32 {
    let glyph_index = (ch as u32).min(255);
    let cell_x = (glyph_index % 16) * 8;
    let cell_y = (glyph_index / 16) * 8;
    let width = sign_glyph_width(font, ch).max(1);
    for row in 0..8 {
        for col in 0..width {
            let src = font.get_pixel(cell_x + col, cell_y + row);
            if src.0[3] <= 8 {
                continue;
            }
            for sy in 0..scale {
                for sx in 0..scale {
                    let px = x + col * scale + sx;
                    let py = y + row * scale + sy;
                    if px < image.width() && py < image.height() {
                        image.put_pixel(px, py, color);
                    }
                }
            }
        }
    }
    (width + 1) * scale
}

fn sign_text_quads(
    builder: &mut FullModeV2Builder,
    block_id: &str,
    properties: &BTreeMap<String, String>,
    faces: &SignTextFaces,
) -> Result<Vec<TemplateQuad>> {
    let mut quads = Vec::new();
    if faces.front.iter().any(|line| !line.is_empty()) {
        let material = sign_text_material(builder, block_id, properties, &faces.front, "front")?;
        quads.push(sign_text_quad(properties, material, false));
    }
    if faces.back.iter().any(|line| !line.is_empty()) {
        let material = sign_text_material(builder, block_id, properties, &faces.back, "back")?;
        quads.push(sign_text_quad(properties, material, true));
    }
    Ok(quads)
}

fn sign_text_quad(
    properties: &BTreeMap<String, String>,
    material: MaterialImage,
    back_face: bool,
) -> TemplateQuad {
    let mut vertices = if back_face {
        [
            [0.84, 0.68, 0.469],
            [0.16, 0.68, 0.469],
            [0.16, 0.32, 0.469],
            [0.84, 0.32, 0.469],
        ]
    } else {
        [
            [0.16, 0.32, 0.531],
            [0.84, 0.32, 0.531],
            [0.84, 0.68, 0.531],
            [0.16, 0.68, 0.531],
        ]
    };
    let y_rotation = if let Some(facing) = properties.get("facing").map(String::as_str) {
        match facing {
            "south" => 180.0,
            "west" => 270.0,
            "east" => 90.0,
            _ => 0.0,
        }
    } else {
        properties
            .get("rotation")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0)
            * 22.5
    };
    if y_rotation != 0.0 {
        for vertex in &mut vertices {
            *vertex = rotate_point(*vertex, [0.5, 0.5, 0.5], "y", y_rotation, false);
        }
    }
    TemplateQuad {
        vertices,
        material,
        uv: None,
        double_sided: false,
        cullface: None,
    }
}

fn water_height(properties: &BTreeMap<String, String>) -> f32 {
    let level = properties
        .get("level")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0)
        .clamp(0, 15);
    if level >= 8 {
        1.0
    } else {
        (1.0 - (level as f32 + 1.0) / 9.0).clamp(0.125, 1.0)
    }
}

fn water_corner_heights(
    pos: (i32, i32, i32),
    properties: &BTreeMap<String, String>,
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> [f32; 4] {
    let current = water_height(properties);
    [
        water_corner_height(
            pos,
            current,
            palette_by_pos,
            &[(0, 0), (-1, 0), (0, -1), (-1, -1)],
        ),
        water_corner_height(
            pos,
            current,
            palette_by_pos,
            &[(0, 0), (-1, 0), (0, 1), (-1, 1)],
        ),
        water_corner_height(
            pos,
            current,
            palette_by_pos,
            &[(0, 0), (1, 0), (0, 1), (1, 1)],
        ),
        water_corner_height(
            pos,
            current,
            palette_by_pos,
            &[(0, 0), (1, 0), (0, -1), (1, -1)],
        ),
    ]
}

fn water_corner_height(
    pos: (i32, i32, i32),
    current_height: f32,
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
    offsets: &[(i32, i32)],
) -> f32 {
    let mut heights = Vec::new();
    for (dx, dz) in offsets {
        let sample_pos = (pos.0 + dx, pos.1, pos.2 + dz);
        if water_has_column_above(sample_pos, palette_by_pos) {
            return 1.0;
        }
        if let Some(height) = water_state_height_at(sample_pos, palette_by_pos) {
            heights.push(height);
        }
    }
    if heights.is_empty() {
        current_height
    } else {
        heights.iter().copied().sum::<f32>() / heights.len() as f32
    }
}

fn water_state_height_at(
    pos: (i32, i32, i32),
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> Option<f32> {
    let (block_id, properties) = palette_by_pos.get(&pos)?;
    if local_id(block_id) == "water" {
        Some(water_height(properties))
    } else if properties.get("waterlogged").map(String::as_str) == Some("true") {
        Some(1.0)
    } else {
        None
    }
}

fn water_has_column_above(
    pos: (i32, i32, i32),
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> bool {
    palette_by_pos
        .get(&(pos.0, pos.1 + 1, pos.2))
        .map(|(block_id, properties)| is_waterlike_state(block_id, properties))
        .unwrap_or(false)
}

fn lava_corner_heights(
    pos: (i32, i32, i32),
    properties: &BTreeMap<String, String>,
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> [f32; 4] {
    let current = water_height(properties);
    [
        lava_corner_height(
            pos,
            current,
            palette_by_pos,
            &[(0, 0), (-1, 0), (0, -1), (-1, -1)],
        ),
        lava_corner_height(
            pos,
            current,
            palette_by_pos,
            &[(0, 0), (-1, 0), (0, 1), (-1, 1)],
        ),
        lava_corner_height(
            pos,
            current,
            palette_by_pos,
            &[(0, 0), (1, 0), (0, 1), (1, 1)],
        ),
        lava_corner_height(
            pos,
            current,
            palette_by_pos,
            &[(0, 0), (1, 0), (0, -1), (1, -1)],
        ),
    ]
}

fn lava_corner_height(
    pos: (i32, i32, i32),
    current_height: f32,
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
    offsets: &[(i32, i32)],
) -> f32 {
    let mut heights = Vec::new();
    for (dx, dz) in offsets {
        let sample_pos = (pos.0 + dx, pos.1, pos.2 + dz);
        if lava_has_column_above(sample_pos, palette_by_pos) {
            return 1.0;
        }
        if let Some(height) = lava_state_height_at(sample_pos, palette_by_pos) {
            heights.push(height);
        }
    }
    if heights.is_empty() {
        current_height
    } else {
        heights.iter().copied().sum::<f32>() / heights.len() as f32
    }
}

fn lava_state_height_at(
    pos: (i32, i32, i32),
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> Option<f32> {
    let (block_id, properties) = palette_by_pos.get(&pos)?;
    if local_id(block_id) == "lava" {
        Some(water_height(properties))
    } else {
        None
    }
}

fn lava_has_column_above(
    pos: (i32, i32, i32),
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> bool {
    palette_by_pos
        .get(&(pos.0, pos.1 + 1, pos.2))
        .map(|(block_id, properties)| is_lavalike_state(block_id, properties))
        .unwrap_or(false)
}

fn water_face_should_render(neighbor: Option<&(String, BTreeMap<String, String>)>) -> bool {
    let Some((block_id, properties)) = neighbor else {
        return true;
    };
    let local = local_id(block_id);
    if is_waterlike_state(block_id, properties) {
        return false;
    }
    if is_full_glass_block(local) {
        return true;
    }
    if crate::mesh::full_mode_preserve_neighbor_faces_local(local) {
        return true;
    }
    !water_face_occluding_local(local)
}

fn water_face_skip_reason(neighbor: Option<&(String, BTreeMap<String, String>)>) -> &'static str {
    let Some((block_id, properties)) = neighbor else {
        return "visible_no_neighbor";
    };
    if is_waterlike_state(block_id, properties) {
        "same_fluid_neighbor"
    } else {
        "neighbor_occludes_water_face"
    }
}

fn is_waterlike_state(block_id: &str, properties: &BTreeMap<String, String>) -> bool {
    local_id(block_id) == "water"
        || properties.get("waterlogged").map(String::as_str) == Some("true")
}

fn lava_face_should_render(neighbor: Option<&(String, BTreeMap<String, String>)>) -> bool {
    let Some((block_id, _)) = neighbor else {
        return true;
    };
    let local = local_id(block_id);
    if local == "lava" {
        return false;
    }
    if is_full_glass_block(local) {
        return true;
    }
    if crate::mesh::full_mode_preserve_neighbor_faces_local(local) {
        return true;
    }
    !water_face_occluding_local(local)
}

fn is_lavalike_state(block_id: &str, _properties: &BTreeMap<String, String>) -> bool {
    local_id(block_id) == "lava"
}

fn trace_water_template_face(
    pos: (i32, i32, i32),
    block_id: &str,
    properties: &BTreeMap<String, String>,
    face: &str,
    neighbor_pos: (i32, i32, i32),
    neighbor: Option<&(String, BTreeMap<String, String>)>,
    generated: bool,
    return_reason: &str,
) {
    if !env_flag_enabled("LBA_WATER_TRACE") {
        return;
    }
    let neighbor_state = neighbor
        .map(|(id, props)| state_key(id, props))
        .unwrap_or_else(|| "<none>".to_string());
    println!(
        "[LBA_WATER_TRACE_20260424A] stage=water_template_at family=fluid block_pos=({}, {}, {}) state={} face={} local_face={} world_face={} cullface=<none> neighbor_pos=({}, {}, {}) neighbor_state={} same_fluid={} template_generated={} entered_mesh={} final_culled={} return_reason={} water_tint_rgb={} final_tint_applied=true visual_material=textured_tinted_single_sided backface_strategy=single_sided",
        pos.0,
        pos.1,
        pos.2,
        state_key(block_id, properties),
        face,
        face,
        face,
        neighbor_pos.0,
        neighbor_pos.1,
        neighbor_pos.2,
        neighbor_state,
        neighbor
            .map(|(id, props)| is_waterlike_state(id, props))
            .unwrap_or(false),
        generated,
        generated,
        !generated,
        return_reason,
        format_rgb(DEFAULT_WATER_TINT),
    );
}

fn trace_lava_template_face(
    pos: (i32, i32, i32),
    block_id: &str,
    properties: &BTreeMap<String, String>,
    face: &str,
    neighbor_pos: (i32, i32, i32),
    neighbor: Option<&(String, BTreeMap<String, String>)>,
    generated: bool,
) {
    if !env_flag_enabled("LBA_LAVA_TRACE") {
        return;
    }
    let neighbor_state = neighbor
        .map(|(id, props)| state_key(id, props))
        .unwrap_or_else(|| "<none>".to_string());
    println!(
        "[LBA_LAVA_TRACE] stage=lava_template_at family=fluid block_pos=({}, {}, {}) state={} face={} neighbor_pos=({}, {}, {}) neighbor_state={} same_fluid={} template_generated={} texture=minecraft:block/lava_still",
        pos.0,
        pos.1,
        pos.2,
        state_key(block_id, properties),
        face,
        neighbor_pos.0,
        neighbor_pos.1,
        neighbor_pos.2,
        neighbor_state,
        neighbor
            .map(|(id, props)| is_lavalike_state(id, props))
            .unwrap_or(false),
        generated,
    );
}

fn trace_waterlogged_overlay(
    pos: (i32, i32, i32),
    block_id: &str,
    properties: &BTreeMap<String, String>,
    quad_count: usize,
    generated: bool,
) {
    if !env_flag_enabled("LBA_WATER_TRACE") {
        return;
    }
    println!(
        "[LBA_WATER_TRACE_20260424A] stage=waterlogged_overlay family=waterlogged block_pos=({}, {}, {}) state={} waterlogged=true sub_geometry_generated={} quad_count={} entered_mesh={} final_culled=false return_reason=waterlogged_overlay",
        pos.0,
        pos.1,
        pos.2,
        state_key(block_id, properties),
        generated,
        quad_count,
        generated,
    );
}

fn water_face_occluding_local(local: &str) -> bool {
    !(water_non_occluding_local(local)
        || is_full_glass_block(local)
        || local.ends_with("_pane")
        || local.ends_with("_slab")
        || local.ends_with("_stairs")
        || local.ends_with("_wall")
        || local.ends_with("_fence")
        || local.ends_with("_gate")
        || local.ends_with("_door")
        || local.ends_with("_trapdoor")
        || local.ends_with("_button")
        || local.ends_with("_pressure_plate")
        || local.ends_with("_sign")
        || local.ends_with("_torch")
        || local.ends_with("_candle")
        || local.ends_with("_skull")
        || local == "ladder"
        || local == "vine"
        || local == "scaffolding")
}

fn water_non_occluding_local(local: &str) -> bool {
    local == "redstone_wire"
        || matches!(
            local,
            "redstone_torch"
                | "redstone_wall_torch"
                | "repeater"
                | "comparator"
                | "lever"
                | "piston"
                | "sticky_piston"
                | "piston_head"
                | "moving_piston"
                | "cauldron"
                | "water_cauldron"
                | "lava_cauldron"
                | "powder_snow_cauldron"
                | "composter"
                | "hopper"
                | "brewing_stand"
                | "chain"
                | "tripwire"
                | "air"
                | "cave_air"
                | "void_air"
        )
}

fn adjacent_pos(pos: (i32, i32, i32), face: &str) -> (i32, i32, i32) {
    match face {
        "north" => (pos.0, pos.1, pos.2 - 1),
        "south" => (pos.0, pos.1, pos.2 + 1),
        "west" => (pos.0 - 1, pos.1, pos.2),
        "east" => (pos.0 + 1, pos.1, pos.2),
        "up" => (pos.0, pos.1 + 1, pos.2),
        "down" => (pos.0, pos.1 - 1, pos.2),
        _ => pos,
    }
}

fn water_side_geometry(face: &str, heights: [f32; 4]) -> ([[f32; 3]; 4], [[f32; 2]; 4]) {
    match face {
        "north" => (
            [
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, heights[0], 0.0],
                [1.0, heights[3], 0.0],
            ],
            [
                [1.0, 1.0],
                [0.0, 1.0],
                [0.0, 1.0 - heights[0]],
                [1.0, 1.0 - heights[3]],
            ],
        ),
        "south" => (
            [
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, heights[2], 1.0],
                [0.0, heights[1], 1.0],
            ],
            [
                [0.0, 1.0],
                [1.0, 1.0],
                [1.0, 1.0 - heights[2]],
                [0.0, 1.0 - heights[1]],
            ],
        ),
        "west" => (
            [
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, heights[1], 1.0],
                [0.0, heights[0], 0.0],
            ],
            [
                [0.0, 1.0],
                [1.0, 1.0],
                [1.0, 1.0 - heights[1]],
                [0.0, 1.0 - heights[0]],
            ],
        ),
        "east" => (
            [
                [1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [1.0, heights[3], 0.0],
                [1.0, heights[2], 1.0],
            ],
            [
                [1.0, 1.0],
                [0.0, 1.0],
                [0.0, 1.0 - heights[3]],
                [1.0, 1.0 - heights[2]],
            ],
        ),
        _ => (
            face_vertices(face, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap(),
            typed_face_quad_uv(face),
        ),
    }
}

fn chest_open_semantic_face<'a>(part: &str, face: &'a str) -> &'a str {
    match (part, face) {
        ("lid", "down") => "up",
        ("base", "up") => "down",
        _ => face,
    }
}

fn chest_cuboid_face_uv(u: f32, v: f32, dx: f32, dy: f32, dz: f32, face: &str) -> [f32; 4] {
    match face {
        "west" => [u, v + dz, u + dz, v + dz + dy],
        "north" => [u + dz, v + dz, u + dz + dx, v + dz + dy],
        "east" => [u + dz + dx, v + dz, u + dz + dx + dz, v + dz + dy],
        "south" => [u + dz + dx + dz, v + dz, u + dz + dx + dz + dx, v + dz + dy],
        "up" => [u + dz, v, u + dz + dx, v + dz],
        "down" => [u + dz + dx, v, u + dz + dx + dx, v + dz],
        _ => [u + dz, v, u + dz + dx, v + dz],
    }
}

fn chest_texture_id_for_face(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    local_face: &str,
    semantic_face: &str,
) -> String {
    let chest_type = properties
        .get("type")
        .map(String::as_str)
        .unwrap_or("single");
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("north");
    let world_face = rotate_direction(local_face, 0, chest_rotation_degrees(facing) as i32);
    let face_class = chest_world_face_class(facing, &world_face);
    let branch_name = chest_texture_identity_branch_name(chest_type, facing, &world_face);
    let pre_identity = chest_canonical_identity(chest_type);
    let identity = chest_texture_identity_for_world_face(chest_type, facing, &world_face);
    let _ = semantic_face;
    let texture_id = chest_texture_id_for_template(block_id, identity);
    if std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_some()
        && local_id(block_id) == "chest"
        && matches!(facing, "east" | "west")
        && matches!(
            face_class,
            ChestWorldFaceClass::Front | ChestWorldFaceClass::Back
        )
        && matches!(local_face, "north" | "south")
    {
        println!(
            "[LBA_FULL_MODE_V2_CHEST_ID] state={} world_face={} local_face={} class={} branch_name={} pre_identity={} post_identity={} final_texture_id={}",
            state_key(block_id, properties),
            world_face,
            local_face,
            face_class.label(),
            branch_name,
            pre_identity.label(),
            identity.label(),
            texture_id,
        );
    }
    texture_id
}

fn chest_texture_id_for_owner_semantic_swap(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    part: &str,
    local_face: &str,
    semantic_face: &str,
) -> String {
    let base_texture_id =
        chest_texture_id_for_face(block_id, properties, local_face, semantic_face);
    if !chest_owner_semantic_swap_target(block_id, properties, part, local_face) {
        return base_texture_id;
    }
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("north");
    let world_face = rotate_direction(local_face, 0, chest_rotation_degrees(facing) as i32);
    let swapped_texture_id = swap_chest_texture_id_suffix(&base_texture_id);
    if std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_some() {
        let pair = chest_debug_visible_pair_half(properties, &world_face)
            .map(|(face_role, pair_half)| format!("{face_role}-{pair_half}"))
            .unwrap_or_else(|| "?:?".to_string());
        println!(
            "[LBA_FULL_MODE_V2_CHEST_OWNER_SWAP] state={} part={} local_face={} world_face={} target={} base_texture_id={} swapped_texture_id={}",
            state_key(block_id, properties),
            part,
            local_face,
            world_face,
            pair,
            base_texture_id,
            swapped_texture_id,
        );
    }
    swapped_texture_id
}

fn swap_chest_texture_id_suffix(texture_id: &str) -> String {
    if texture_id.ends_with("_left") {
        format!("{}{}", texture_id.trim_end_matches("_left"), "_right")
    } else if texture_id.ends_with("_right") {
        format!("{}{}", texture_id.trim_end_matches("_right"), "_left")
    } else {
        texture_id.to_string()
    }
}

fn chest_owner_semantic_swap_target(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    part: &str,
    local_face: &str,
) -> bool {
    local_id(block_id) == "chest"
        && matches!(part, "base" | "lid")
        && matches!(local_face, "north" | "south")
        && matches!(
            properties
                .get("facing")
                .map(String::as_str)
                .unwrap_or("north"),
            "east" | "west"
        )
        && matches!(
            properties
                .get("type")
                .map(String::as_str)
                .unwrap_or("single"),
            "left" | "right"
        )
}

fn chest_texture_identity_for_world_face(
    chest_type: &str,
    facing: &str,
    world_face: &str,
) -> ChestTemplateIdentity {
    let identity = chest_canonical_identity(chest_type);
    if chest_texture_identity_should_swap(chest_type, facing, world_face) {
        swap_chest_texture_identity(identity)
    } else {
        identity
    }
}

fn chest_texture_identity_should_swap(chest_type: &str, facing: &str, world_face: &str) -> bool {
    if chest_type == "single" {
        return false;
    }
    match (
        facing,
        chest_type,
        chest_world_face_class(facing, world_face),
    ) {
        ("north", "left" | "right", ChestWorldFaceClass::Back) => true,
        ("south", "left" | "right", ChestWorldFaceClass::Back) => true,
        ("east", "left", ChestWorldFaceClass::Front) => false,
        ("east", "right", ChestWorldFaceClass::Front) => false,
        ("west", "left", ChestWorldFaceClass::Front) => false,
        ("west", "right", ChestWorldFaceClass::Front) => false,
        ("east", "left", ChestWorldFaceClass::Back) => true,
        ("east", "right", ChestWorldFaceClass::Back) => true,
        ("west", "left", ChestWorldFaceClass::Back) => true,
        ("west", "right", ChestWorldFaceClass::Back) => true,
        _ => false,
    }
}

fn chest_texture_identity_branch_name(
    chest_type: &str,
    facing: &str,
    world_face: &str,
) -> &'static str {
    if chest_type == "single" {
        return "single_no_swap";
    }
    match (
        facing,
        chest_type,
        chest_world_face_class(facing, world_face),
    ) {
        ("north", "left" | "right", ChestWorldFaceClass::Back) => "north_back_swap",
        ("south", "left" | "right", ChestWorldFaceClass::Back) => "south_back_swap",
        ("east", "left", ChestWorldFaceClass::Front) => "east_left_front_no_swap",
        ("east", "right", ChestWorldFaceClass::Front) => "east_right_front_no_swap",
        ("west", "left", ChestWorldFaceClass::Front) => "west_left_front_no_swap",
        ("west", "right", ChestWorldFaceClass::Front) => "west_right_front_no_swap",
        ("east", "left", ChestWorldFaceClass::Back) => "east_left_back_swap",
        ("east", "right", ChestWorldFaceClass::Back) => "east_right_back_swap",
        ("west", "left", ChestWorldFaceClass::Back) => "west_left_back_swap",
        ("west", "right", ChestWorldFaceClass::Back) => "west_right_back_swap",
        _ => "no_swap",
    }
}

fn swap_chest_texture_identity(identity: ChestTemplateIdentity) -> ChestTemplateIdentity {
    match identity {
        ChestTemplateIdentity::Left => ChestTemplateIdentity::Right,
        ChestTemplateIdentity::Right => ChestTemplateIdentity::Left,
        ChestTemplateIdentity::Single => ChestTemplateIdentity::Single,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChestWorldFaceClass {
    Front,
    Back,
    Other,
}

impl ChestWorldFaceClass {
    fn label(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Back => "back",
            Self::Other => "other",
        }
    }
}

fn chest_world_face_class(facing: &str, world_face: &str) -> ChestWorldFaceClass {
    if world_face == facing {
        ChestWorldFaceClass::Front
    } else if world_face == opposite_direction(facing) {
        ChestWorldFaceClass::Back
    } else {
        ChestWorldFaceClass::Other
    }
}

fn opposite_direction(direction: &str) -> &str {
    match direction {
        "north" => "south",
        "south" => "north",
        "west" => "east",
        "east" => "west",
        "up" => "down",
        "down" => "up",
        _ => direction,
    }
}

fn chest_texture_id_for_template(block_id: &str, identity: ChestTemplateIdentity) -> String {
    let local = local_id(block_id);
    if local == "ender_chest" {
        return "minecraft:entity/chest/ender".to_string();
    }
    let base = if local == "trapped_chest" {
        "trapped"
    } else {
        "normal"
    };
    match identity {
        ChestTemplateIdentity::Left => format!("minecraft:entity/chest/{base}_left"),
        ChestTemplateIdentity::Right => format!("minecraft:entity/chest/{base}_right"),
        ChestTemplateIdentity::Single => format!("minecraft:entity/chest/{base}"),
    }
}

fn chest_debug_trace_target(
    block_id: &str,
    properties: &BTreeMap<String, String>,
    face_name: &str,
) -> bool {
    if std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_none() || local_id(block_id) != "chest" {
        return false;
    }
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("north");
    if !matches!(facing, "east" | "west") {
        return false;
    }
    matches!(chest_debug_local_face(face_name), "north" | "south")
}

fn chest_debug_local_face(face_name: &str) -> &str {
    face_name
        .split('_')
        .nth(1)
        .and_then(|segment| segment.split('@').next())
        .unwrap_or("?")
}

fn chest_debug_semantic_face(face_name: &str) -> &str {
    face_name.split("@semantic=").nth(1).unwrap_or("?")
}

fn chest_debug_world_face(properties: &BTreeMap<String, String>, face_name: &str) -> String {
    let facing = properties
        .get("facing")
        .map(String::as_str)
        .unwrap_or("north");
    rotate_direction(
        chest_debug_local_face(face_name),
        0,
        chest_rotation_degrees(facing) as i32,
    )
}

fn chest_debug_palette_target(block_id: &str, properties: &BTreeMap<String, String>) -> bool {
    if std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_none() || local_id(block_id) != "chest" {
        return false;
    }
    matches!(
        properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north"),
        "east" | "west"
    ) && matches!(
        properties
            .get("type")
            .map(String::as_str)
            .unwrap_or("single"),
        "left" | "right"
    )
}

fn chest_debug_sample_label(properties: &BTreeMap<String, String>) -> Option<&'static str> {
    match (
        properties
            .get("facing")
            .map(String::as_str)
            .unwrap_or("north"),
        properties
            .get("type")
            .map(String::as_str)
            .unwrap_or("single"),
    ) {
        ("east", "left") => Some("C49-6"),
        ("east", "right") => Some("C55-6"),
        ("west", "right") => Some("C54-5"),
        ("west", "left") => Some("C56-5"),
        _ => None,
    }
}

fn chest_debug_bc_lookup_key(state: &str, vertices: &[[f32; 3]; 4]) -> String {
    format!("{state}|{}", quantized_chest_debug_world_bounds(vertices))
}

fn quantized_chest_debug_world_bounds(vertices: &[[f32; 3]; 4]) -> String {
    let min_x = vertices
        .iter()
        .map(|vertex| vertex[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = vertices
        .iter()
        .map(|vertex| vertex[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = vertices
        .iter()
        .map(|vertex| vertex[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = vertices
        .iter()
        .map(|vertex| vertex[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_z = vertices
        .iter()
        .map(|vertex| vertex[2])
        .fold(f32::INFINITY, f32::min);
    let max_z = vertices
        .iter()
        .map(|vertex| vertex[2])
        .fold(f32::NEG_INFINITY, f32::max);
    format!("[{min_x:.4}..{max_x:.4},{min_y:.4}..{max_y:.4},{min_z:.4}..{max_z:.4}]")
}

fn format_bounds3(min: [f32; 3], max: [f32; 3]) -> String {
    format!(
        "[{:.4}..{:.4},{:.4}..{:.4},{:.4}..{:.4}]",
        min[0], max[0], min[1], max[1], min[2], max[2]
    )
}

fn facing_offset(facing: &str, distance: f32) -> [f32; 3] {
    match facing {
        "down" => [0.0, -distance, 0.0],
        "up" => [0.0, distance, 0.0],
        "south" => [0.0, 0.0, distance],
        "west" => [-distance, 0.0, 0.0],
        "east" => [distance, 0.0, 0.0],
        _ => [0.0, 0.0, -distance],
    }
}

fn offset_block_pos(pos: (i32, i32, i32), facing: &str, distance: i32) -> (i32, i32, i32) {
    match facing {
        "down" => (pos.0, pos.1 - distance, pos.2),
        "up" => (pos.0, pos.1 + distance, pos.2),
        "south" => (pos.0, pos.1, pos.2 + distance),
        "west" => (pos.0 - distance, pos.1, pos.2),
        "east" => (pos.0 + distance, pos.1, pos.2),
        _ => (pos.0, pos.1, pos.2 - distance),
    }
}

fn piston_debug_labels_by_pos(
    palette_by_pos: &HashMap<(i32, i32, i32), (String, BTreeMap<String, String>)>,
) -> HashMap<(i32, i32, i32), String> {
    let mut pistons = palette_by_pos
        .iter()
        .filter(|(_, (block_id, _))| is_static_piston_family(local_id(block_id)))
        .map(|(pos, _)| *pos)
        .collect::<Vec<_>>();
    pistons.sort_by_key(|(x, y, z)| (*z, *y, *x));
    pistons
        .into_iter()
        .enumerate()
        .map(|(index, pos)| (pos, format!("P{:02}", index + 1)))
        .collect()
}

fn translate_template_quads(quads: &mut [TemplateQuad], offset: [f32; 3]) {
    for quad in quads {
        for vertex in &mut quad.vertices {
            vertex[0] += offset[0];
            vertex[1] += offset[1];
            vertex[2] += offset[2];
        }
    }
}
