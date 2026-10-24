//! Session-only, renderer-independent 2D layer execution. Packed NBT remains in
//! Core; the Host carries bounded PNGs and single-cell hits, never block arrays.
use crate::{
    nbt::{self, LitematicRoot, PaletteLayout},
    visual::{self, VisualCatalog},
};
use anyhow::{bail, ensure, Context, Result};
use base64::Engine;
use image::{ImageEncoder, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::SystemTime,
};
#[path = "layer_hints.rs"]
mod hints;

const MAX_IMAGE_EDGE: u32 = 1536;
const SPRITE_CACHE_BYTES: usize = 8 * 1024 * 1024;
const SOURCE_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_REGION_EDGE: i32 = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewRect {
    pub x: i32,
    pub z: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    action: String,
    source_path: Option<String>,
    storage_dir: Option<PathBuf>,
    ui_locale: Option<String>,
    #[serde(default)]
    resource_packs: Vec<PathBuf>,
    scene_handle: Option<String>,
    generation: Option<u64>,
    request_id: Option<u64>,
    y: Option<i32>,
    x: Option<i32>,
    z: Option<i32>,
    view_rect: Option<ViewRect>,
    scale: Option<f64>,
    #[serde(default)]
    onion_depth: u8,
    #[serde(default)]
    state_hints: bool,
    #[serde(default)]
    overrides: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Clone, PartialEq, Eq)]
