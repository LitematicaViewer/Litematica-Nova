//! Shared lossless interchange for format adapters and composition. Geometry is
//! always the existing LitematicRoot, never a second renderer representation.
use crate::{
    model::BlockStateNbt,
    nbt::{self, LitematicRoot},
};
use anyhow::{Result, bail, ensure};
use fastnbt::Value;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneBounds {
    pub min: [i32; 3],
    pub max: [i32; 3],
}
impl SceneBounds {
    pub fn validate(self) -> Result<Self> {
        ensure!(
            (0..3).all(|a| self.min[a] <= self.max[a]),
            "范围最小值不得大于最大值"
        );
        Ok(self)
    }
    pub fn contains(self, p: [i32; 3]) -> bool {
        (0..3).all(|a| p[a] >= self.min[a] && p[a] <= self.max[a])
    }
    pub fn intersects(self, b: Self) -> bool {
        (0..3).all(|a| self.min[a] <= b.max[a] && b.min[a] <= self.max[a])
    }
    pub fn size(self) -> Result<[i32; 3]> {
        let mut s = [0; 3];
        for a in 0..3 {
            s[a] = i32::try_from(i64::from(self.max[a]) - i64::from(self.min[a]) + 1)?;
        }
        Ok(s)
    }
    pub fn union(self, b: Self) -> Self {
        Self {
            min: std::array::from_fn(|a| self.min[a].min(b.min[a])),
            max: std::array::from_fn(|a| self.max[a].max(b.max[a])),
        }
    }
}
#[derive(Clone, Debug)]
pub struct SceneCell {
    pub region: String,
    pub palette_index: usize,
    pub position: [i32; 3],
}
#[derive(Debug, Default)]
pub struct SectionCell {
    pub coordinate: [i32; 3],
    pub blocks: BTreeMap<u16, Vec<SceneCell>>,
}
#[derive(Debug, Default)]
pub struct SceneIndex {
    pub sections: BTreeMap<[i32; 3], SectionCell>,
    pub bounds: Option<SceneBounds>,
    pub non_air_blocks: u64,
}
#[derive(Debug)]
pub struct SceneDocument {
    pub root: Arc<LitematicRoot>,
    /// Original NBT preserves unknown root/metadata/region fields and typed arrays.
    pub raw: Value,
    pub source_format: String,
    pub content_id: String,
    index: OnceLock<Arc<SceneIndex>>,
}
pub fn from_litematic_root(root: LitematicRoot, raw: Option<Value>) -> Result<SceneDocument> {
    let raw = match raw {
        Some(v) => v,
        None => fastnbt::from_bytes(&fastnbt::to_bytes(&root)?)?,
    };
    let content_id = content_identity(&raw);
    Ok(SceneDocument {
        root: Arc::new(root),
        raw,
        source_format: "litematic".into(),
        content_id,
        index: OnceLock::new(),
    })
}
pub fn from_raw_litematic(raw: Value, source_format: &str) -> Result<SceneDocument> {
    let root = fastnbt::from_bytes(&fastnbt::to_bytes(&raw)?)?;
    let mut document = from_litematic_root(root, Some(raw))?;
    document.source_format = source_format.into();
    Ok(document)
}
impl SceneDocument {
    pub fn raw_litematic(&self) -> Result<Value> {
        Ok(self.raw.clone())
    }
    pub fn data_version(&self) -> Option<i32> {
        self.root.minecraft_data_version
    }
    pub fn presence_mask(&self, name: &str) -> Result<Option<&[i8]>> {
        let region = self
            .root
            .regions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("源选区不存在"))?;
        match self.region_raw(name) {
            Some(Value::Compound(fields)) => match fields.get("NovaPresenceMask") {
                None => Ok(None),
                Some(Value::ByteArray(mask)) => {
                    ensure!(
                        mask.len() == nbt::region_volume(&region.size)?.div_ceil(8),
                        "选区{name}的缺格mask长度错误"
                    );
                    Ok(Some(&mask[..]))
                }
                Some(_) => bail!("选区{name}的缺格mask不是ByteArray"),
            },
            _ => Ok(None),
        }
    }
    pub fn index(&self, cancel: &AtomicBool) -> Result<Arc<SceneIndex>> {
        if let Some(index) = self.index.get() {
            return Ok(index.clone());
        }
        let mut index = SceneIndex::default();
        for (name, region) in &self.root.regions {
            ensure!(!cancel.load(Ordering::Relaxed), "组合操作已取消");
            let b = nbt::region_bounds(region);
            let bounds = SceneBounds {
                min: [b.min_x, b.min_y, b.min_z],
                max: [b.max_x, b.max_y, b.max_z],
            };
            index.bounds = Some(index.bounds.map_or(bounds, |p| p.union(bounds)));
            let volume = nbt::region_volume(&region.size)?;
            let presence = self.presence_mask(name)?;
            nbt::for_each_palette_index(
                &region.block_states,
                volume,
                nbt::bits_for_palette(region.block_state_palette.len()),
                region.block_state_palette.len(),
                |i, palette_index| {
                    if i & 0x3fff == 0 {
                        ensure!(!cancel.load(Ordering::Relaxed), "组合操作已取消")
                    }
                    if !mask_contains(presence, i) {
                        return Ok(());
                    }
                    let state = region
                        .block_state_palette
                        .get(palette_index)
                        .ok_or_else(|| anyhow::anyhow!("损坏的方块调色板"))?;
                    if is_air(state) {
                        return Ok(());
                    }

                    let width = i64::from(region.size.x).unsigned_abs() as usize;
                    let length = i64::from(region.size.z).unsigned_abs() as usize;
                    let (x, y, z) = nbt::storage_to_region_coords(
                        i % width,
                        i / (width * length),
                        (i / width) % length,
                        &region.size,
                    );
                    let position = [
                        region.position.x.checked_add(x),
                        region.position.y.checked_add(y),
                        region.position.z.checked_add(z),
                    ];
                    let position = position
                        .into_iter()
                        .map(|v| v.ok_or_else(|| anyhow::anyhow!("源选区坐标溢出")))
                        .collect::<Result<Vec<_>>>()?;
                    let p = [position[0], position[1], position[2]];
                    let coordinate = p.map(|v| v.div_euclid(16));
                    let local = (p[0].rem_euclid(16)
                        + 16 * (p[2].rem_euclid(16) + 16 * p[1].rem_euclid(16)))
                        as u16;
                    index
                        .sections
                        .entry(coordinate)
                        .or_insert_with(|| SectionCell {
                            coordinate,
                            ..Default::default()
                        })
                        .blocks
                        .entry(local)
                        .or_default()
                        .push(SceneCell {
                            region: name.clone(),
                            palette_index,
                            position: p,
                        });
                    index.non_air_blocks += 1;
                    Ok(())
                },
            )?;
        }
        let index = Arc::new(index);
        let _ = self.index.set(index.clone());
        Ok(self.index.get().cloned().unwrap_or(index))
    }
    pub fn state(&self, cell: &SceneCell) -> Result<&BlockStateNbt> {
        self.root
            .regions
            .get(&cell.region)
            .and_then(|r| r.block_state_palette.get(cell.palette_index))
            .ok_or_else(|| anyhow::anyhow!("源区段身份不再有效"))
    }
    pub fn region_raw(&self, name: &str) -> Option<&Value> {
        match &self.raw {
            Value::Compound(root) => match root.get("Regions") {
                Some(Value::Compound(regions)) => regions.get(name),
                _ => None,
            },
            _ => None,
        }
    }
}
pub fn is_air(state: &BlockStateNbt) -> bool {
    matches!(
        state.name.as_str(),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}
/// NBT compounds are unordered maps: hashing their serializer iteration order
/// would invalidate an otherwise identical persisted snapshot after restart.
pub fn content_identity(value: &Value) -> String {
    fn hash(value: &Value, h: &mut Sha256) {
        match value {
            Value::Byte(v) => {
                h.update([1]);
                h.update(v.to_be_bytes())
            }
            Value::Short(v) => {
                h.update([2]);
                h.update(v.to_be_bytes())
            }
            Value::Int(v) => {
                h.update([3]);
                h.update(v.to_be_bytes())
            }
            Value::Long(v) => {
                h.update([4]);
                h.update(v.to_be_bytes())
            }
            Value::Float(v) => {
                h.update([5]);
                h.update(v.to_bits().to_be_bytes())
            }
            Value::Double(v) => {
                h.update([6]);
                h.update(v.to_bits().to_be_bytes())
            }
            Value::ByteArray(v) => {
                h.update([7]);
                h.update((v.len() as u64).to_be_bytes());
                for n in v.iter() {
                    h.update(n.to_be_bytes())
                }
            }
            Value::String(v) => {
                h.update([8]);
                h.update((v.len() as u64).to_be_bytes());
                h.update(v.as_bytes())
            }
            Value::List(v) => {
                h.update([9]);
                h.update((v.len() as u64).to_be_bytes());
                for n in v {
                    hash(n, h)
                }
            }
            Value::Compound(v) => {
                h.update([10]);
                h.update((v.len() as u64).to_be_bytes());
                for (k, v) in v.iter().collect::<BTreeMap<_, _>>() {
                    h.update((k.len() as u64).to_be_bytes());
                    h.update(k.as_bytes());
                    hash(v, h)
                }
            }
            Value::IntArray(v) => {
                h.update([11]);
                h.update((v.len() as u64).to_be_bytes());
                for n in v.iter() {
                    h.update(n.to_be_bytes())
                }
            }
            Value::LongArray(v) => {
                h.update([12]);
                h.update((v.len() as u64).to_be_bytes());
                for n in v.iter() {
                    h.update(n.to_be_bytes())
                }
            }
        }
    }
    let mut hash_state = Sha256::new();
    hash(value, &mut hash_state);
    format!("{:x}", hash_state.finalize())
}
pub fn mask_contains(mask: Option<&[i8]>, index: usize) -> bool {
    mask.is_none_or(|bytes| {
        bytes
            .get(index / 8)
            .is_some_and(|byte| (*byte as u8) & (1 << (index & 7)) != 0)
    })
}
pub fn cell_is_present(scene: &SceneDocument, region: &str, index: usize) -> Result<bool> {
    let r = scene
        .root
        .regions
        .get(region)
        .ok_or_else(|| anyhow::anyhow!("源选区不存在"))?;
    ensure!(index < nbt::region_volume(&r.size)?, "缺格查询超出选区范围");
    Ok(mask_contains(scene.presence_mask(region)?, index))
}
pub fn root_compound(value: &Value) -> Result<&std::collections::HashMap<String, Value>> {
    if let Value::Compound(v) = value {
        Ok(v)
    } else {
        bail!("场景NBT根节点不是Compound")
    }
}

/// One authoritative single-region conversion for format exporters. Only runs
/// for an actual export; NovaPresenceMask preserves absent cells separately
/// from explicit air and real user-placed structure_void blocks.
pub fn flatten(scene: &SceneDocument, cancel: &AtomicBool) -> Result<SceneDocument> {
    use crate::{
        composition_transform::Transform,
        model::EnclosingSize,
        nbt::{MetadataNbt, RegionNbt, Vec3i},
    };
    use std::collections::HashMap;
    let mut bounds: Option<SceneBounds> = None;
    for region in scene.root.regions.values() {
        let b = nbt::region_bounds(region);
        let b = SceneBounds {
            min: [b.min_x, b.min_y, b.min_z],
            max: [b.max_x, b.max_y, b.max_z],
        };
        bounds = Some(bounds.map_or(b, |p| p.union(b)));
    }
    let bounds = bounds.ok_or_else(|| anyhow::anyhow!("不能导出空场景"))?;
    let size = bounds.size()?;
    let volume = usize::try_from(i64::from(size[0]))?
        .checked_mul(usize::try_from(size[1])?)
        .and_then(|v| v.checked_mul(usize::try_from(size[2]).ok()?))
        .ok_or_else(|| anyhow::anyhow!("目标格式体积超出可表示范围"))?;
    let mut indices = Vec::new();
    indices.try_reserve_exact(volume)?;
    indices.resize(volume, 0usize);
    let mut presence = vec![0i8; volume.div_ceil(8)];
    let mut palette = vec![BlockStateNbt {
        name: "minecraft:air".into(),
        properties: BTreeMap::new(),
    }];
    let mut ids = BTreeMap::from([(serde_json::to_string(&palette[0])?, 0usize)]);
    let mut occupied: BTreeMap<usize, String> = BTreeMap::new();
    let mut entities = Vec::new();
    let mut tiles = Vec::new();
    let mut extra_ticks: HashMap<String, Vec<Value>> = HashMap::new();
    let mut blocks = 0u64;
    for (name, region) in &scene.root.regions {
        ensure!(!cancel.load(Ordering::Relaxed), "导出已取消");
        let mut mapping = Vec::new();
        for state in &region.block_state_palette {
            let key = serde_json::to_string(state)?;
            let index = if let Some(index) = ids.get(&key) {
                *index
            } else {
                let index = palette.len();
                palette.push(state.clone());
                ids.insert(key, index);
                index
            };
            mapping.push(index);
        }
        let source_presence = scene.presence_mask(name)?;
        let width = i64::from(region.size.x).unsigned_abs() as usize;
        let length = i64::from(region.size.z).unsigned_abs() as usize;
        nbt::for_each_palette_index(
            &region.block_states,
            nbt::region_volume(&region.size)?,
            nbt::bits_for_palette(region.block_state_palette.len()),
            region.block_state_palette.len(),
            |i, index| {
                if i & 0x3fff == 0 {
                    ensure!(!cancel.load(Ordering::Relaxed), "导出已取消")
                }
                let state = &region.block_state_palette[index];
                if !mask_contains(source_presence, i) {
                    return Ok(());
                }
                let (x, y, z) = nbt::storage_to_region_coords(
                    i % width,
                    i / (width * length),
                    (i / width) % length,
                    &region.size,
                );
                let p = [
                    region
                        .position
                        .x
                        .checked_add(x)
                        .ok_or_else(|| anyhow::anyhow!("区域坐标溢出"))?,
                    region
                        .position
                        .y
                        .checked_add(y)
                        .ok_or_else(|| anyhow::anyhow!("区域坐标溢出"))?,
                    region
                        .position
                        .z
                        .checked_add(z)
                        .ok_or_else(|| anyhow::anyhow!("区域坐标溢出"))?,
                ];
                let local = std::array::from_fn::<_, 3, _>(|a| {
                    (i64::from(p[a]) - i64::from(bounds.min[a])) as usize
                });
                let output = (local[1] * size[2] as usize + local[2]) * size[0] as usize + local[0];
                presence[output / 8] |= (1u8 << (output & 7)) as i8;
                if !is_air(state) {
                    ensure!(
                        !occupied.contains_key(&output),
                        "多个选区在{p:?}存在方块冲突，请先在组合中明确处理"
                    );
                    occupied.insert(output, name.clone());
                    blocks += 1;
                    indices[output] = mapping[index];
                } else if !occupied.contains_key(&output) {
                    indices[output] = mapping[index];
                }
                Ok(())
            },
        )?;
        let from = [region.position.x, region.position.y, region.position.z];
        for entity in &region.entities {
            let mut value: Value = fastnbt::from_bytes(&fastnbt::to_bytes(entity)?)?;
            Transform::default().entity(&mut value, from, bounds.min)?;
            entities.push(fastnbt::from_bytes(&fastnbt::to_bytes(&value)?)?);
        }
        for tile in &region.tile_entities {
            let mut value = tile.clone();
            Transform::default().block_entity(&mut value, from, bounds.min)?;
            tiles.push(value);
        }
        if let Some(Value::Compound(raw)) = scene.region_raw(name) {
            for (key, value) in raw {
                if matches!(
                    key.as_str(),
                    "PendingBlockTicks" | "PendingFluidTicks" | "NovaAnvilScheduledTicks"
                ) {
                    let Value::List(list) = value else {
                        bail!("计划刻字段无效")
                    };
                    for value in list {
                        let mut value = value.clone();
                        Transform::default().block_entity(&mut value, from, bounds.min)?;
                        extra_ticks.entry(key.clone()).or_default().push(value);
                    }
                } else if !matches!(
                    key.as_str(),
                    "Position"
                        | "Size"
                        | "BlockStatePalette"
                        | "BlockStates"
                        | "Entities"
                        | "TileEntities"
                        | "BlockEntities"
                        | "NovaPresenceMask"
                ) {
                    bail!("选区{name}的{key}尚未定义单区域导出语义")
                }
            }
        }
    }
    let root = LitematicRoot {
        metadata: MetadataNbt {
            author: scene.root.metadata.author.clone(),
            description: scene.root.metadata.description.clone(),
            name: scene.root.metadata.name.clone(),
            time_created: scene.root.metadata.time_created,
            time_modified: scene.root.metadata.time_modified,
            total_blocks: Some(i32::try_from(blocks)?),
            total_volume: Some(i32::try_from(volume)?),
            region_count: Some(1),
            enclosing_size: Some(EnclosingSize {
                x: size[0],
                y: size[1],
                z: size[2],
            }),
            preview_image_data: None,
        },
        regions: BTreeMap::from([(
            "merged".into(),
            RegionNbt {
                position: Vec3i {
                    x: bounds.min[0],
                    y: bounds.min[1],
                    z: bounds.min[2],
                },
                size: Vec3i {
                    x: size[0],
                    y: size[1],
                    z: size[2],
                },
                block_state_palette: palette.clone(),
                block_states: nbt::pack_palette_indices(
                    &indices,
                    nbt::bits_for_palette(palette.len()),
                ),
                entities,
                tile_entities: tiles,
            },
        )]),
        version: scene.root.version,
        sub_version: scene.root.sub_version,
        minecraft_data_version: scene.root.minecraft_data_version,
        preview_image_data: None,
    };
    let mut raw: Value = fastnbt::from_bytes(&fastnbt::to_bytes(&root)?)?;
    if let Value::Compound(output) = &mut raw {
        if let Value::Compound(original) = &scene.raw {
            for (key, value) in original {
                if !matches!(
                    key.as_str(),
                    "Regions"
                        | "Metadata"
                        | "Version"
                        | "SubVersion"
                        | "MinecraftDataVersion"
                        | "PreviewImageData"
                ) {
                    output.insert(key.clone(), value.clone());
                }
            }
        }
        if let Some(Value::Compound(regions)) = output.get_mut("Regions") {
            if let Some(Value::Compound(region)) = regions.get_mut("merged") {
                region.insert(
                    "NovaPresenceMask".into(),
                    Value::ByteArray(fastnbt::ByteArray::new(presence)),
                );
                for (key, values) in extra_ticks {
                    region.insert(key, Value::List(values));
                }
            }
        }
    }
    from_raw_litematic(raw, &scene.source_format)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn negative_sections_and_touching_bounds_are_exact() {
        let b = SceneBounds {
            min: [-17, 0, -1],
            max: [-1, 2, 5],
        };
        assert!(b.contains([-16, 0, 0]));
        assert_eq!(b.size().unwrap(), [17, 3, 7]);
        assert!(!b.intersects(SceneBounds {
            min: [0, 0, 0],
            max: [2, 2, 2]
        }));
    }
    #[test]
    fn canonical_hash_does_not_depend_on_map_insertion_order() {
        use std::collections::HashMap;
        let first = Value::Compound(HashMap::from([
            ("x".into(), Value::Int(1)),
            ("y".into(), Value::Long(2)),
        ]));
        let second = Value::Compound(HashMap::from([
            ("y".into(), Value::Long(2)),
            ("x".into(), Value::Int(1)),
        ]));
        assert_eq!(content_identity(&first), content_identity(&second));
        assert_ne!(
            content_identity(&first),
            content_identity(&Value::Compound(HashMap::from([
                ("x".into(), Value::Long(1)),
                ("y".into(), Value::Long(2))
            ])))
        );
    }
}
