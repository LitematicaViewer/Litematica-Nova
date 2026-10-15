//! Version-independent semantic input for materials and schematic tooling.
//!
//! Minecraft `DataVersion` is consumed at this boundary.  Downstream scene,
//! cache and render code reads canonical identifiers and interned block states;
//! it must not reinterpret the source version.  The large packed `BlockStates`
//! arrays are moved into this representation unchanged so normalization has a
//! bounded working set with respect to schematic volume.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use fastnbt::Value;

use crate::model::{BlockStateNbt, EntityNbt};
use crate::nbt::{BigEndianLongArray, LitematicRoot, MetadataNbt, RegionNbt, Vec3i};

/// Version of the viewer-owned semantic IR.  This is deliberately unrelated
/// to Mojang's `DataVersion` and is the value cache schemas must key on.
pub const CANONICAL_IR_SCHEMA_VERSION: u32 = 1;

/// Revision of native input normalization rules within the current IR schema.
pub const NATIVE_ADAPTER_REVISION: u32 = 2;

const FIRST_FLATTENED_DATA_VERSION: i32 = 1_519;
const FIRST_VERIFIED_MODERN_DATA_VERSION: i32 = 3_463;
const FIRST_COMPONENT_ERA_DATA_VERSION: i32 = 3_953;
const IRON_CHAIN_RENAME_DATA_VERSION: i32 = 4_903;
const LAST_SUPPORTED_DATA_VERSION: i32 = 5_023;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CanonicalAdapterKind {
    Modern,
    ComponentEra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalProvenance {
    source_minecraft_data_version: i32,
    source_litematic_version: i32,
    source_litematic_sub_version: Option<i32>,
    adapter: CanonicalAdapterKind,
    adapter_revision: u32,
}

impl CanonicalProvenance {
    pub const fn source_minecraft_data_version(self) -> i32 {
        self.source_minecraft_data_version
    }

    pub const fn source_litematic_version(self) -> i32 {
        self.source_litematic_version
    }

    pub const fn source_litematic_sub_version(self) -> Option<i32> {
        self.source_litematic_sub_version
    }

    pub const fn adapter(self) -> CanonicalAdapterKind {
        self.adapter
    }

    pub const fn adapter_revision(self) -> u32 {
        self.adapter_revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalIdentifier(Box<str>);

impl CanonicalIdentifier {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalBlockStateId(u32);

impl CanonicalBlockStateId {
    pub const fn value(self) -> u32 {
        self.0
    }

    pub(crate) const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalFluidKind {
    Water,
    Lava,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalFluidState {
    None,
    Waterlogged,
    Fluid {
        kind: CanonicalFluidKind,
        level: u8,
        falling: bool,
    },
}

/// One globally interned canonical block state.  `renderer_state` is a stable
/// compatibility view for existing scene builders; it already contains the
/// normalized identifier and properties and carries no source-version tag.
#[derive(Debug, Clone)]
pub struct CanonicalBlockState {
    identifier: CanonicalIdentifier,
    properties: BTreeMap<String, String>,
    fluid: CanonicalFluidState,
    renderer_state: BlockStateNbt,
}

impl CanonicalBlockState {
    pub fn identifier(&self) -> &CanonicalIdentifier {
        &self.identifier
    }

    pub fn properties(&self) -> &BTreeMap<String, String> {
        &self.properties
    }

    pub const fn fluid(&self) -> CanonicalFluidState {
        self.fluid
    }

    pub(crate) fn renderer_state(&self) -> &BlockStateNbt {
        &self.renderer_state
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalPosePart {
    name: Box<str>,
    rotation_degrees: [f32; 3],
}

impl CanonicalPosePart {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn rotation_degrees(&self) -> [f32; 3] {
        self.rotation_degrees
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalUnknownFields {
    field_names: Box<[Box<str>]>,
}

/// Legacy item metadata that still affects rendering for modern schematics.
/// Pre-flattening block numeric IDs remain an explicit unsupported adapter
/// boundary, while item damage/tag semantics can be carried without keeping a
/// second copy of the full payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalLegacyMetadata {
    damage: Option<i32>,
    tag_present: bool,
}

impl CanonicalLegacyMetadata {
    pub const fn damage(self) -> Option<i32> {
        self.damage
    }

    pub const fn tag_present(self) -> bool {
        self.tag_present
    }
}

impl CanonicalUnknownFields {
    pub fn field_names(&self) -> &[Box<str>] {
        &self.field_names
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalItemStack {
    identifier: CanonicalIdentifier,
    count: i32,
    slot: Option<i32>,
    component_keys: Box<[Box<str>]>,
    legacy: CanonicalLegacyMetadata,
    unknown: CanonicalUnknownFields,
}

impl CanonicalItemStack {
    pub fn identifier(&self) -> &CanonicalIdentifier {
        &self.identifier
    }

    pub const fn count(&self) -> i32 {
        self.count
    }

    pub const fn slot(&self) -> Option<i32> {
        self.slot
    }

    pub fn component_keys(&self) -> &[Box<str>] {
        &self.component_keys
    }

    pub const fn has_legacy_tag(&self) -> bool {
        self.legacy.tag_present
    }

    pub const fn legacy_metadata(&self) -> CanonicalLegacyMetadata {
        self.legacy
    }

    pub fn unknown(&self) -> &CanonicalUnknownFields {
        &self.unknown
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalEntity {
    identifier: Option<CanonicalIdentifier>,
    position: Option<[f64; 3]>,
    orientation_degrees: Option<[f32; 2]>,
    pose: Box<[CanonicalPosePart]>,
    passengers: Box<[CanonicalEntity]>,
    inventory: Box<[CanonicalItemStack]>,
    equipment: Box<[CanonicalItemStack]>,
    unknown: CanonicalUnknownFields,
    payload_index: usize,
}

impl CanonicalEntity {
    pub fn identifier(&self) -> Option<&CanonicalIdentifier> {
        self.identifier.as_ref()
    }

    pub const fn position(&self) -> Option<[f64; 3]> {
        self.position
    }

    pub const fn orientation_degrees(&self) -> Option<[f32; 2]> {
        self.orientation_degrees
    }

    pub fn pose(&self) -> &[CanonicalPosePart] {
        &self.pose
    }

    pub fn passengers(&self) -> &[CanonicalEntity] {
        &self.passengers
    }

    pub fn inventory(&self) -> &[CanonicalItemStack] {
        &self.inventory
    }

    pub fn equipment(&self) -> &[CanonicalItemStack] {
        &self.equipment
    }

    pub fn unknown(&self) -> &CanonicalUnknownFields {
        &self.unknown
    }

    pub const fn payload_index(&self) -> usize {
        self.payload_index
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBlockEntity {
    identifier: Option<CanonicalIdentifier>,
    position: Option<[i32; 3]>,
    inventory: Box<[CanonicalItemStack]>,
    unknown: CanonicalUnknownFields,
    payload_index: usize,
}

impl CanonicalBlockEntity {
    pub fn identifier(&self) -> Option<&CanonicalIdentifier> {
        self.identifier.as_ref()
    }

    pub const fn position(&self) -> Option<[i32; 3]> {
        self.position
    }

    pub fn inventory(&self) -> &[CanonicalItemStack] {
        &self.inventory
    }

    pub fn unknown(&self) -> &CanonicalUnknownFields {
        &self.unknown
    }

    pub const fn payload_index(&self) -> usize {
        self.payload_index
    }
}

#[derive(Debug)]
pub struct CanonicalRegion {
    name: Box<str>,
    position: Vec3i,
    size: Vec3i,
    palette: Box<[CanonicalBlockStateId]>,
    /// Source-version palette semantics for business analysis. This small
    /// sidecar retains each original ID/properties even when distinct source
    /// entries intern to the same renderer state; packed block data stays single.
    source_palette: Box<[BlockStateNbt]>,
    block_states: BigEndianLongArray,
    entities: Box<[CanonicalEntity]>,
    block_entities: Box<[CanonicalBlockEntity]>,
    // Full unknown/fallback payload is retained once, not copied into the
    // semantic sidecars above. Existing renderers consume this canonicalized
    // compatibility view while they migrate to typed fields.
    renderer_entities: Vec<EntityNbt>,
    renderer_block_entities: Vec<Value>,
}

impl CanonicalRegion {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn position(&self) -> Vec3i {
        self.position
    }

    pub const fn size(&self) -> Vec3i {
        self.size
    }

    pub fn palette(&self) -> &[CanonicalBlockStateId] {
        &self.palette
    }

    pub(crate) fn source_palette(&self) -> &[BlockStateNbt] {
        &self.source_palette
    }

    pub fn entities(&self) -> &[CanonicalEntity] {
        &self.entities
    }

    pub fn block_entities(&self) -> &[CanonicalBlockEntity] {
        &self.block_entities
    }

    pub(crate) fn block_states(&self) -> &BigEndianLongArray {
        &self.block_states
    }

    pub(crate) fn renderer_entities(&self) -> &[EntityNbt] {
        &self.renderer_entities
    }

    pub(crate) fn renderer_block_entities(&self) -> &[Value] {
        &self.renderer_block_entities
    }
}

#[derive(Debug)]
pub struct CanonicalMetadata {
    pub author: Option<String>,
    pub description: Option<String>,
    pub name: Option<String>,
    pub time_created: Option<i64>,
    pub time_modified: Option<i64>,
    pub total_blocks: Option<i32>,
    pub total_volume: Option<i32>,
    pub region_count: Option<i32>,
    pub enclosing_size: Option<crate::model::EnclosingSize>,
}

#[derive(Debug)]
pub struct CanonicalWorld {
    schema_version: u32,
    provenance: CanonicalProvenance,
    metadata: CanonicalMetadata,
    states: Box<[CanonicalBlockState]>,
    regions: Box<[CanonicalRegion]>,
}

impl CanonicalWorld {
    /// Directly adapts a parsed litematic into viewer semantics. No Java,
    /// Minecraft client or sequential per-version upgrade chain participates.
    pub fn normalize(root: LitematicRoot) -> Result<Self, CanonicalIrError> {
        let source_data_version = root
            .minecraft_data_version
            .ok_or(CanonicalIrError::MissingDataVersion)?;
        let adapter = NativeVersionAdapter::select(source_data_version)?;
        let provenance = CanonicalProvenance {
            source_minecraft_data_version: source_data_version,
            source_litematic_version: root.version,
            source_litematic_sub_version: root.sub_version,
            adapter: adapter.kind,
            adapter_revision: NATIVE_ADAPTER_REVISION,
        };
        let LitematicRoot {
            metadata,
            regions,
            version: _,
            sub_version: _,
            minecraft_data_version: _,
            preview_image_data: _,
        } = root;

        let mut state_ids = BTreeMap::<BlockStateKey, CanonicalBlockStateId>::new();
        let mut states = Vec::<CanonicalBlockState>::new();
        let mut canonical_regions = Vec::new();
        canonical_regions
            .try_reserve_exact(regions.len())
            .map_err(|source| CanonicalIrError::Allocation {
                context: "region table",
                detail: source.to_string(),
            })?;

        for (name, region) in regions {
            canonical_regions.push(adapter.normalize_region(
                name,
                region,
                &mut state_ids,
                &mut states,
            )?);
        }

        Ok(Self {
            schema_version: CANONICAL_IR_SCHEMA_VERSION,
            provenance,
            metadata: canonical_metadata(metadata),
            states: states.into_boxed_slice(),
            regions: canonical_regions.into_boxed_slice(),
        })
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn provenance(&self) -> CanonicalProvenance {
        self.provenance
    }

    pub const fn source_minecraft_data_version(&self) -> i32 {
        self.provenance.source_minecraft_data_version
    }

    pub fn metadata(&self) -> &CanonicalMetadata {
        &self.metadata
    }

    pub fn states(&self) -> &[CanonicalBlockState] {
        &self.states
    }

    pub fn regions(&self) -> &[CanonicalRegion] {
        &self.regions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalIrError {
    MissingDataVersion,
    InvalidDataVersion {
        found: i32,
    },
    PreFlatteningUnsupported {
        found: i32,
        first_flattened: i32,
    },
    UnverifiedModernDataVersion {
        found: i32,
        first_supported: i32,
    },
    NewerDataVersion {
        found: i32,
        last_supported: i32,
    },
    EmptyIdentifier {
        context: Box<str>,
    },
    TooManyBlockStates {
        count: usize,
    },
    PassengerDepth {
        region: Box<str>,
        record_index: usize,
        depth: usize,
    },
    Allocation {
        context: &'static str,
        detail: String,
    },
}

impl fmt::Display for CanonicalIrError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDataVersion => formatter.write_str(
                "litematic has no MinecraftDataVersion; native adapter selection is impossible",
            ),
            Self::InvalidDataVersion { found } => write!(
                formatter,
                "litematic MinecraftDataVersion {found} is not a positive version"
            ),
            Self::PreFlatteningUnsupported {
                found,
                first_flattened,
            } => write!(
                formatter,
                "MinecraftDataVersion {found} uses pre-flattening block metadata; native legacy-id/metadata semantics are not implemented (first flattened DataVersion {first_flattened})"
            ),
            Self::UnverifiedModernDataVersion {
                found,
                first_supported,
            } => write!(
                formatter,
                "MinecraftDataVersion {found} is flattened but outside the native adapter corpus; first currently supported modern DataVersion is {first_supported}"
            ),
            Self::NewerDataVersion {
                found,
                last_supported,
            } => write!(
                formatter,
                "MinecraftDataVersion {found} is newer than the native adapter's last supported version {last_supported}"
            ),
            Self::EmptyIdentifier { context } => {
                write!(
                    formatter,
                    "{context} contains an empty namespaced identifier"
                )
            }
            Self::TooManyBlockStates { count } => write!(
                formatter,
                "canonical block-state table has {count} entries and exceeds u32 ids"
            ),
            Self::PassengerDepth {
                region,
                record_index,
                depth,
            } => write!(
                formatter,
                "region {region:?} entity {record_index} passenger hierarchy exceeds NBT safety depth {depth}"
            ),
            Self::Allocation { context, detail } => {
                write!(formatter, "cannot allocate canonical {context}: {detail}")
            }
        }
    }
}

impl Error for CanonicalIrError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BlockStateKey {
    identifier: Box<str>,
    properties: Box<[(Box<str>, Box<str>)]>,
}

#[derive(Debug, Clone, Copy)]
struct NativeVersionAdapter {
    kind: CanonicalAdapterKind,
    source_data_version: i32,
}

impl NativeVersionAdapter {
    fn select(data_version: i32) -> Result<Self, CanonicalIrError> {
        if data_version <= 0 {
            return Err(CanonicalIrError::InvalidDataVersion {
                found: data_version,
            });
        }
        if data_version < FIRST_FLATTENED_DATA_VERSION {
            return Err(CanonicalIrError::PreFlatteningUnsupported {
                found: data_version,
                first_flattened: FIRST_FLATTENED_DATA_VERSION,
            });
        }
        if data_version < FIRST_VERIFIED_MODERN_DATA_VERSION {
            return Err(CanonicalIrError::UnverifiedModernDataVersion {
                found: data_version,
                first_supported: FIRST_VERIFIED_MODERN_DATA_VERSION,
            });
        }
        if data_version > LAST_SUPPORTED_DATA_VERSION {
            return Err(CanonicalIrError::NewerDataVersion {
                found: data_version,
                last_supported: LAST_SUPPORTED_DATA_VERSION,
            });
        }
        Ok(Self {
            kind: if data_version < FIRST_COMPONENT_ERA_DATA_VERSION {
                CanonicalAdapterKind::Modern
            } else {
                CanonicalAdapterKind::ComponentEra
            },
            source_data_version: data_version,
        })
    }

    fn normalize_region(
        self,
        name: String,
        region: RegionNbt,
        state_ids: &mut BTreeMap<BlockStateKey, CanonicalBlockStateId>,
        states: &mut Vec<CanonicalBlockState>,
    ) -> Result<CanonicalRegion, CanonicalIrError> {
        let RegionNbt {
            position,
            size,
            block_state_palette,
            block_states,
            mut entities,
            mut tile_entities,
        } = region;

        let mut palette = Vec::new();
        palette
            .try_reserve_exact(block_state_palette.len())
            .map_err(|source| CanonicalIrError::Allocation {
                context: "region palette",
                detail: source.to_string(),
            })?;
        let mut source_palette = Vec::with_capacity(block_state_palette.len());
        for state in block_state_palette {
            source_palette.push(state.clone());
            let state = normalize_block_state(state, self.source_data_version)?;
            let key = BlockStateKey {
                identifier: state.identifier.0.clone(),
                properties: state
                    .properties
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone().into_boxed_str(),
                            value.clone().into_boxed_str(),
                        )
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            };
            let id = if let Some(id) = state_ids.get(&key) {
                *id
            } else {
                let value = u32::try_from(states.len()).map_err(|_| {
                    CanonicalIrError::TooManyBlockStates {
                        count: states.len().saturating_add(1),
                    }
                })?;
                let id = CanonicalBlockStateId(value);
                state_ids.insert(key, id);
                states.push(state);
                id
            };
            palette.push(id);
        }

        for entity in &mut entities {
            normalize_entity_payload(entity, self.source_data_version)?;
        }
        for block_entity in &mut tile_entities {
            normalize_block_entity_payload(block_entity, self.source_data_version)?;
        }

        let mut canonical_entities = Vec::new();
        canonical_entities
            .try_reserve_exact(entities.len())
            .map_err(|source| CanonicalIrError::Allocation {
                context: "entity semantic table",
                detail: source.to_string(),
            })?;
        for (record_index, entity) in entities.iter().enumerate() {
            canonical_entities.push(summarize_entity(
                entity,
                record_index,
                &name,
                record_index,
                0,
            )?);
        }

        let canonical_block_entities = tile_entities
            .iter()
            .enumerate()
            .map(|(payload_index, value)| summarize_block_entity(value, payload_index))
            .collect::<Result<Vec<_>, _>>()?;

        let _ = self.kind; // Adapter-specific rewrites remain confined here.
        Ok(CanonicalRegion {
            name: name.into_boxed_str(),
            position,
            size,
            palette: palette.into_boxed_slice(),
            source_palette: source_palette.into_boxed_slice(),
            block_states,
            entities: canonical_entities.into_boxed_slice(),
            block_entities: canonical_block_entities.into_boxed_slice(),
            renderer_entities: entities,
            renderer_block_entities: tile_entities,
        })
    }
}

fn canonical_metadata(metadata: MetadataNbt) -> CanonicalMetadata {
    CanonicalMetadata {
        author: metadata.author,
        description: metadata.description,
        name: metadata.name,
        time_created: metadata.time_created,
        time_modified: metadata.time_modified,
        total_blocks: metadata.total_blocks,
        total_volume: metadata.total_volume,
        region_count: metadata.region_count,
        enclosing_size: metadata.enclosing_size,
    }
}

fn normalize_identifier(
    value: &str,
    context: impl Into<Box<str>>,
) -> Result<CanonicalIdentifier, CanonicalIrError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CanonicalIrError::EmptyIdentifier {
            context: context.into(),
        });
    }
    let normalized = match value.split_once(':') {
        Some((namespace, path)) if !namespace.is_empty() && !path.is_empty() => {
            format!(
                "{}:{}",
                namespace.to_ascii_lowercase(),
                path.to_ascii_lowercase()
            )
        }
        Some(_) => {
            return Err(CanonicalIrError::EmptyIdentifier {
                context: context.into(),
            });
        }
        None => format!("minecraft:{}", value.to_ascii_lowercase()),
    };
    Ok(CanonicalIdentifier(normalized.into_boxed_str()))
}

fn normalize_block_state(
    state: BlockStateNbt,
    source_data_version: i32,
) -> Result<CanonicalBlockState, CanonicalIrError> {
    let mut identifier = normalize_identifier(&state.name, "block-state Name")?;
    // MC 26 renamed the vanilla chain block/item to iron_chain. This is a
    // version rule, not a corpus/file-name exception: every supported source
    // predating the rename maps the old semantic identifier once here.
    if source_data_version < IRON_CHAIN_RENAME_DATA_VERSION
        && identifier.as_str() == "minecraft:chain"
    {
        identifier = CanonicalIdentifier(Box::from("minecraft:iron_chain"));
    }
    let properties = state
        .properties
        .into_iter()
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect::<BTreeMap<_, _>>();
    let fluid = canonical_fluid_state(&identifier, &properties);
    let renderer_state = BlockStateNbt {
        name: identifier.as_str().to_owned(),
        properties: properties.clone(),
    };
    Ok(CanonicalBlockState {
        identifier,
        properties,
        fluid,
        renderer_state,
    })
}

fn canonical_fluid_state(
    identifier: &CanonicalIdentifier,
    properties: &BTreeMap<String, String>,
) -> CanonicalFluidState {
    let kind = match identifier.as_str() {
        "minecraft:water" => Some(CanonicalFluidKind::Water),
        "minecraft:lava" => Some(CanonicalFluidKind::Lava),
        _ => None,
    };
    if let Some(kind) = kind {
        let level = properties
            .get("level")
            .and_then(|level| level.parse::<u8>().ok())
            .unwrap_or(0);
        let falling = properties
            .get("falling")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
            || level >= 8;
        return CanonicalFluidState::Fluid {
            kind,
            level,
            falling,
        };
    }
    if properties
        .get("waterlogged")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        CanonicalFluidState::Waterlogged
    } else {
        CanonicalFluidState::None
    }
}

fn normalize_entity_payload(
    entity: &mut EntityNbt,
    source_data_version: i32,
) -> Result<(), CanonicalIrError> {
    if let Some(raw_id) = entity.entity_id() {
        let id = normalize_identifier(raw_id, "entity id")?;
        entity.id_lower = Some(id.as_str().to_owned());
        entity.id_upper = None;
    }
    if let Some(passengers) = entity.data.get_mut("Passengers") {
        normalize_passenger_values(passengers, source_data_version)?;
    }
    for name in [
        "Inventory",
        "Items",
        "HandItems",
        "ArmorItems",
        "equipment",
        "Item",
        "item",
    ] {
        if let Some(value) = entity.data.get_mut(name) {
            normalize_item_values(value, source_data_version)?;
        }
    }
    Ok(())
}

fn normalize_block_entity_payload(
    value: &mut Value,
    source_data_version: i32,
) -> Result<(), CanonicalIrError> {
    let Value::Compound(compound) = value else {
        return Ok(());
    };
    normalize_compound_id(compound, "block-entity id")?;
    // 26.3 DataFixers schema 4996.1 changes the BE's four-item list to named
    // ItemStackTemplate sides. Integer source version 4996 precedes 4996.1.
    if source_data_version <= 4_996
        && matches!(compound.get("id"), Some(Value::String(id)) if id == "minecraft:decorated_pot")
    {
        let decorations = compound
            .entry("sherds".to_owned())
            .or_insert_with(|| Value::List(Vec::new()));
        unflatten_pot_decorations(decorations);
    }
    for name in ["Items", "Inventory", "Item", "item"] {
        if let Some(value) = compound.get_mut(name) {
            normalize_item_values(value, source_data_version)?;
        }
    }
    Ok(())
}

fn normalize_passenger_values(
    value: &mut Value,
    source_data_version: i32,
) -> Result<(), CanonicalIrError> {
    match value {
        Value::Compound(compound) => {
            normalize_compound_id(compound, "passenger entity id")?;
            if let Some(passengers) = compound.get_mut("Passengers") {
                normalize_passenger_values(passengers, source_data_version)?;
            }
            for name in [
                "Inventory",
                "Items",
                "HandItems",
                "ArmorItems",
                "equipment",
                "Item",
                "item",
            ] {
                if let Some(value) = compound.get_mut(name) {
                    normalize_item_values(value, source_data_version)?;
                }
            }
        }
        Value::List(values) => {
            for nested in values {
                normalize_passenger_values(nested, source_data_version)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn normalize_item_values(
    value: &mut Value,
    source_data_version: i32,
) -> Result<(), CanonicalIrError> {
    match value {
        Value::Compound(compound) => {
            if let Some(Value::String(id)) = compound.get_mut("id") {
                let normalized = normalize_item_identifier(id, source_data_version)?;
                *id = normalized.as_str().to_owned();
                // The item component was migrated at schema 4996 (before the
                // corresponding block-entity fix at 4996.1).
                if source_data_version < 4_996
                    && let Some(Value::Compound(components)) = compound.get_mut("components")
                    && let Some(decorations) = components.get_mut("minecraft:pot_decorations")
                {
                    unflatten_pot_decorations(decorations);
                }
            } else {
                // Equipment maps are containers rather than item stacks.
                for nested in compound.values_mut() {
                    normalize_item_values(nested, source_data_version)?;
                }
            }
        }
        Value::List(values) => {
            for nested in values {
                normalize_item_values(nested, source_data_version)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn unflatten_pot_decorations(value: &mut Value) {
    let Value::List(items) = value else {
        return;
    };
    // PotDecorationsComponentUnflatteningFix.unpackList: absent/non-string
    // entries become brick; an empty id preserves the original invalid list.
    let mut sides = std::collections::HashMap::with_capacity(4);
    for (index, side) in ["back", "left", "right", "front"].into_iter().enumerate() {
        let item = match items.get(index) {
            Some(Value::String(item)) => item.as_str(),
            _ => "minecraft:brick",
        };
        if item.is_empty() {
            return;
        }
        sides.insert(
            side.to_owned(),
            Value::Compound(std::collections::HashMap::from([(
                "id".to_owned(),
                Value::String(item.to_owned()),
            )])),
        );
    }
    *value = Value::Compound(sides);
}

fn normalize_item_identifier(
    value: &str,
    source_data_version: i32,
) -> Result<CanonicalIdentifier, CanonicalIrError> {
    let mut identifier = normalize_identifier(value, "item-stack id")?;
    let replacement = match identifier.as_str() {
        "minecraft:chain" if source_data_version < IRON_CHAIN_RENAME_DATA_VERSION => {
            Some("minecraft:iron_chain")
        }
        "minecraft:grass" | "minecraft:scute"
            if source_data_version < FIRST_COMPONENT_ERA_DATA_VERSION =>
        {
            Some(if identifier.as_str() == "minecraft:grass" {
                "minecraft:short_grass"
            } else {
                "minecraft:turtle_scute"
            })
        }
        _ => None,
    };
    if let Some(replacement) = replacement {
        identifier = CanonicalIdentifier(Box::from(replacement));
    }
    Ok(identifier)
}

fn normalize_compound_id(
    compound: &mut std::collections::HashMap<String, Value>,
    context: &'static str,
) -> Result<(), CanonicalIrError> {
    if !compound.contains_key("id")
        && matches!(compound.get("Id"), Some(Value::String(_)))
        && let Some(value) = compound.remove("Id")
    {
        compound.insert("id".to_owned(), value);
    }
    if let Some(Value::String(id)) = compound.get_mut("id") {
        let normalized = normalize_identifier(id, context)?;
        *id = normalized.as_str().to_owned();
    }
    Ok(())
}

fn summarize_entity(
    entity: &EntityNbt,
    payload_index: usize,
    region: &str,
    root_record_index: usize,
    depth: usize,
) -> Result<CanonicalEntity, CanonicalIrError> {
    if depth > 512 {
        return Err(CanonicalIrError::PassengerDepth {
            region: region.into(),
            record_index: root_record_index,
            depth,
        });
    }
    summarize_entity_fields(
        entity.entity_id(),
        &entity.data,
        payload_index,
        region,
        root_record_index,
        depth,
    )
}

fn summarize_entity_fields(
    id: Option<&str>,
    fields: &BTreeMap<String, Value>,
    payload_index: usize,
    region: &str,
    root_record_index: usize,
    depth: usize,
) -> Result<CanonicalEntity, CanonicalIrError> {
    let identifier = id
        .map(|id| normalize_identifier(id, "canonical entity id"))
        .transpose()?;
    let position = fields.get("Pos").and_then(value_vec3_f64);
    let orientation_degrees = fields.get("Rotation").and_then(value_vec2_f32).or_else(|| {
        Some([
            value_f32(fields.get("Yaw")?)?,
            value_f32(fields.get("Pitch")?)?,
        ])
    });
    let pose = fields
        .get("Pose")
        .and_then(value_compound)
        .map(|pose| {
            pose.iter()
                .filter_map(|(name, value)| {
                    value_vec3_f32(value).map(|rotation_degrees| CanonicalPosePart {
                        name: name.clone().into_boxed_str(),
                        rotation_degrees,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut passengers = Vec::new();
    if let Some(Value::List(values)) = fields.get("Passengers") {
        passengers
            .try_reserve_exact(values.len())
            .map_err(|source| CanonicalIrError::Allocation {
                context: "passenger hierarchy",
                detail: source.to_string(),
            })?;
        for value in values {
            let Some(compound) = value_compound(value) else {
                continue;
            };
            let passenger_id = compound.get("id").and_then(value_string);
            let passenger_fields = compound
                .iter()
                .filter(|(name, _)| name.as_str() != "id")
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>();
            passengers.push(summarize_entity_fields(
                passenger_id,
                &passenger_fields,
                payload_index,
                region,
                root_record_index,
                depth + 1,
            )?);
        }
    }

    let inventory = summarize_items_in_named_fields(fields, &["Inventory", "Items"])?;
    let equipment = summarize_items_in_named_fields(
        fields,
        &["HandItems", "ArmorItems", "equipment", "Item", "item"],
    )?;
    let known = BTreeSet::from([
        "Pos",
        "Rotation",
        "Yaw",
        "Pitch",
        "Pose",
        "Passengers",
        "Inventory",
        "Items",
        "HandItems",
        "ArmorItems",
        "equipment",
        "Item",
        "item",
    ]);

    Ok(CanonicalEntity {
        identifier,
        position,
        orientation_degrees,
        pose: pose.into_boxed_slice(),
        passengers: passengers.into_boxed_slice(),
        inventory: inventory.into_boxed_slice(),
        equipment: equipment.into_boxed_slice(),
        unknown: unknown_fields(fields.keys().map(String::as_str), &known),
        payload_index,
    })
}

fn summarize_block_entity(
    value: &Value,
    payload_index: usize,
) -> Result<CanonicalBlockEntity, CanonicalIrError> {
    let Some(fields) = value_compound(value) else {
        return Ok(CanonicalBlockEntity {
            identifier: None,
            position: None,
            inventory: Box::new([]),
            unknown: CanonicalUnknownFields {
                field_names: Box::new([]),
            },
            payload_index,
        });
    };
    let identifier = fields
        .get("id")
        .and_then(value_string)
        .map(|id| normalize_identifier(id, "canonical block-entity id"))
        .transpose()?;
    let position = match (
        fields.get("x").and_then(value_i32),
        fields.get("y").and_then(value_i32),
        fields.get("z").and_then(value_i32),
    ) {
        (Some(x), Some(y), Some(z)) => Some([x, y, z]),
        _ => None,
    };
    let inventory = summarize_items_in_named_compound_fields(fields, &["Items", "Inventory"])?;
    let known = BTreeSet::from(["id", "x", "y", "z", "Items", "Inventory"]);
    Ok(CanonicalBlockEntity {
        identifier,
        position,
        inventory: inventory.into_boxed_slice(),
        unknown: unknown_fields(fields.keys().map(String::as_str), &known),
        payload_index,
    })
}

fn summarize_items_in_named_fields(
    fields: &BTreeMap<String, Value>,
    names: &[&str],
) -> Result<Vec<CanonicalItemStack>, CanonicalIrError> {
    let mut result = Vec::new();
    for name in names {
        if let Some(value) = fields.get(*name) {
            summarize_items(value, &mut result)?;
        }
    }
    Ok(result)
}

fn summarize_items_in_named_compound_fields(
    fields: &std::collections::HashMap<String, Value>,
    names: &[&str],
) -> Result<Vec<CanonicalItemStack>, CanonicalIrError> {
    let mut result = Vec::new();
    for name in names {
        if let Some(value) = fields.get(*name) {
            summarize_items(value, &mut result)?;
        }
    }
    Ok(result)
}

fn summarize_items(
    value: &Value,
    result: &mut Vec<CanonicalItemStack>,
) -> Result<(), CanonicalIrError> {
    match value {
        Value::Compound(compound) => {
            if let Some(raw_id) = compound.get("id").and_then(value_string) {
                let identifier = normalize_identifier(raw_id, "item-stack id")?;
                let count = compound
                    .get("count")
                    .or_else(|| compound.get("Count"))
                    .and_then(value_i32)
                    .unwrap_or(1);
                let slot = compound.get("Slot").and_then(value_i32);
                let component_keys = compound
                    .get("components")
                    .and_then(value_compound)
                    .map(|components| {
                        components
                            .keys()
                            .map(|key| key.clone().into_boxed_str())
                            .collect::<Vec<_>>()
                            .into_boxed_slice()
                    })
                    .unwrap_or_default();
                let known = BTreeSet::from([
                    "id",
                    "count",
                    "Count",
                    "Slot",
                    "Damage",
                    "components",
                    "tag",
                ]);
                result.push(CanonicalItemStack {
                    identifier,
                    count,
                    slot,
                    component_keys,
                    legacy: CanonicalLegacyMetadata {
                        damage: compound.get("Damage").and_then(value_i32),
                        tag_present: compound.contains_key("tag"),
                    },
                    unknown: unknown_fields(compound.keys().map(String::as_str), &known),
                });
                return Ok(());
            }
            for nested in compound.values() {
                summarize_items(nested, result)?;
            }
        }
        Value::List(values) => {
            for nested in values {
                summarize_items(nested, result)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn unknown_fields<'a>(
    fields: impl Iterator<Item = &'a str>,
    known: &BTreeSet<&str>,
) -> CanonicalUnknownFields {
    CanonicalUnknownFields {
        field_names: fields
            .filter(|field| !known.contains(field))
            .map(|field| Box::<str>::from(field))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }
}

fn value_compound(value: &Value) -> Option<&std::collections::HashMap<String, Value>> {
    match value {
        Value::Compound(value) => Some(value),
        _ => None,
    }
}

fn value_string(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        _ => None,
    }
}

fn value_i32(value: &Value) -> Option<i32> {
    match value {
        Value::Byte(value) => Some(i32::from(*value)),
        Value::Short(value) => Some(i32::from(*value)),
        Value::Int(value) => Some(*value),
        Value::Long(value) => i32::try_from(*value).ok(),
        _ => None,
    }
}

fn value_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Byte(value) => Some(f64::from(*value)),
        Value::Short(value) => Some(f64::from(*value)),
        Value::Int(value) => Some(f64::from(*value)),
        Value::Long(value) => Some(*value as f64),
        Value::Float(value) => Some(f64::from(*value)),
        Value::Double(value) => Some(*value),
        _ => None,
    }
}

fn value_f32(value: &Value) -> Option<f32> {
    value_f64(value).map(|value| value as f32)
}

fn value_vec3_f64(value: &Value) -> Option<[f64; 3]> {
    let Value::List(values) = value else {
        return None;
    };
    Some([
        value_f64(values.first()?)?,
        value_f64(values.get(1)?)?,
        value_f64(values.get(2)?)?,
    ])
}

fn value_vec3_f32(value: &Value) -> Option<[f32; 3]> {
    value_vec3_f64(value).map(|values| values.map(|value| value as f32))
}

fn value_vec2_f32(value: &Value) -> Option<[f32; 2]> {
    let Value::List(values) = value else {
        return None;
    };
    Some([value_f32(values.first()?)?, value_f32(values.get(1)?)?])
}