struct SourceStamp {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}
impl SourceStamp {
    fn read(path: &Path) -> Result<Self> {
        let path = fs::canonicalize(path)?;
        let meta = fs::metadata(&path)?;
        ensure!(meta.is_file(), "Layer source must be a file");
        Ok(Self {
            path,
            bytes: meta.len(),
            modified: meta.modified()?,
        })
    }
}
struct RegionIndex {
    name: String,
    layout: PaletteLayout,
    palette: Vec<Option<usize>>,
}
struct Scene {
    generation: u64,
    source: SourceStamp,
    content_id: String,
    resource_context: Arc<crate::layer_resources::LayerResourceContext>,
    resource_packs: Vec<PathBuf>,
    root: Arc<LitematicRoot>,
    catalog: Arc<VisualCatalog>,
    regions: Arc<Vec<RegionIndex>>,
    version: String,
    assets: hints::HintAssets,
    latest: AtomicU64,
    cancelled: AtomicU64,
    released: AtomicBool,
    visual_generation: AtomicU64,
    sprites: Mutex<VecDeque<(String, Arc<RgbaImage>)>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualIssue {
    state: String,
    kind: String,
    resource_key: Option<String>,
    message: String,
    retryable: bool,
    block_count: usize,
}
#[derive(Clone)]
struct SpriteVisual {
    image: Arc<RgbaImage>,
    state: String,
    transparent: bool,
    issue: Option<VisualIssue>,
}

/// A scene pin owns the immutable packed source. Only derived sprites are cached,
/// with a byte bound; release drops that scene without touching any 3D lifecycle.
#[derive(Default)]
pub struct LayerService {
    next: AtomicU64,
    scenes: Mutex<BTreeMap<String, Arc<Scene>>>,
    sources: Mutex<VecDeque<CachedSource>>,
}
#[derive(Clone)]
struct CachedSource {
    source: SourceStamp,
    content_id: String,
    root: Arc<LitematicRoot>,
    catalog: Arc<VisualCatalog>,
    regions: Arc<Vec<RegionIndex>>,
    version: String,
    weight: usize,
}

impl LayerService {
    /// Runs on the transport reader before lane admission. Supersession and
    /// cancellation must not wait behind the render they are stopping.
    pub fn prepare(&self, input: &Value) {
        let Some(handle) = input["sceneHandle"].as_str() else {
            return;
        };
        let Ok(scenes) = self.scenes.lock() else {
            return;
        };
        let Some(scene) = scenes.get(handle) else {
            return;
        };
        if input["generation"].as_u64() != Some(scene.generation) {
            return;
        }
        match input["action"].as_str() {
            Some("requestLayerView") => {
                if let Some(id) = input["requestId"].as_u64() {
                    scene.latest.fetch_max(id, Ordering::AcqRel);
                }
            }
            Some("cancel") => {
                if let Some(id) = input["requestId"].as_u64() {
                    scene.cancelled.fetch_max(id, Ordering::AcqRel);
                }
            }
            Some("release") => scene.released.store(true, Ordering::Release),
            Some("invalidateVisuals") => {
                scene.visual_generation.fetch_add(1, Ordering::AcqRel);
            }
            _ => {}
        }
    }
    pub fn execute(&self, input: &Value, cancel: &AtomicBool) -> Result<Value> {
        let input: Input = serde_json::from_value(input.clone())?;
        if input.action == "openScene" {
            return self.open(&input, cancel);
        }
        let handle = input
            .scene_handle
            .as_deref()
            .context("Layer scene handle is missing")?;
        let scene = self
            .scenes
            .lock()
            .map_err(|_| anyhow::anyhow!("Layer registry lock failed"))?
            .get(handle)
            .cloned()
            .context("LayerSceneExpired: Layer scene has been released; reopen the source")?;
        ensure!(
            input.generation == Some(scene.generation),
            "Stale layer scene generation"
        );
        if input.action == "release" {
            scene.released.store(true, Ordering::Release);
            self.scenes.lock().unwrap().remove(handle);
            return Ok(json!({"sceneHandle":handle,"generation":scene.generation,"released":true}));
        }
        if input.action == "cancel" {
            if let Some(id) = input.request_id {
                scene.cancelled.fetch_max(id, Ordering::AcqRel);
            }
            return Ok(
                json!({"sceneHandle":handle,"generation":scene.generation,"cancelled":true}),
            );
        }
        ensure!(
            SourceStamp::read(&scene.source.path)
                .context("LayerSceneExpired: Inspect layer source; reopen the source")?
                == scene.source,
            "LayerSceneExpired: Layer source changed; reopen the source"
        );
        scene.check_resources(&input.resource_packs)?;
        match input.action.as_str() {
            "getLayerMeta" => Ok(scene.meta(handle)),
            "invalidateVisuals" => {
                scene.visual_generation.fetch_add(1, Ordering::AcqRel);
                scene.sprites.lock().unwrap().clear();
                Ok(
                    json!({"sceneHandle":handle,"generation":scene.generation,"resourceIdentity":scene.resource_identity(),"visualGeneration":scene.visual_generation.load(Ordering::Acquire)}),
                )
            }
            "requestLayerView" => {
                let id = input
                    .request_id
                    .context("Layer request identity is missing")?;
                ensure!(id > 0, "Layer request identity must be positive");
                let previous = scene.latest.fetch_max(id, Ordering::AcqRel);
                ensure!(id >= previous, "Layer request superseded");
                scene.check(id, cancel)?;
                let result = scene.render(handle, &input, id, cancel)?;
                scene.check(id, cancel)?;
                scene.check_resources(&input.resource_packs)?;
                Ok(result)
            }
            "hitTest" => {
                let id = input
                    .request_id
                    .context("Layer request identity is missing")?;
                scene.check(id, cancel)?;
                let x = input.x.context("Layer hit X is missing")?;
                let z = input.z.context("Layer hit Z is missing")?;
                let y = input.y.context("Layer hit Y is missing")?;
                let mut hit = None;
                for depth in 0..=input.onion_depth.min(5) as i32 {
                    if let Some(block) = scene.hit(
                        x,
                        y - depth,
                        z,
                        &input.overrides,
                        input.ui_locale.as_deref().unwrap_or("zh-CN"),
                    )? {
                        hit = Some(block);
                        break;
                    }
                }
                scene.check(id, cancel)?;
                scene.check_resources(&input.resource_packs)?;
                Ok(
                    json!({"sceneHandle":handle,"generation":scene.generation,"requestId":id,"hit":hit}),
                )
            }
            _ => bail!("Unknown layer operation: {}", input.action),
        }
    }
    fn open(&self, input: &Input, cancel: &AtomicBool) -> Result<Value> {
        ensure!(!cancel.load(Ordering::Relaxed), "Layer operation cancelled");
        let source = SourceStamp::read(Path::new(
            input
                .source_path
                .as_deref()
                .context("Layer source path is missing")?,
        ))?;
        // Page lifetime is not source lifetime. Keep at most two byte-bounded,
        // immutable packed catalogs for this worker session; active scene pins
        // are separate and are never invalidated by cache eviction.
        let cached = self
            .sources
            .lock()
            .unwrap()
            .iter()
            .find(|entry| entry.source == source)
            .cloned();
        if let Some(cached) = cached {
            return self.pin(cached, input);
        }
        let (root, catalog) = visual::load_visual_catalog(&source.path)?;
        ensure!(!cancel.load(Ordering::Relaxed), "Layer operation cancelled");
        let db = crate::block_database::database()?;
        let data_version = root
            .minecraft_data_version
            .context("Projection has no Minecraft data version; convert it in Properties first")?;
        // Exact data version only; a recent resource selection is not the file's version.
        let versions = db.query(None, "", None)?;
        let version = versions["versions"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(
                versions["mapColorVersions"]
                    .as_array()
                    .into_iter()
                    .flatten(),
            )
            .find(|v| {
                v["dataVersion"].as_i64() == Some(i64::from(data_version))
                    && v["blockCount"].as_u64().unwrap_or(0) > 0
            })
            .and_then(|v| v["version"].as_str())
            .context("No exact block database for the projection data version")?
            .to_owned();
        let mut regions = Vec::new();
        for (name, region) in &root.regions {
            ensure!(!cancel.load(Ordering::Relaxed), "Layer operation cancelled");
            regions.push(RegionIndex {
                name: name.clone(),
                layout: nbt::choose_palette_packing(
                    &region.block_states,
                    nbt::region_volume(&region.size)?,
                    nbt::bits_for_palette(region.block_state_palette.len()),
                    region.block_state_palette.len(),
                )?,
                palette: region
                    .block_state_palette
                    .iter()
                    .map(|block| {
                        catalog
                            .state_lookup
                            .get(&visual::format_block_state(block))
                            .copied()
                    })
                    .collect(),
            });
        }
        let mut file = fs::File::open(&source.path)?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            ensure!(!cancel.load(Ordering::Relaxed), "Layer operation cancelled");
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        ensure!(
            SourceStamp::read(&source.path)? == source,
            "Layer source changed during open"
        );
        let weight = root
            .regions
            .values()
            .map(|region| region.block_states.len().saturating_mul(8))
            .sum::<usize>()
            .saturating_add(catalog.palette.len().saturating_mul(256));
        let cached = CachedSource {
            source,
            content_id: format!("{:x}", digest.finalize()),
            root: Arc::new(root),
            catalog: Arc::new(catalog),
            regions: Arc::new(regions),
            version,
            weight,
        };
        if weight <= SOURCE_CACHE_BYTES {
            let mut entries = self.sources.lock().unwrap();
            entries.retain(|entry| entry.source.path != cached.source.path);
            while !entries.is_empty()
                && (entries.len() >= 2
                    || entries
                        .iter()
                        .map(|entry| entry.weight)
                        .sum::<usize>()
                        .saturating_add(weight)
                        > SOURCE_CACHE_BYTES)
            {
                entries.pop_front();
            }
            entries.push_back(cached.clone());
        }
        self.pin(cached, input)
    }
    fn pin(&self, cached: CachedSource, input: &Input) -> Result<Value> {
        let resource_context = crate::layer_resources::LayerResourceContext::resolve(
            &cached.version,
            &input.resource_packs,
        )?;
        let assets = hints::HintAssets::new(
            input
                .storage_dir
                .clone()
                .unwrap_or(crate::runtime_paths::data_root()?),
            &cached.version,
        )?;
        let generation = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let handle = format!("layer-{}-{generation}", std::process::id());
        let scene = Arc::new(Scene {
            generation,
            source: cached.source,
            content_id: cached.content_id,
            resource_context,
            resource_packs: input.resource_packs.clone(),
            root: cached.root,
            catalog: cached.catalog,
            regions: cached.regions,
            version: cached.version,
            assets,
            latest: AtomicU64::new(0),
            cancelled: AtomicU64::new(0),
            released: AtomicBool::new(false),
            visual_generation: AtomicU64::new(1),
            sprites: Mutex::new(VecDeque::new()),
        });
        let meta = scene.meta(&handle);
        self.scenes.lock().unwrap().insert(handle, scene);
        Ok(meta)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
    x: i32,
    y: i32,
    z: i32,
    world_x: i32,
    world_y: i32,
    world_z: i32,
    palette_id: usize,
    id: String,
    name: String,
    states: String,
    state_record: BTreeMap<String, String>,
    original_state_record: BTreeMap<String, String>,
    has_states: bool,
    minecraft_version: String,
    region: String,
    property_options: BTreeMap<String, Vec<String>>,
}
impl Scene {
    fn resource_identity(&self) -> String {
        format!(
            "{}:{}:{}",
            self.version,
            self.resource_context.identity(),
            self.assets.revision()
        )
    }
    fn check_resources(&self, packs: &[PathBuf]) -> Result<()> {
        ensure!(
            packs == self.resource_packs,
            "LayerResourceChanged: Effective resource pack order changed; reopen the source"
        );
        ensure!(
            self.resource_context
                .is_current()
                .context("LayerResourceChanged: Inspect versioned resource identity")?,
            "LayerResourceChanged: Versioned model or texture resources changed; reopen the source"
        );
        ensure!(
            self.assets
                .is_current()
                .context("LayerResourceChanged: Inspect layer annotation identity")?,
            "LayerResourceChanged: Layer icon or state-hint assets changed; reopen the source"
        );
        Ok(())
    }
    fn min(&self) -> [i32; 3] {
        self.catalog
            .bounds
            .map_or([0; 3], |b| [b.min_x, b.min_y, b.min_z])
    }
    fn meta(&self, handle: &str) -> Value {
        let size = &self.catalog.metadata.enclosing_size;
        let min = self.min();
        let origin = self
            .catalog
            .bounds
            .filter(|b| {
                (0..3).all(|i| min[i] <= 0)
                    && b.max_x >= 0
                    && b.max_y >= 0
                    && b.max_z >= 0
                    && min != [0; 3]
            })
            .map(|_| json!({"x":-min[0],"y":-min[1],"z":-min[2]}));
        json!({"sceneHandle":handle,"generation":self.generation,"sourceIdentity":self.content_id,"resourceIdentity":self.resource_identity(),"visualGeneration":self.visual_generation.load(Ordering::Acquire),
            "meta":{"size_x":size.x,"size_y":size.y,"size_z":size.z,"min_x":min[0],"min_y":min[1],"min_z":min[2],
            "minecraft_data_version":self.root.minecraft_data_version,"version":self.version,"name":self.catalog.metadata.name},"origin":origin})
    }
    fn check(&self, id: u64, cancel: &AtomicBool) -> Result<()> {
        ensure!(
            !self.released.load(Ordering::Acquire),
            "Layer scene released"
        );
        ensure!(
            !cancel.load(Ordering::Relaxed) && id > self.cancelled.load(Ordering::Acquire),
            "Layer operation cancelled"
        );
        ensure!(
            self.latest.load(Ordering::Acquire) == id,
            "Layer request superseded"
        );
        Ok(())
    }
    fn cells(
        &self,
        y: i32,
        rect: ViewRect,
        id: u64,
        cancel: &AtomicBool,
    ) -> Result<BTreeMap<(i32, i32), usize>> {
        let mut cells = BTreeMap::new();
        let min = self.min();
        let world_y = i64::from(y) + i64::from(min[1]);
        for index in self.regions.iter() {
            self.check(id, cancel)?;
            let region = &self.root.regions[&index.name];
            let bounds = nbt::region_bounds(region);
            let relative_y = world_y - i64::from(region.position.y);
            let Some(sy) = i32::try_from(relative_y)
                .ok()
                .and_then(|value| nbt::region_coord_to_storage_coord(value, region.size.y))
            else {
                continue;
            };
            let x0 = (i64::from(rect.x) + i64::from(min[0])).max(i64::from(bounds.min_x));
            let x1 = (i64::from(rect.x) + i64::from(rect.width) + i64::from(min[0]))
                .min(i64::from(bounds.max_x) + 1);
            let z0 = (i64::from(rect.z) + i64::from(min[2])).max(i64::from(bounds.min_z));
            let z1 = (i64::from(rect.z) + i64::from(rect.height) + i64::from(min[2]))
                .min(i64::from(bounds.max_z) + 1);
            if x0 >= x1 || z0 >= z1 {
                continue;
            }
            let width = i64::from(region.size.x).unsigned_abs() as usize;
            let length = i64::from(region.size.z).unsigned_abs() as usize;
            let zero_is_air = index.palette.first().is_some_and(Option::is_none);
            for world_z in z0..z1 {
                self.check(id, cancel)?;
                let sz = (world_z - i64::from(bounds.min_z)) as usize;
                let start_x = (x0 - i64::from(bounds.min_x)) as usize;
                let start = (sy * length + sz) * width + start_x;
                let end = start + (x1 - x0) as usize;
                index.layout.visit_palette_range(
                    &region.block_states,
                    start,
                    end,
                    zero_is_air,
                    |position, palette| {
                        if let Some(Some(global)) = index.palette.get(palette) {
                            let x = (x0 + (position - start) as i64 - i64::from(min[0])) as i32;
                            let z = (world_z - i64::from(min[2])) as i32;
                            cells.insert((x, z), *global);
                        }
                    },
                );
            }
        }
        Ok(cells)
    }
    fn hit(
        &self,
        x: i32,
        y: i32,
        z: i32,
        overrides: &BTreeMap<String, BTreeMap<String, String>>,
        locale: &str,
    ) -> Result<Option<Hit>> {
        let size = &self.catalog.metadata.enclosing_size;
        if x < 0 || y < 0 || z < 0 || x >= size.x || y >= size.y || z >= size.z {
            return Ok(None);
        }
        let min = self.min();
        let world = [x + min[0], y + min[1], z + min[2]];
        // Deterministic region precedence shared by drawing and hit-testing:
        // later region name wins, AIR never erases an earlier selected region.
        for index in self.regions.iter().rev() {
            let region = &self.root.regions[&index.name];
            let Some(sx) =
                nbt::region_coord_to_storage_coord(world[0] - region.position.x, region.size.x)
            else {
                continue;
            };
            let Some(sy) =
                nbt::region_coord_to_storage_coord(world[1] - region.position.y, region.size.y)
            else {
                continue;
            };
            let Some(sz) =
                nbt::region_coord_to_storage_coord(world[2] - region.position.z, region.size.z)
            else {
                continue;
            };
            let width = i64::from(region.size.x).unsigned_abs() as usize;
            let length = i64::from(region.size.z).unsigned_abs() as usize;
            let palette = index
                .layout
                .palette_index(&region.block_states, (sy * length + sz) * width + sx);
            let block = region
                .block_state_palette
                .get(palette)
                .context("Invalid layer palette index")?;
            if visual::is_air_state(&block.name) {
                continue;
            }
            let original = visual::format_block_state(block);
            let state_record = overrides
                .get(&format!("{x}:{y}:{z}"))
                .cloned()
                .unwrap_or_else(|| block.properties.clone());
            let state = state_key(&block.name, &state_record);
            let db = crate::block_database::database()?;
            let options = db
                .registry(&self.version)
                .ok()
                .and_then(|r| r.blocks.get(&block.name))
                .map(|b| b.properties.clone())
                .unwrap_or_default();
            if state != original {
                ensure!(
                    db.registry(&self.version)?
                        .blocks
                        .get(&block.name)
                        .is_some_and(|b| b.accepts(&state, &state_record)),
                    "Invalid edited block state for the source version"
                );
            }
            return Ok(Some(Hit {
                x,
                y,
                z,
                world_x: world[0],
                world_y: world[1],
                world_z: world[2],
                palette_id: *self
                    .catalog
                    .state_lookup
                    .get(&original)
                    .context("Layer palette is missing")?,
                name: if locale.starts_with("en") {
                    db.registry(&self.version)
                        .ok()
                        .and_then(|r| r.blocks.get(&block.name))
                        .map(|b| b.en.clone())
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| block.name.clone())
                } else {
                    db.name(&self.version, &block.name)
                },
                id: block.name.clone(),
                states: state,
                has_states: !block.properties.is_empty(),
                state_record,
                original_state_record: block.properties.clone(),
                minecraft_version: self.version.clone(),
                region: index.name.clone(),
                property_options: options,
            }));
        }
        Ok(None)
    }
    fn sprite(&self, state: &str, hints: bool) -> Result<SpriteVisual> {
        let epoch = self.visual_generation.load(Ordering::Acquire);
        let key = format!("{}:{epoch}:{hints}:{state}", self.resource_identity());
        if let Some((_, image)) = self
            .sprites
            .lock()
            .unwrap()
            .iter()
            .filter(|_| self.assets.can_cache())
            .find(|(known, _)| known == &key)
        {
            return Ok(SpriteVisual {
                state: state.into(),
                image: image.clone(),
                transparent: image.pixels().all(|pixel| pixel.0[3] == 0),
                issue: None,
            });
        }
        let _scope = crate::layer_resources::enter(self.resource_context.clone());
        let issue = |kind: &str, resource_key: Option<String>, message: String, retryable: bool| {
            VisualIssue {
                state: state.into(),
                kind: kind.into(),
                resource_key,
                message,
                retryable,
                block_count: 0,
            }
        };
        let produced = (|| -> std::result::Result<RgbaImage, VisualIssue> {
            let db = crate::block_database::database()
                .map_err(|error| issue("database_unavailable", None, format!("{error:#}"), true))?;
            db.layer_top_sprite(&self.version, state).map_err(|error| {
                issue(
                    error.kind,
                    error.resource_key,
                    error.message,
                    error.retryable,
                )
            })
        })();
        let (mut image, mut failure) = match produced {
            Ok(image) => (image, None),
            Err(error) => {
                // An explicitly diagnosed error is not a successful grey sprite.
                // Keep it local to this response, so the next view retries Core.
                let mut marker = RgbaImage::from_pixel(32, 32, Rgba([128, 128, 128, 255]));
                for index in 0..32 {
                    marker.put_pixel(index, index, Rgba([32, 32, 32, 255]));
                }
                (marker, Some(error))
            }
        };
        if failure.is_none() && image.pixels().any(|pixel| pixel.0[3] != 0) {
            match self.assets.apply(&image, state, hints) {
                Ok(annotated) => image = annotated,
                Err(error) => {
                    // Preserve the real top image if an optional annotation fails.
                    let detail = error.downcast_ref::<hints::HintAssetError>();
                    failure = Some(issue(
                        detail.map_or("hint_asset_failure", |value| value.kind),
                        detail.map(|value| value.resource_key.clone()),
                        format!("{error:#}"),
                        detail.is_none_or(|value| value.retryable),
                    ));
                }
            }
        }
        let image = Arc::new(image);
        let transparent = image.pixels().all(|pixel| pixel.0[3] == 0);
        // Cache only complete success. Missing source assets, decode errors and
        // temporary reads stay distinct diagnostics and may recover next request.
        if failure.is_none()
            && self.assets.can_cache()
            && epoch == self.visual_generation.load(Ordering::Acquire)
        {
            let bytes = image.as_raw().len();
            let mut cache = self.sprites.lock().unwrap();
            while !cache.is_empty()
                && cache
                    .iter()
                    .map(|(_, image)| image.as_raw().len())
                    .sum::<usize>()
                    .saturating_add(bytes)
                    > SPRITE_CACHE_BYTES
            {
                cache.pop_front();
            }
            if bytes <= SPRITE_CACHE_BYTES {
                cache.push_back((key, image.clone()));
            }
        }
        Ok(SpriteVisual {
            state: state.into(),
            image,
            transparent,
            issue: failure,
        })
    }
    fn render(&self, handle: &str, input: &Input, id: u64, cancel: &AtomicBool) -> Result<Value> {
        let y = input.y.context("Layer Y is missing")?;
        ensure!(
            y >= 0 && y < self.catalog.metadata.enclosing_size.y,
            "Layer Y is outside the projection"
        );
        let rect = input.view_rect.context("Layer view rectangle is missing")?;
        ensure!(
            rect.width > 0
                && rect.height > 0
                && rect.width <= MAX_REGION_EDGE
                && rect.height <= MAX_REGION_EDGE,
            "Layer view rectangle exceeds the bounded tile extent"
        );
        ensure!(
            rect.x >= 0
                && rect.z >= 0
                && rect.x.checked_add(rect.width).is_some()
                && rect.z.checked_add(rect.height).is_some(),
            "Invalid layer view coordinates"
        );
        let scale = input.scale.context("Layer view scale is missing")?;
        ensure!(
            scale.is_finite() && scale > 0. && scale <= 128.,
            "Invalid layer view scale"
        );
        let ratio =
            (MAX_IMAGE_EDGE as f64 / (f64::from(rect.width.max(rect.height)) * scale)).min(1.);
        let pixel_scale = scale * ratio;
        let width = (f64::from(rect.width) * pixel_scale).ceil().max(1.) as u32;
        let height = (f64::from(rect.height) * pixel_scale).ceil().max(1.) as u32;
        let mut image = RgbaImage::new(width, height);
        let mut count = 0usize;
        let epoch = self.visual_generation.load(Ordering::Acquire);
        let mut issues = BTreeMap::<String, VisualIssue>::new();
        let mut ready = std::collections::BTreeSet::new();
        let mut transparent = std::collections::BTreeSet::new();
        let mut palette_sprites = BTreeMap::<usize, SpriteVisual>::new();
        let mut edited_sprites = BTreeMap::<String, SpriteVisual>::new();
        for depth in (0..=input.onion_depth.min(5) as i32).rev() {
            if y - depth < 0 {
                continue;
            }
            let opacity = if depth == 0 {
                1.
            } else {
                (f64::from(input.onion_depth.min(5)) + 1. - f64::from(depth))
                    / (f64::from(input.onion_depth.min(5)) + 1.)
            };
            // Decode only a 16-row sparse band at once. Even a 4096-block
            // viewport cannot inflate a second full-layer object catalog.
            for band_z in (rect.z..rect.z + rect.height).step_by(16) {
                let band = ViewRect {
                    x: rect.x,
                    z: band_z,
                    width: rect.width,
                    height: (rect.z + rect.height - band_z).min(16),
                };
                let cells = self.cells(y - depth, band, id, cancel)?;
                if depth == 0 {
                    count += cells.len();
                }
                for ((x, z), palette_id) in cells {
                    self.check(id, cancel)?;
                    let entry = &self.catalog.palette[palette_id];
                    let edited = input.overrides.get(&format!("{x}:{}:{z}", y - depth));
                    let visual = if let Some(properties) = edited {
                        let state = state_key(&entry.block_id, properties);
                        if !edited_sprites.contains_key(&state) {
                            if edited_sprites.len() >= 512 {
                                edited_sprites.pop_first();
                            }
                            let db = crate::block_database::database()?;
                            ensure!(
                                db.registry(&self.version)?
                                    .blocks
                                    .get(&entry.block_id)
                                    .is_some_and(|b| b.accepts(&state, properties)),
                                "Invalid edited block state for the source version"
                            );
                            edited_sprites
                                .insert(state.clone(), self.sprite(&state, input.state_hints)?);
                        }
                        edited_sprites[&state].clone()
                    } else {
                        if !palette_sprites.contains_key(&palette_id) {
                            if palette_sprites.len() >= 512 {
                                palette_sprites.pop_first();
                            }
                            let state = state_key(
                                &entry.block_id,
                                &self.catalog.property_pool[entry.property_id],
                            );
                            palette_sprites
                                .insert(palette_id, self.sprite(&state, input.state_hints)?);
                        }
                        palette_sprites[&palette_id].clone()
                    };
                    if let Some(issue) = visual.issue {
                        let known = issues.entry(issue.state.clone()).or_insert(issue);
                        known.block_count += 1;
                    } else {
                        ready.insert(visual.state.clone());
                        if visual.transparent {
                            transparent.insert(visual.state.clone());
                        }
                    }
                    let sprite = visual.image;
                    // Use the same half-open edge quantizer on both sides.
                    // floor(left)/ceil(right) draws shared pixels twice at
                    // fractional zoom and incorrectly increases material alpha.
                    let left = (f64::from(x - rect.x) * pixel_scale).ceil() as u32;
                    let top = (f64::from(z - rect.z) * pixel_scale).ceil() as u32;
                    let right = (f64::from(x + 1 - rect.x) * pixel_scale)
                        .ceil()
                        .min(width as f64) as u32;
                    let bottom = (f64::from(z + 1 - rect.z) * pixel_scale)
                        .ceil()
                        .min(height as f64) as u32;
                    for py in top..bottom {
                        for px in left..right {
                            let mut color = *sprite.get_pixel(
                                ((px - left) * sprite.width() / (right - left).max(1))
                                    .min(sprite.width() - 1),
                                ((py - top) * sprite.height() / (bottom - top).max(1))
                                    .min(sprite.height() - 1),
                            );
                            color.0[3] = (f64::from(color.0[3]) * opacity).round() as u8;
                            use image::Pixel;
                            image.get_pixel_mut(px, py).blend(&color);
                        }
                    }
                }
            }
        }
        self.check(id, cancel)?;
        ensure!(
            epoch == self.visual_generation.load(Ordering::Acquire),
            "Layer visual request superseded"
        );
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )?;
        self.check(id, cancel)?;
        ensure!(
            epoch == self.visual_generation.load(Ordering::Acquire),
            "Layer visual request superseded"
        );
        Ok(
            json!({"sceneHandle":handle,"generation":self.generation,"requestId":id,"y":y,"worldY":y+self.min()[1],
            "viewRect":rect,"pixelWidth":width,"pixelHeight":height,"blockCount":count,"missingVisuals":issues.len(),
            "resourceIdentity":self.resource_identity(),"visualGeneration":epoch,
            "visualDiagnostics":{"readyStates":ready.len(),"transparentStates":transparent.len(),"missingStates":issues.values().filter(|issue|issue.kind.starts_with("missing_")).count(),"failedStates":issues.values().filter(|issue|!issue.kind.starts_with("missing_")).count(),"retryableStates":issues.values().filter(|issue|issue.retryable).count(),"issues":issues.values().collect::<Vec<_>>()},
            "imageUrl":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png))}),
        )
    }
}

fn state_key(id: &str, properties: &BTreeMap<String, String>) -> String {
    if properties.is_empty() {
        id.to_owned()
    } else {
        format!(
            "{id}[{}]",
            properties
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Scene {
        use crate::model::BlockStateNbt;
        use crate::nbt::{MetadataNbt, RegionNbt, Vec3i};
        let region = |position, size, block: &str, count, occupied| {
            let mut indices = vec![0; count];
            indices[occupied] = 1;
            RegionNbt {
                position,
                size,
                block_state_palette: vec![
                    BlockStateNbt {
                        name: "minecraft:air".into(),
                        properties: BTreeMap::new(),
                    },
                    BlockStateNbt {
                        name: block.into(),
                        properties: BTreeMap::new(),
                    },
                ],
                block_states: nbt::pack_palette_indices(&indices, 2),
                entities: vec![],
                tile_entities: vec![],
            }
        };
        let root = LitematicRoot {
            metadata: MetadataNbt {
                author: None,
                description: None,
                name: None,
                time_created: None,
                time_modified: None,
                total_blocks: None,
                total_volume: None,
                region_count: None,
                enclosing_size: None,
                preview_image_data: None,
            },
            regions: BTreeMap::from([
                (
                    "a_base".into(),
                    region(
                        Vec3i {
                            x: -3,
                            y: -6,
                            z: -2,
                        },
                        Vec3i { x: 2, y: 3, z: 2 },
                        "minecraft:stone",
                        12,
                        0,
                    ),
                ),
                (
                    "b_overlap".into(),
                    region(
                        Vec3i {
                            x: -3,
                            y: -6,
                            z: -2,
                        },
                        Vec3i { x: 1, y: 1, z: 1 },
                        "minecraft:gold_block",
                        1,
                        0,
                    ),
                ),
                (
                    "c_negative".into(),
                    region(
                        Vec3i {
                            x: -2,
                            y: -4,
                            z: -1,
                        },
                        Vec3i {
                            x: -2,
                            y: -3,
                            z: -2,
                        },
                        "minecraft:glass",
                        12,
                        11,
                    ),
                ),
            ]),
            version: 6,
            sub_version: None,
            minecraft_data_version: Some(3955),
            preview_image_data: None,
        };
        let bounds = visual::compute_bounds(&root);
        let palette = ["minecraft:stone", "minecraft:gold_block", "minecraft:glass"]
            .iter()
            .map(|id| crate::model::PaletteEntryOutput {
                block_id: (*id).into(),
                property_id: 0,
            })
            .collect();
        let state_lookup = std::collections::HashMap::from([
            ("minecraft:stone".into(), 0),
            ("minecraft:gold_block".into(), 1),
            ("minecraft:glass".into(), 2),
        ]);
        let catalog = VisualCatalog {
            metadata: visual::build_metadata_output(&root, bounds),
            bounds,
            palette,
            property_pool: vec![BTreeMap::new()],
            state_lookup,
        };
        let regions = root
            .regions
            .iter()
            .map(|(name, region)| RegionIndex {
                name: name.clone(),
                layout: nbt::choose_palette_packing(
                    &region.block_states,
                    nbt::region_volume(&region.size).unwrap(),
                    2,
                    2,
                )
                .unwrap(),
                palette: vec![
                    None,
                    catalog
                        .state_lookup
                        .get(&region.block_state_palette[1].name)
                        .copied(),
                ],
            })
            .collect();
        Scene {
            generation: 1,
            source: SourceStamp {
                path: PathBuf::new(),
                bytes: 0,
                modified: SystemTime::UNIX_EPOCH,
            },
            content_id: "fixture".into(),
            resource_context: crate::layer_resources::LayerResourceContext::resolve("1.21.1", &[])
                .unwrap(),
            resource_packs: vec![],
            root: Arc::new(root),
            catalog: Arc::new(catalog),
            regions: Arc::new(regions),
            version: "1.21.1".into(),
            assets: hints::HintAssets::fixture(),
            latest: AtomicU64::new(7),
            cancelled: AtomicU64::new(0),
            released: AtomicBool::new(false),
            visual_generation: AtomicU64::new(1),
            sprites: Mutex::new(VecDeque::new()),
        }
    }
    #[test]
    fn canonical_state_order_is_not_locale_dependent() {
        assert_eq!(
            state_key(
                "minecraft:oak_log",
                &BTreeMap::from([("axis".into(), "y".into())])
            ),
            "minecraft:oak_log[axis=y]"
        );
    }
    #[test]
    fn real_transparent_sprite_is_success_and_missing_resource_is_never_positive_cached() {
        let scene = fixture();
        let clear = scene.sprite("minecraft:air", false).unwrap();
        assert!(clear.issue.is_none());
        assert!(clear.transparent);
        assert!(clear.image.pixels().all(|pixel| pixel.0[3] == 0));
        assert_eq!(scene.sprites.lock().unwrap().len(), 1);
        for _ in 0..2 {
            let missing = scene.sprite("nova_missing_resource:absent", false).unwrap();
            let issue = missing
                .issue
                .expect("Missing source must remain diagnostic");
            assert_eq!(issue.state, "nova_missing_resource:absent");
            assert!(!issue.kind.is_empty());
            assert_eq!(scene.sprites.lock().unwrap().len(), 1);
        }
        let cached_clear = scene.sprite("minecraft:air", false).unwrap();
        assert!(cached_clear.issue.is_none() && cached_clear.transparent);
        assert!(Arc::ptr_eq(&clear.image, &cached_clear.image));
    }
    #[test]
    fn visual_invalidation_clears_success_cache_and_advances_epoch_without_reparsing() {
        let service = LayerService::default();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("visual-epoch.litematic");
        let mut scene = fixture();
        nbt::save_litematic_root(&path, &scene.root).unwrap();
        scene.source = SourceStamp::read(&path).unwrap();
        let scene = Arc::new(scene);
        scene.sprite("minecraft:stone", false).unwrap();
        assert_eq!(scene.sprites.lock().unwrap().len(), 1);
        let packed_source = scene.root.clone();
        service
            .scenes
            .lock()
            .unwrap()
            .insert("fixture".into(), scene.clone());
        let request = json!({"action":"invalidateVisuals","sceneHandle":"fixture","generation":1});
        let before = scene.visual_generation.load(Ordering::Acquire);
        service.prepare(&request);
        let receipt = service.execute(&request, &AtomicBool::new(false)).unwrap();
        assert!(receipt["visualGeneration"].as_u64().unwrap() > before);
        assert!(scene.sprites.lock().unwrap().is_empty());
        assert!(Arc::ptr_eq(&packed_source, &scene.root));
        let changed = scene
            .check_resources(&[PathBuf::from("new-effective-pack.zip")])
            .unwrap_err();
        assert!(changed.to_string().starts_with("LayerResourceChanged:"));
    }
    #[test]
    fn real_hint_decode_failure_preserves_top_pixels_and_recovers_after_resource_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let overlay = directory.path().join("flake/state_overlay/axis_x.png");
        fs::create_dir_all(overlay.parent().unwrap()).unwrap();
        fs::write(&overlay, b"interrupted PNG download").unwrap();
        let mut scene = fixture();
        scene.assets = hints::HintAssets::new(directory.path().to_owned(), &scene.version).unwrap();
        let state = "minecraft:oak_log[axis=x]";
        let plain = scene.sprite(state, false).unwrap();
        assert!(plain.issue.is_none());
        assert!(plain.image.pixels().any(|pixel| pixel.0[3] != 0));
        let failed = scene.sprite(state, true).unwrap();
        let issue = failed.issue.unwrap();
        assert_eq!(issue.kind, "hint_decode_failed");
        assert!(issue.retryable);
        assert_eq!(
            issue.resource_key.as_deref(),
            Some(overlay.to_str().unwrap())
        );
        assert_eq!(failed.image.as_raw(), plain.image.as_raw());
        assert_eq!(scene.sprites.lock().unwrap().len(), 1);
        let source = crate::runtime_paths::app_root()
            .unwrap()
            .join("pack-in/flake/state_overlay/axis_x.png");
        fs::copy(&source, &overlay).unwrap();
        assert!(!scene.assets.is_current().unwrap());
        // The same request is retryable, but cannot silently bless a changed
        // generation and keep other previously cached sprites.
        let retried = scene.sprite(state, true).unwrap();
        assert!(retried.issue.is_none());
        assert!(!scene.assets.is_current().unwrap());
        scene.assets = hints::HintAssets::new(directory.path().to_owned(), &scene.version).unwrap();
        scene.sprites.lock().unwrap().clear();
        let recovered = scene.sprite(state, true).unwrap();
        assert!(recovered.issue.is_none());
        assert!(scene.assets.is_current().unwrap());
        assert_eq!(scene.sprites.lock().unwrap().len(), 1);
        assert!(Arc::ptr_eq(
            &recovered.image,
            &scene.sprite(state, true).unwrap().image
        ));
    }
    #[test]
    fn layer_sampling_keeps_real_glass_alpha_at_low_zoom_and_fractional_edges() {
        use image::Pixel;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("glass-alpha.litematic");
        let mut root = Arc::try_unwrap(fixture().root).ok().unwrap();
        root.regions.retain(|name, _| name == "a_base");
        let region = root.regions.get_mut("a_base").unwrap();
        region.position = crate::nbt::Vec3i { x: 0, y: 0, z: 0 };
        region.size = crate::nbt::Vec3i { x: 2, y: 2, z: 1 };
        region.block_state_palette[1].name = "minecraft:white_stained_glass".into();
        region.block_states = nbt::pack_palette_indices(&[1, 1, 1, 1], 2);
        nbt::save_litematic_root(&path, &root).unwrap();
        let service = LayerService::default();
        let cancel = AtomicBool::new(false);
        let opened = service
            .execute(&json!({"action":"openScene","sourcePath":path}), &cancel)
            .unwrap();
        let handle = opened["sceneHandle"].as_str().unwrap();
        let generation = opened["generation"].as_u64().unwrap();
        let scene = service.scenes.lock().unwrap()[handle].clone();
        let sprite = scene.sprite("minecraft:white_stained_glass", true).unwrap();
        assert!(sprite.issue.is_none());
        let center = *sprite
            .image
            .get_pixel(sprite.image.width() / 2, sprite.image.height() / 2);
        assert!(center.0[3] > 0 && center.0[3] < 255);
        let decode = |view: &Value| {
            image::load_from_memory_with_format(
                &base64::engine::general_purpose::STANDARD
                    .decode(
                        view["imageUrl"]
                            .as_str()
                            .unwrap()
                            .split_once(',')
                            .unwrap()
                            .1,
                    )
                    .unwrap(),
                image::ImageFormat::Png,
            )
            .unwrap()
            .to_rgba8()
        };
        for (id, scale) in [(1, 4.), (2, 16.), (3, 1.5)] {
            let view = service.execute(&json!({"action":"requestLayerView","sceneHandle":handle,"generation":generation,
                "requestId":id,"y":1,"viewRect":{"x":0,"z":0,"width":2,"height":1},"scale":scale,"stateHints":true}), &cancel).unwrap();
            assert_eq!(view["missingVisuals"], 0);
            let rendered = decode(&view);
            if scale >= 4. {
                assert_eq!(
                    *rendered.get_pixel((scale / 2.) as u32, (scale / 2.) as u32),
                    center
                );
            }
            let source_max = sprite.image.pixels().map(|pixel| pixel.0[3]).max().unwrap();
            assert!(rendered.pixels().all(|pixel| pixel.0[3] <= source_max));
        }
        let view = service.execute(&json!({"action":"requestLayerView","sceneHandle":handle,"generation":generation,
            "requestId":4,"y":1,"onionDepth":1,"viewRect":{"x":0,"z":0,"width":2,"height":1},"scale":4.,"stateHints":true}), &cancel).unwrap();
        let mut lower = center;
        lower.0[3] = (f64::from(lower.0[3]) * 0.5).round() as u8;
        let mut expected = Rgba([0, 0, 0, 0]);
        expected.blend(&lower);
        expected.blend(&center);
        assert_eq!(*decode(&view).get_pixel(2, 2), expected);
    }
    #[test]
    fn input_rejects_unbounded_unknown_routing_fields() {
        assert!(
            serde_json::from_value::<Input>(json!({"action":"openScene","viewerMode":"full"}))
                .is_err()
        );
    }
    #[test]
    fn monotonic_requests_never_revive_cancelled_or_old_layers() {
        let latest = AtomicU64::new(10);
        assert_eq!(latest.fetch_max(8, Ordering::AcqRel), 10);
        assert_eq!(latest.load(Ordering::Acquire), 10);
        assert_eq!(latest.fetch_max(11, Ordering::AcqRel), 10);
        assert_eq!(latest.load(Ordering::Acquire), 11);
    }
    #[test]
    fn negative_y_negative_sizes_overlap_and_empty_layer_keep_one_packed_source() {
        let scene = fixture();
        let cancel = AtomicBool::new(false);
        let rect = ViewRect {
            x: 0,
            z: 0,
            width: 2,
            height: 2,
        };
        assert_eq!(scene.min(), [-3, -6, -2]);
        assert_eq!(scene.meta("fixture")["meta"]["min_y"], -6);
        assert_eq!(
            scene.cells(0, rect, 7, &cancel).unwrap(),
            BTreeMap::from([((0, 0), 1)])
        );
        assert!(scene.cells(1, rect, 7, &cancel).unwrap().is_empty());
        assert_eq!(
            scene.cells(2, rect, 7, &cancel).unwrap(),
            BTreeMap::from([((1, 1), 2)])
        );
        assert!(scene
            .cells(
                2,
                ViewRect {
                    x: 0,
                    z: 0,
                    width: 1,
                    height: 1
                },
                7,
                &cancel
            )
            .unwrap()
            .is_empty());
        assert_eq!(scene.regions.len(), 3);
    }
    #[test]
    fn reader_supersedes_running_view_before_the_next_lane_executes() {
        let service = LayerService::default();
        let scene = Arc::new(fixture());
        service
            .scenes
            .lock()
            .unwrap()
            .insert("fixture".into(), scene.clone());
        let cancel = AtomicBool::new(false);
        service.prepare(&json!({"action":"requestLayerView","sceneHandle":"fixture","generation":1,"requestId":8}));
        assert!(scene
            .check(7, &cancel)
            .unwrap_err()
            .to_string()
            .contains("superseded"));
        assert!(scene.check(8, &cancel).is_ok());
        service.prepare(
            &json!({"action":"cancel","sceneHandle":"fixture","generation":1,"requestId":8}),
        );
        assert!(scene
            .check(8, &cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
        service.prepare(&json!({"action":"requestLayerView","sceneHandle":"fixture","generation":1,"requestId":9}));
        assert!(scene.check(9, &cancel).is_ok());
        service.prepare(&json!({"action":"release","sceneHandle":"fixture","generation":1}));
        assert!(scene
            .check(9, &cancel)
            .unwrap_err()
            .to_string()
            .contains("released"));
    }
    #[test]
    fn headless_open_png_hit_cancel_and_release_share_real_source_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("negative.litematic");
        nbt::save_litematic_root(&path, &fixture().root).unwrap();
        let service = LayerService::default();
        let cancel = AtomicBool::new(false);
        let opened = service
            .execute(&json!({"action":"openScene","sourcePath":path}), &cancel)
            .unwrap();
        let handle = opened["sceneHandle"].as_str().unwrap();
        let generation = opened["generation"].as_u64().unwrap();
        let view=service.execute(&json!({"action":"requestLayerView","sceneHandle":handle,"generation":generation,"requestId":1,
            "y":0,"viewRect":{"x":0,"z":0,"width":2,"height":2},"scale":16.,"stateHints":true}),&cancel).unwrap();
        assert_eq!(view["worldY"], -6);
        assert_eq!(view["blockCount"], 1);
        let png = base64::engine::general_purpose::STANDARD
            .decode(
                view["imageUrl"]
                    .as_str()
                    .unwrap()
                    .split_once(',')
                    .unwrap()
                    .1,
            )
            .unwrap();
        let image = image::load_from_memory(&png).unwrap();
        assert_eq!((image.width(), image.height()), (32, 32));
        let hit=service.execute(&json!({"action":"hitTest","sceneHandle":handle,"generation":generation,"requestId":1,"x":0,"y":0,"z":0}),&cancel).unwrap();
        assert_eq!(hit["hit"]["worldY"], -6);
        assert_eq!(hit["hit"]["id"], "minecraft:gold_block");
        assert_eq!(hit["hit"]["region"], "b_overlap");
        let empty=service.execute(&json!({"action":"requestLayerView","sceneHandle":handle,"generation":generation,"requestId":2,
            "y":1,"viewRect":{"x":0,"z":0,"width":2,"height":2},"scale":1.}),&cancel).unwrap();
        assert_eq!(empty["blockCount"], 0);
        assert_eq!(
            service.scenes.lock().unwrap()[handle].content_id,
            opened["sourceIdentity"].as_str().unwrap()
        );
        assert!(service.execute(&json!({"action":"requestLayerView","sceneHandle":handle,"generation":generation,"requestId":1,
            "y":0,"viewRect":{"x":0,"z":0,"width":2,"height":2},"scale":1.}),&cancel).is_err());
        service.execute(&json!({"action":"cancel","sceneHandle":handle,"generation":generation,"requestId":2}),&cancel).unwrap();
        assert!(service.execute(&json!({"action":"hitTest","sceneHandle":handle,"generation":generation,"requestId":2,"x":0,"y":1,"z":0}),&cancel).is_err());
        let packed_pin = service.scenes.lock().unwrap()[handle].root.clone();
        service
            .execute(
                &json!({"action":"release","sceneHandle":handle,"generation":generation}),
                &cancel,
            )
            .unwrap();
        assert!(service.scenes.lock().unwrap().is_empty());
        let reopened = service
            .execute(&json!({"action":"openScene","sourcePath":path}), &cancel)
            .unwrap();
        let new_handle = reopened["sceneHandle"].as_str().unwrap();
        assert_ne!(new_handle, handle);
        assert!(Arc::ptr_eq(
            &packed_pin,
            &service.scenes.lock().unwrap()[new_handle].root
        ));
        assert_eq!(reopened["sourceIdentity"], opened["sourceIdentity"]);
    }
}
