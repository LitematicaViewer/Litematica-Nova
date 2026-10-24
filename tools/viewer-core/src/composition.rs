//! Non-destructive composition projects. All edits, conflicts and exports are
//! owned by the persistent data worker; the UI sends intentions only.
use crate::{
    composition_scene::{self, SceneBounds, SceneDocument},
    composition_transform::{Transform, integer, number},
    model::BlockStateNbt,
    nbt,
    schematic_formats::{self, FormatId},
};
use anyhow::{Context, Result, bail, ensure};
use fastnbt::Value as Tag;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartInstance {
    pub id: String,
    pub name: String,
    pub source_path: PathBuf,
    pub snapshot_id: String,
    #[serde(default)]
    pub selection: Option<SceneBounds>,
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub transform: Transform,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub clear_air: bool,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectState {
    pub name: String,
    pub parts: Vec<PartInstance>,
    pub target_data_version: Option<i32>,
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    #[default]
    Reject,
    KeepFirst,
    KeepLast,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompositionProject {
    pub schema: u32,
    pub id: String,
    pub generation: u64,
    pub state: ProjectState,
    pub undo: Vec<ProjectState>,
    pub redo: Vec<ProjectState>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SplitBox {
    pub name: String,
    pub bounds: SceneBounds,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictReport {
    pub block_count: u64,
    pub block_entity_count: u64,
    pub entity_count: u64,
    pub section_count: usize,
    pub collision_count: u64,
    pub collisions: Vec<Collision>,
    pub issues: Vec<String>,
    pub bounds: Option<SceneBounds>,
    pub can_export: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collision {
    pub position: [i32; 3],
    pub kept_instance: String,
    pub incoming_instance: String,
}
#[derive(Clone)]
struct PlacedBlock {
    state: BlockStateNbt,
    owner: String,
    region: String,
}
#[derive(Default)]
struct BuildSection {
    cells: BTreeMap<u16, PlacedBlock>,
}
struct Composed {
    scene: SceneDocument,
    report: ConflictReport,
}
static SOURCES: OnceLock<Mutex<BTreeMap<String, Arc<SceneDocument>>>> = OnceLock::new();
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "组合操作已取消");
    Ok(())
}
fn db(root: &Path) -> Result<Connection> {
    fs::create_dir_all(root.join("db"))?;
    let conn = Connection::open(root.join("db/stockpile.sqlite"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS nova_composition_projects(id TEXT PRIMARY KEY, generation INTEGER NOT NULL, document TEXT NOT NULL, updated_at INTEGER NOT NULL);")?;
    Ok(conn)
}
fn persist(root: &Path, project: &CompositionProject, expected: Option<u64>) -> Result<()> {
    let mut conn = db(root)?;
    let tx = conn.transaction()?;
    if let Some(expected) = expected {
        let current = tx.query_row(
            "SELECT generation FROM nova_composition_projects WHERE id=?1",
            [&project.id],
            |r| r.get::<_, u64>(0),
        )?;
        ensure!(current == expected, "组合已被其他窗口更新，请重新打开");
    }
    tx.execute("INSERT INTO nova_composition_projects(id,generation,document,updated_at) VALUES(?1,?2,?3,unixepoch()) ON CONFLICT(id) DO UPDATE SET generation=excluded.generation,document=excluded.document,updated_at=excluded.updated_at",params![project.id,project.generation,serde_json::to_string(project)?])?;
    tx.commit()?;
    Ok(())
}
fn load(root: &Path, id: &str) -> Result<CompositionProject> {
    let raw: String = db(root)?
        .query_row(
            "SELECT document FROM nova_composition_projects WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .context("未找到此组合项目")?;
    let p: CompositionProject = serde_json::from_str(&raw)?;
    ensure!(p.schema == 1, "组合项目版本不受支持");
    Ok(p)
}
fn snapshot_path(root: &Path, id: &str) -> Result<PathBuf> {
    ensure!(
        id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "源快照身份无效"
    );
    Ok(root
        .join("composition/snapshots")
        .join(format!("{id}.litematic")))
}
fn save_source(root: &Path, scene: SceneDocument, cancel: &AtomicBool) -> Result<String> {
    let id = composition_scene::content_identity(&scene.raw);
    let path = snapshot_path(root, &id)?;
    fs::create_dir_all(path.parent().unwrap())?;
    if !path.exists() {
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        {
            let mut gz = GzEncoder::new(tmp.as_file_mut(), Compression::default());
            fastnbt::to_writer(&mut gz, &scene.raw_litematic()?)?;
            gz.finish()?;
        }
        cancelled(cancel)?;
        tmp.as_file().sync_all()?;
        match tmp.persist_noclobber(&path) {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.error.into()),
        }
    }
    SOURCES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(id.clone(), Arc::new(scene));
    Ok(id)
}
fn source(root: &Path, id: &str) -> Result<Arc<SceneDocument>> {
    if let Some(s) = SOURCES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(id)
        .cloned()
    {
        return Ok(s);
    }
    let raw: Tag = fastnbt::from_reader(BufReader::new(GzDecoder::new(fs::File::open(
        snapshot_path(root, id)?,
    )?)))?;
    let mut s = composition_scene::from_raw_litematic(raw, "litematic")?;
    ensure!(s.content_id == id, "源快照内容与组合记录不一致");
    s.content_id = id.into();
    let s = Arc::new(s);
    SOURCES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(id.into(), s.clone());
    Ok(s)
}
fn decode_source(path: &Path, cancel: &AtomicBool) -> Result<SceneDocument> {
    schematic_formats::decode(path, cancel)
}
fn identifier() -> String {
    format!("{:032x}", rand::random::<u128>())
}
fn choose_region(part: &PartInstance, name: &str) -> bool {
    part.regions.is_empty() || part.regions.iter().any(|r| r == name)
}
fn local_key(p: [i32; 3]) -> ([i32; 3], u16) {
    (
        p.map(|v| v.div_euclid(16)),
        (p[0].rem_euclid(16) + 16 * (p[2].rem_euclid(16) + 16 * p[1].rem_euclid(16))) as u16,
    )
}
fn key_position(section: [i32; 3], key: u16) -> [i32; 3] {
    [
        section[0] * 16 + i32::from(key % 16),
        section[1] * 16 + i32::from(key / 256),
        section[2] * 16 + i32::from((key / 16) % 16),
    ]
}
fn tag_point(tag: &Tag, origin: [i32; 3], entity: bool) -> Result<[i32; 3]> {
    let Tag::Compound(m) = tag else {
        bail!("实体NBT不是Compound")
    };
    if entity {
        let Some(Tag::List(pos)) = m.get("Pos") else {
            bail!("实体缺少Pos锚点")
        };
        ensure!(pos.len() == 3, "实体Pos无效");
        let mut p = [0; 3];
        for a in 0..3 {
            let n = number(&pos[a])?.floor();
            ensure!(
                n >= f64::from(i32::MIN) && n <= f64::from(i32::MAX),
                "实体锚点越界"
            );
            p[a] = (n as i32).checked_add(origin[a]).context("实体坐标溢出")?;
        }
        Ok(p)
    } else {
        if let Some(Tag::IntArray(pos)) = m.get("pos") {
            ensure!(pos.len() == 3, "计划刻pos长度无效");
            let mut p = [0; 3];
            for a in 0..3 {
                p[a] = pos[a].checked_add(origin[a]).context("计划刻坐标溢出")?;
            }
            return Ok(p);
        }
        let keys = ["x", "y", "z"];
        let mut p = [0; 3];
        for a in 0..3 {
            p[a] = i32::try_from(integer(m.get(keys[a]).context("方块实体坐标不完整")?)?)?
                .checked_add(origin[a])
                .context("方块实体坐标溢出")?;
        }
        Ok(p)
    }
}
fn issue(report: &mut ConflictReport, text: String) {
    if !report.issues.contains(&text) {
        report.issues.push(text)
    }
}
fn entity_uuid(fields: &HashMap<String, Tag>) -> Option<[i32; 4]> {
    if let Some(Tag::IntArray(v)) = fields.get("UUID") {
        if v.len() == 4 {
            return Some([v[0], v[1], v[2], v[3]]);
        }
    }
    if let (Some(Tag::Long(most)), Some(Tag::Long(least))) =
        (fields.get("UUIDMost"), fields.get("UUIDLeast"))
    {
        return Some([
            (*most >> 32) as i32,
            *most as i32,
            (*least >> 32) as i32,
            *least as i32,
        ]);
    }
    None
}
fn instance_uuids(scene: &SceneDocument, instance: &str) -> Result<BTreeMap<[i32; 4], [i32; 4]>> {
    use sha2::{Digest, Sha256};
    fn visit(value: &Tag, instance: &str, ids: &mut BTreeMap<[i32; 4], [i32; 4]>) {
        if let Tag::Compound(map) = value {
            if let Some(old) = entity_uuid(map) {
                let mut h = Sha256::new();
                h.update(instance.as_bytes());
                for n in old {
                    h.update(n.to_be_bytes())
                }
                let mut bytes: [u8; 32] = h.finalize().into();
                bytes[6] = (bytes[6] & 0x0f) | 0x40;
                bytes[8] = (bytes[8] & 0x3f) | 0x80;
                let new = std::array::from_fn(|a| {
                    i32::from_be_bytes(bytes[a * 4..a * 4 + 4].try_into().unwrap())
                });
                ids.insert(old, new);
            }
            if let Some(Tag::List(passengers)) = map.get("Passengers") {
                for p in passengers {
                    visit(p, instance, ids)
                }
            }
        }
    }
    let mut ids = BTreeMap::new();
    for region in scene.root.regions.values() {
        for entity in &region.entities {
            let raw: Tag = fastnbt::from_bytes(&fastnbt::to_bytes(entity)?)?;
            visit(&raw, instance, &mut ids)
        }
    }
    Ok(ids)
}
fn remap_uuids(value: &mut Tag, ids: &BTreeMap<[i32; 4], [i32; 4]>) {
    match value {
        Tag::Compound(map) => {
            if let Some(new) = entity_uuid(map).and_then(|old| ids.get(&old).copied()) {
                if map.contains_key("UUID") {
                    map.insert(
                        "UUID".into(),
                        Tag::IntArray(fastnbt::IntArray::new(new.to_vec())),
                    );
                }
                if map.contains_key("UUIDMost") {
                    map.insert(
                        "UUIDMost".into(),
                        Tag::Long((i64::from(new[0]) << 32) | i64::from(new[1] as u32)),
                    );
                    map.insert(
                        "UUIDLeast".into(),
                        Tag::Long((i64::from(new[2]) << 32) | i64::from(new[3] as u32)),
                    );
                }
            }
            for (key, child) in map.iter_mut() {
                if matches!(
                    key.as_str(),
                    "Owner" | "OwnerUUID" | "Attach" | "LoveCause" | "ConversionPlayer"
                ) {
                    if let Tag::IntArray(v) = child {
                        if v.len() == 4 {
                            if let Some(new) = ids.get(&[v[0], v[1], v[2], v[3]]) {
                                *v = fastnbt::IntArray::new(new.to_vec());
                            }
                        }
                    }
                }
                remap_uuids(child, ids);
            }
        }
        Tag::List(values) => {
            for child in values {
                remap_uuids(child, ids)
            }
        }
        _ => {}
    }
}
fn compose(root: &Path, project: &CompositionProject, cancel: &AtomicBool) -> Result<Composed> {
    let mut sections: BTreeMap<[i32; 3], BuildSection> = BTreeMap::new();
    let mut report = ConflictReport::default();
    let mut prepared = Vec::new();
    let mut version = project.state.target_data_version;
    for part in project.state.parts.iter().filter(|p| p.visible) {
        cancelled(cancel)?;
        part.transform.validate()?;
        if let Some(b) = part.selection {
            b.validate()?;
        }
        let scene = source(root, &part.snapshot_id)?;
        if let Some(v) = scene.data_version() {
            if let Some(expected) = version {
                if v != expected {
                    issue(
                        &mut report,
                        format!(
                            "{}的数据版本{v}与组合版本{expected}不同；请先通过现有版本转换生成兼容部件",
                            part.name
                        ),
                    );
                    continue;
                }
            } else {
                version = Some(v)
            }
        } else {
            issue(&mut report, format!("{}缺少可确认的数据版本", part.name));
            continue;
        }
        let index = scene.index(cancel)?;
        if part.clear_air {
            // Only source-present air clears content. Synthetic missing cells
            // remain no-op; real structure_void stays a distinct block.
            for (name, region) in &scene.root.regions {
                if !choose_region(part, name) {
                    continue;
                }
                let source_presence = scene.presence_mask(name)?;
                let width = i64::from(region.size.x).unsigned_abs() as usize;
                let length = i64::from(region.size.z).unsigned_abs() as usize;
                nbt::for_each_palette_index(
                    &region.block_states,
                    nbt::region_volume(&region.size)?,
                    nbt::bits_for_palette(region.block_state_palette.len()),
                    region.block_state_palette.len(),
                    |i, palette| {
                        if i & 0x3fff == 0 {
                            cancelled(cancel)?;
                        }
                        if !composition_scene::mask_contains(source_presence, i) {
                            return Ok(());
                        }
                        let state = &region.block_state_palette[palette];
                        if !matches!(
                            state.name.as_str(),
                            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
                        ) {
                            return Ok(());
                        }
                        let (x, y, z) = nbt::storage_to_region_coords(
                            i % width,
                            i / (width * length),
                            (i / width) % length,
                            &region.size,
                        );
                        let p = [
                            region.position.x.checked_add(x).context("源坐标溢出")?,
                            region.position.y.checked_add(y).context("源坐标溢出")?,
                            region.position.z.checked_add(z).context("源坐标溢出")?,
                        ];
                        if part.selection.is_some_and(|b| !b.contains(p)) {
                            return Ok(());
                        }
                        let (c, k) = local_key(part.transform.block(p)?);
                        sections.entry(c).or_default().cells.insert(
                            k,
                            PlacedBlock {
                                state: state.clone(),
                                owner: part.id.clone(),
                                region: name.clone(),
                            },
                        );
                        Ok(())
                    },
                )?;
            }
        }
        let mut transformed: BTreeMap<(String, usize), Option<BlockStateNbt>> = BTreeMap::new();
        for section in index.sections.values() {
            cancelled(cancel)?;
            for cells in section.blocks.values() {
                for cell in cells {
                    if !choose_region(part, &cell.region)
                        || part.selection.is_some_and(|b| !b.contains(cell.position))
                    {
                        continue;
                    }
                    let key = (cell.region.clone(), cell.palette_index);
                    if !transformed.contains_key(&key) {
                        let state = match part.transform.state(scene.state(cell)?) {
                            Ok(s) => Some(s),
                            Err(e) => {
                                issue(&mut report, format!("{}：{e}", part.name));
                                None
                            }
                        };
                        transformed.insert(key.clone(), state);
                    }
                    let Some(state) = transformed[&key].as_ref() else {
                        continue;
                    };
                    let p = part.transform.block(cell.position)?;
                    let (c, k) = local_key(p);
                    let out = sections.entry(c).or_default();
                    if let Some(previous) = out.cells.get(&k) {
                        if !composition_scene::is_air(&previous.state) {
                            report.collision_count += 1;
                            if report.collisions.len() < 200 {
                                report.collisions.push(Collision {
                                    position: p,
                                    kept_instance: previous.owner.clone(),
                                    incoming_instance: part.id.clone(),
                                });
                            }
                            if project.state.conflict_policy != ConflictPolicy::KeepLast {
                                continue;
                            }
                        }
                    }
                    out.cells.insert(
                        k,
                        PlacedBlock {
                            state: state.clone(),
                            owner: part.id.clone(),
                            region: cell.region.clone(),
                        },
                    );
                }
            }
        }
        prepared.push((part, scene));
    }
    if report.collision_count > 0 && project.state.conflict_policy == ConflictPolicy::Reject {
        issue(
            &mut report,
            "存在非空气方块同格冲突，请明确选择保留前项或后项".into(),
        );
    }
    let mut raw_regions = HashMap::new();
    let mut provenance = Vec::new();
    let mut format_extras = HashMap::new();
    let mut ticks: BTreeMap<String, Vec<Tag>> = BTreeMap::new();
    let mut entities = Vec::new();
    let mut tiles = Vec::new();
    for (part, scene) in prepared {
        let entity_ids = instance_uuids(&scene, &part.id)?;
        if let Tag::Compound(fields) = &scene.raw {
            if let Some(Tag::Compound(extras)) = fields.get("NovaFormatExtras") {
                for (key, value) in extras {
                    if matches!(
                        value,
                        Tag::Compound(_)
                            | Tag::List(_)
                            | Tag::ByteArray(_)
                            | Tag::IntArray(_)
                            | Tag::LongArray(_)
                    ) {
                        issue(
                            &mut report,
                            format!("{}的格式扩展{}尚未定义组合/拆分空间语义", part.name, key),
                        );
                    } else {
                        format_extras
                            .entry(key.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
            let extras = fields
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "Regions" | "PreviewImageData"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            provenance.push(Tag::Compound(HashMap::from([
                ("InstanceId".into(), Tag::String(part.id.clone())),
                (
                    "SourceIdentity".into(),
                    Tag::String(part.snapshot_id.clone()),
                ),
                ("SourceMetadata".into(), Tag::Compound(extras)),
            ])));
        }
        for (name, r) in &scene.root.regions {
            if !choose_region(part, name) {
                continue;
            }
            let from = [r.position.x, r.position.y, r.position.z];
            if let Some(raw) = scene.region_raw(name) {
                if let Tag::Compound(fields) = raw {
                    for (key, value) in fields {
                        if matches!(
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
                            continue;
                        }
                        if matches!(
                            key.as_str(),
                            "PendingBlockTicks" | "PendingFluidTicks" | "NovaAnvilScheduledTicks"
                        ) {
                            if let Tag::List(items) = value {
                                for item in items {
                                    let mut item = item.clone();
                                    let point = tag_point(&item, from, false)?;
                                    if part.selection.is_some_and(|b| !b.contains(point)) {
                                        continue;
                                    }
                                    part.transform.block_entity(&mut item, from, [0; 3])?;
                                    ticks.entry(key.clone()).or_default().push(item);
                                }
                            } else {
                                issue(
                                    &mut report,
                                    format!("{}选区{}的{}不是有效计划刻列表", part.name, name, key),
                                );
                            }
                        } else {
                            issue(
                                &mut report,
                                format!(
                                    "{}选区{}含尚未定义合并语义的NBT字段{}",
                                    part.name, name, key
                                ),
                            );
                        }
                    }
                }
            }
            for entity in &r.entities {
                let mut raw: Tag = fastnbt::from_bytes(&fastnbt::to_bytes(entity)?)?;
                remap_uuids(&mut raw, &entity_ids);
                let anchor = match tag_point(&raw, from, true) {
                    Ok(p) => p,
                    Err(e) => {
                        issue(&mut report, format!("{}实体锚点：{e}", part.name));
                        continue;
                    }
                };
                if part.selection.is_some_and(|b| !b.contains(anchor)) {
                    continue;
                }
                if part.transform.is_oriented()
                    && !entity
                        .entity_id()
                        .is_some_and(|id| id.starts_with("minecraft:"))
                {
                    issue(
                        &mut report,
                        format!(
                            "{}：模组实体{}的变换语义未提供",
                            part.name,
                            entity.entity_id().unwrap_or("未知")
                        ),
                    );
                    continue;
                }
                match part.transform.entity(&mut raw, from, [0; 3]) {
                    Ok(()) => entities.push(raw),
                    Err(e) => issue(&mut report, format!("{}实体变换：{e}", part.name)),
                }
            }
            for tile in &r.tile_entities {
                let mut tile = tile.clone();
                let anchor = tag_point(&tile, from, false)?;
                if part.selection.is_some_and(|b| !b.contains(anchor)) {
                    continue;
                }
                let pos = part.transform.block(anchor)?;
                let (c, k) = local_key(pos);
                if sections.get(&c).and_then(|s| s.cells.get(&k)).is_none() {
                    issue(
                        &mut report,
                        format!(
                            "{}选区{}的方块实体位于缺失或空气格{anchor:?}",
                            part.name, name
                        ),
                    );
                    continue;
                }
                if !sections
                    .get(&c)
                    .and_then(|s| s.cells.get(&k))
                    .is_some_and(|v| v.owner == part.id && v.region == *name)
                {
                    continue;
                }
                if let Err(e) = part.transform.block_entity(&mut tile, from, [0; 3]) {
                    issue(&mut report, format!("{}方块实体变换：{e}", part.name));
                    continue;
                }
                tiles.push(tile);
            }
        }
    }
    for entity in &entities {
        let (c, _) = local_key(tag_point(entity, [0; 3], true)?);
        sections.entry(c).or_default();
    }
    for values in ticks.values() {
        for tick in values {
            let (c, _) = local_key(tag_point(tick, [0; 3], false)?);
            sections.entry(c).or_default();
        }
    }
    // Sparse output sections preserve full geometry without allocating the
    // empty enclosing cuboid of far-separated source instances.
    let mut total_volume = 0usize;
    for (c, section) in &sections {
        cancelled(cancel)?;
        let inside = |tag: &Tag, entity: bool| {
            tag_point(tag, [0; 3], entity).is_ok_and(|p| p.map(|v| v.div_euclid(16)) == *c)
        };
        let mut occupied_bounds: Option<SceneBounds> = None;
        let mut include = |p: [i32; 3]| {
            let b = SceneBounds { min: p, max: p };
            occupied_bounds = Some(occupied_bounds.map_or(b, |v| v.union(b)));
        };
        for key in section.cells.keys() {
            include(key_position(*c, *key));
        }
        for entity in entities.iter().filter(|v| inside(v, true)) {
            include(tag_point(entity, [0; 3], true)?);
        }
        for values in ticks.values() {
            for tick in values.iter().filter(|v| inside(v, false)) {
                include(tag_point(tick, [0; 3], false)?);
            }
        }
        let Some(occupied_bounds) = occupied_bounds else {
            continue;
        };
        report.bounds = Some(
            report
                .bounds
                .map_or(occupied_bounds, |v| v.union(occupied_bounds)),
        );
        let base = occupied_bounds.min;
        let region_size = occupied_bounds.size()?;
        let volume = region_size.iter().try_fold(1usize, |a, b| {
            a.checked_mul(*b as usize).context("区段体积溢出")
        })?;
        total_volume = total_volume.checked_add(volume).context("组合体积溢出")?;
        let mut palette = vec![BlockStateNbt {
            name: "minecraft:air".into(),
            properties: BTreeMap::new(),
        }];
        let mut palette_ids = BTreeMap::new();
        let mut values = vec![0usize; volume];
        let mut presence = vec![0i8; volume.div_ceil(8)];
        for (k, cell) in &section.cells {
            let state_key = serde_json::to_string(&cell.state)?;
            let index = if let Some(i) = palette_ids.get(&state_key) {
                *i
            } else {
                let i = palette.len();
                palette.push(cell.state.clone());
                palette_ids.insert(state_key, i);
                i
            };
            let p = key_position(*c, *k);
            let offset = std::array::from_fn::<_, 3, _>(|a| (p[a] - base[a]) as usize);
            let local = (offset[1] * region_size[2] as usize + offset[2]) * region_size[0] as usize
                + offset[0];
            values[local] = index;
            presence[local / 8] |= (1u8 << (local & 7)) as i8;
            let b = SceneBounds { min: p, max: p };
            report.bounds = Some(report.bounds.map_or(b, |v| v.union(b)));
            if !composition_scene::is_air(&cell.state) {
                report.block_count += 1;
            }
        }
        let mut region = HashMap::new();
        region.insert("Position".into(), vec_tag(base));
        region.insert("Size".into(), vec_tag(region_size));
        region.insert(
            "NovaPresenceMask".into(),
            Tag::ByteArray(fastnbt::ByteArray::new(presence)),
        );
        region.insert("BlockStatePalette".into(), tag_value(&palette)?);
        let packed = nbt::pack_palette_indices(&values, nbt::bits_for_palette(palette.len()));
        region.insert("BlockStates".into(), tag_value(&packed)?);
        let mut section_entities = entities
            .iter()
            .filter(|v| inside(v, true))
            .cloned()
            .collect::<Vec<_>>();
        let mut section_tiles = tiles
            .iter()
            .filter(|v| inside(v, false))
            .cloned()
            .collect::<Vec<_>>();
        for tag in &mut section_entities {
            Transform::default().entity(tag, [0; 3], base)?
        }
        for tag in &mut section_tiles {
            Transform::default().block_entity(tag, [0; 3], base)?
        }
        report.entity_count += section_entities.len() as u64;
        report.block_entity_count += section_tiles.len() as u64;
        for (key, values) in &ticks {
            let mut selected = values
                .iter()
                .filter(|v| inside(v, false))
                .cloned()
                .collect::<Vec<_>>();
            for tick in &mut selected {
                Transform::default().block_entity(tick, [0; 3], base)?;
            }
            region.insert(key.clone(), Tag::List(selected));
        }
        region.insert("Entities".into(), Tag::List(section_entities));
        region.insert("TileEntities".into(), Tag::List(section_tiles));
        raw_regions.insert(
            format!("section_{}_{}_{}", c[0], c[1], c[2]),
            Tag::Compound(region),
        );
    }
    report.section_count = raw_regions.len();
    report.can_export = report.issues.is_empty() && !raw_regions.is_empty();
    let size = report
        .bounds
        .map(|b| b.size())
        .transpose()?
        .unwrap_or([0; 3]);
    let metadata = Tag::Compound(HashMap::from([
        ("Name".into(), Tag::String(project.state.name.clone())),
        ("Author".into(), Tag::String("Litematica Nova".into())),
        (
            "Description".into(),
            Tag::String("Non-destructive composition export".into()),
        ),
        (
            "RegionCount".into(),
            Tag::Int(i32::try_from(raw_regions.len())?),
        ),
        (
            "TotalBlocks".into(),
            Tag::Int(i32::try_from(report.block_count)?),
        ),
        ("TotalVolume".into(), Tag::Int(i32::try_from(total_volume)?)),
        ("EnclosingSize".into(), vec_tag(size)),
    ]));
    let raw = Tag::Compound(HashMap::from([
        ("Version".into(), Tag::Int(6)),
        ("SubVersion".into(), Tag::Int(1)),
        (
            "MinecraftDataVersion".into(),
            Tag::Int(version.unwrap_or(0)),
        ),
        ("Metadata".into(), metadata),
        ("Regions".into(), Tag::Compound(raw_regions)),
        ("NovaCompositionSources".into(), Tag::List(provenance)),
        ("NovaFormatExtras".into(), Tag::Compound(format_extras)),
    ]));
    Ok(Composed {
        scene: composition_scene::from_raw_litematic(raw, "litematic")?,
        report,
    })
}
fn vec_tag(v: [i32; 3]) -> Tag {
    Tag::Compound(HashMap::from([
        ("x".into(), Tag::Int(v[0])),
        ("y".into(), Tag::Int(v[1])),
        ("z".into(), Tag::Int(v[2])),
    ]))
}
fn tag_value(v: &impl Serialize) -> Result<Tag> {
    #[derive(Serialize)]
    struct Envelope<'a, T: ?Sized> {
        value: &'a T,
    }
    let value: Tag = fastnbt::from_bytes(&fastnbt::to_bytes(&Envelope { value: v })?)?;
    let Tag::Compound(mut map) = value else {
        bail!("NBT编码器未生成Compound")
    };
    map.remove("value").context("NBT值编码失败")
}

fn project_view(root: &Path, project: &CompositionProject, cancel: &AtomicBool) -> Result<Value> {
    let mut parts = Vec::new();
    for p in &project.state.parts {
        let scene = source(root, &p.snapshot_id)?;
        let index = scene.index(cancel)?;
        let bounds = index
            .bounds
            .map(|b| {
                let a = p.transform.block(b.min)?;
                let z = p.transform.block(b.max)?;
                Ok::<_, anyhow::Error>(SceneBounds {
                    min: std::array::from_fn(|i| a[i].min(z[i])),
                    max: std::array::from_fn(|i| a[i].max(z[i])),
                })
            })
            .transpose()?;
        parts.push(json!({"part":p,"bounds":bounds,"size":bounds.map(|b|b.size()).transpose()?,"blockCount":index.non_air_blocks,"dataVersion":scene.data_version(),"regions":scene.root.regions.keys().collect::<Vec<_>>() }));
    }
    Ok(
        json!({"project":project_summary(project),"parts":parts,"canUndo":!project.undo.is_empty(),"canRedo":!project.redo.is_empty()}),
    )
}
fn project_summary(project: &CompositionProject) -> Value {
    json!({"schema":project.schema,"id":project.id,"generation":project.generation,"state":project.state})
}
pub fn execute(input: &Value, cancel: &AtomicBool) -> Result<Value> {
    cancelled(cancel)?;
    let root = PathBuf::from(
        input["storageDir"]
            .as_str()
            .context("组合持久化目录未配置")?,
    );
    ensure!(root.is_absolute(), "组合目录必须是绝对路径");
    let action = input["action"].as_str().unwrap_or("list");
    if action == "list" {
        let conn = db(&root)?;
        let mut statement = conn.prepare(
            "SELECT document FROM nova_composition_projects ORDER BY updated_at DESC,id",
        )?;
        let docs = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let projects = docs
            .into_iter()
            .map(|d| serde_json::from_str::<CompositionProject>(&d))
            .collect::<serde_json::Result<Vec<_>>>()?;
        return Ok(json!({"projects":projects.iter().map(project_summary).collect::<Vec<_>>()}));
    }
    let mut project = if action == "create" {
        CompositionProject {
            schema: 1,
            id: identifier(),
            generation: 1,
            state: ProjectState {
                name: input["name"].as_str().unwrap_or("新组合").into(),
                parts: vec![],
                target_data_version: None,
                conflict_policy: ConflictPolicy::Reject,
            },
            undo: vec![],
            redo: vec![],
        }
    } else {
        load(
            &root,
            input["projectId"].as_str().context("请选择组合项目")?,
        )?
    };
    let expected = project.generation;
    if let Some(g) = input["generation"].as_u64() {
        ensure!(g == expected, "组合项目已变化，请重试当前操作");
    }
    if action == "open" {
        return project_view(&root, &project, cancel);
    }
    if matches!(action, "migrationReport" | "migrate") {
        let target = if let Some(target) = input["targetVersion"].as_str() {
            target.to_owned()
        } else {
            let data_version = project
                .state
                .target_data_version
                .or_else(|| {
                    project
                        .state
                        .parts
                        .first()
                        .and_then(|p| source(&root, &p.snapshot_id).ok()?.data_version())
                })
                .context("首部件没有可确认的数据版本，请明确选择目标版本")?;
            crate::block_database::database()?
                .version_for_data_version(data_version)
                .context("此数据版本没有实际转换注册表")?
                .to_owned()
        };
        let target_data_version = crate::block_database::database()?
            .version(&target)?
            .data_version;
        let directory = root.join("composition/conversion");
        fs::create_dir_all(&directory)?;
        let staging = tempfile::tempdir_in(directory)?;
        let mut cache = crate::version_conversion::ConversionCache::new(512 * 1024 * 1024);
        let mut results = Vec::new();
        let mut replacements = Vec::new();
        let mut writable = true;
        for part in &project.state.parts {
            cancelled(cancel)?;
            if source(&root, &part.snapshot_id)?.data_version() == Some(target_data_version) {
                continue;
            }
            let request = crate::version_conversion::ConversionRequest {
                target_version: target.clone(),
                replacements: BTreeMap::new(),
                expected_source_sha256: None,
                overwrite: false,
                selection: None,
                property_edit: None,
            };
            let output = staging.path().join(format!("{}.litematic", part.id));
            let preview = cache.convert(
                &snapshot_path(&root, &part.snapshot_id)?,
                if action == "migrate" {
                    Some(output.as_path())
                } else {
                    None
                },
                &request,
                cancel,
                |_| {},
            )?;
            writable &= preview.writable;
            if action == "migrate" && preview.writable {
                let scene = decode_source(&output, cancel)?;
                ensure!(
                    scene.data_version() == Some(target_data_version),
                    "版本转换结果身份不符"
                );
                replacements.push((part.id.clone(), save_source(&root, scene, cancel)?));
            }
            results.push(json!({"partId":part.id,"name":part.name,"conversion":preview}));
        }
        if action == "migrationReport" {
            return Ok(
                json!({"projectId":project.id,"generation":project.generation,"targetVersion":target,"targetDataVersion":target_data_version,"writable":writable,"migrationReports":results}),
            );
        }
        ensure!(writable, "有部件不能无损转换；请先查看版本迁移报告");
        let before = project.state.clone();
        for (id, snapshot) in replacements {
            project
                .state
                .parts
                .iter_mut()
                .find(|p| p.id == id)
                .context("转换期间部件消失")?
                .snapshot_id = snapshot;
        }
        project.state.target_data_version = Some(target_data_version);
        project.undo.push(before);
        project.redo.clear();
        project.generation += 1;
        cancelled(cancel)?;
        persist(&root, &project, Some(expected))?;
        return project_view(&root, &project, cancel);
    }
    if matches!(
        action,
        "report" | "preview" | "export" | "splitReport" | "split"
    ) {
        let composed = compose(&root, &project, cancel)?;
        if action == "report" {
            return Ok(
                json!({"projectId":project.id,"generation":project.generation,"report":composed.report}),
            );
        }
        ensure!(
            composed.report.can_export,
            "组合尚有未解决问题：{}",
            composed.report.issues.join("；")
        );
        if action == "split" || action == "splitReport" {
            return split(
                &root,
                &project,
                &composed.scene,
                &composed.report,
                input,
                cancel,
            );
        }
        let output = if action == "preview" {
            let directory = root.join("composition/previews");
            fs::create_dir_all(&directory)?;
            directory.join(format!("{}-{}.litematic", project.id, project.generation))
        } else {
            PathBuf::from(input["outputPath"].as_str().context("请选择导出文件")?)
        };
        ensure!(
            project.state.parts.iter().all(|p| p.source_path != output),
            "不能覆盖组合源文件"
        );
        let format: FormatId = if action == "preview" {
            FormatId::Litematic
        } else {
            serde_json::from_value(input.get("format").cloned().unwrap_or(json!("litematic")))?
        };
        if action == "preview" && output.exists() {
            let existing = decode_source(&output, cancel)?;
            ensure!(
                composition_scene::content_identity(&existing.raw)
                    == composition_scene::content_identity(&composed.scene.raw),
                "组合预览缓存身份不一致，请重新准备"
            );
            return Ok(
                json!({"projectId":project.id,"generation":project.generation,"outputPath":output,"report":composed.report,"cacheReused":true}),
            );
        }
        let result = schematic_formats::encode(&composed.scene, format, &output, cancel)?;
        return Ok(
            json!({"projectId":project.id,"generation":project.generation,"outputPath":output,"report":composed.report,"formatReport":result}),
        );
    }
    let before = project.state.clone();
    match action {
        "create" | "add" => {
            let paths = input["sourcePaths"].as_array().context("请选择源投影")?;
            for value in paths {
                cancelled(cancel)?;
                let path = PathBuf::from(value.as_str().context("源路径无效")?);
                let scene = decode_source(&path, cancel)?;
                if project.state.target_data_version.is_none() {
                    project.state.target_data_version = scene.data_version();
                }
                let snapshot_id = save_source(&root, scene, cancel)?;
                let mut transform = Transform::default();
                if input["placement"] == "beside" {
                    let scene = source(&root, &snapshot_id)?;
                    let current = compose(&root, &project, cancel)?.report.bounds;
                    if let (Some(a), Some(b)) = (current, scene.index(cancel)?.bounds) {
                        transform.offset[0] = a.max[0]
                            .checked_add(2)
                            .and_then(|v| v.checked_sub(b.min[0]))
                            .context("组合偏移溢出")?;
                    }
                }
                project.state.parts.push(PartInstance {
                    id: identifier(),
                    name: path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    source_path: path,
                    snapshot_id,
                    selection: None,
                    regions: vec![],
                    transform,
                    visible: true,
                    locked: false,
                    clear_air: false,
                });
            }
        }
        "update" => {
            let id = input["partId"].as_str().context("请选择部件")?;
            let p = project
                .state
                .parts
                .iter_mut()
                .find(|p| p.id == id)
                .context("部件已删除")?;
            let patch = &input["patch"];
            if let Some(lock) = patch["locked"].as_bool() {
                p.locked = lock;
            }
            ensure!(
                !p.locked
                    || patch
                        .as_object()
                        .is_some_and(|v| v.len() == 1 && v.contains_key("locked")),
                "部件已锁定"
            );
            if let Some(v) = patch["name"].as_str() {
                ensure!(!v.trim().is_empty(), "部件名称不能为空");
                p.name = v.into()
            }
            if let Some(v) = patch["visible"].as_bool() {
                p.visible = v
            }
            if let Some(v) = patch["clearAir"].as_bool() {
                p.clear_air = v
            }
            if let Some(v) = patch.get("transform") {
                p.transform = serde_json::from_value::<Transform>(v.clone())?.validate()?
            }
            if let Some(v) = patch.get("selection") {
                p.selection = serde_json::from_value::<Option<SceneBounds>>(v.clone())?
                    .map(SceneBounds::validate)
                    .transpose()?
            }
            if let Some(v) = patch.get("regions") {
                p.regions = serde_json::from_value(v.clone())?;
                let scene = source(&root, &p.snapshot_id)?;
                ensure!(
                    p.regions.iter().all(|n| scene.root.regions.contains_key(n)),
                    "选择的源选区不存在"
                );
            }
        }
        "remove" | "duplicate" => {
            let id = input["partId"].as_str().context("请选择部件")?;
            let index = project
                .state
                .parts
                .iter()
                .position(|p| p.id == id)
                .context("部件已删除")?;
            ensure!(!project.state.parts[index].locked, "部件已锁定");
            if action == "remove" {
                project.state.parts.remove(index);
            } else {
                let mut p = project.state.parts[index].clone();
                p.id = identifier();
                p.name = format!("{} 副本", p.name);
                project.state.parts.insert(index + 1, p);
            }
        }
        "reorder" => {
            let ids: Vec<String> = serde_json::from_value(input["partIds"].clone())?;
            ensure!(ids.len() == project.state.parts.len(), "部件顺序不完整");
            let mut parts = BTreeMap::new();
            for p in project.state.parts.drain(..) {
                parts.insert(p.id.clone(), p);
            }
            for id in ids {
                project
                    .state
                    .parts
                    .push(parts.remove(&id).context("部件顺序包含重复或未知身份")?);
            }
            ensure!(parts.is_empty(), "部件顺序不完整");
        }
        "save" => {
            if let Some(v) = input["name"].as_str() {
                ensure!(!v.trim().is_empty(), "组合名称不能为空");
                project.state.name = v.into()
            }
            if let Some(v) = input.get("conflictPolicy") {
                project.state.conflict_policy = serde_json::from_value(v.clone())?
            }
        }
        "undo" => {
            if let Some(state) = project.undo.pop() {
                project.redo.push(project.state);
                project.state = state;
            } else {
                bail!("没有可撤销的操作")
            }
        }
        "redo" => {
            if let Some(state) = project.redo.pop() {
                project.undo.push(project.state);
                project.state = state;
            } else {
                bail!("没有可重做的操作")
            }
        }
        _ => bail!("不支持的组合操作:{action}"),
    }
    if !matches!(action, "create" | "undo" | "redo") {
        project.undo.push(before);
        project.redo.clear();
    }
    project.generation = project.generation.checked_add(1).context("组合代次溢出")?;
    cancelled(cancel)?;
    persist(
        &root,
        &project,
        if action == "create" {
            None
        } else {
            Some(expected)
        },
    )?;
    project_view(&root, &project, cancel)
}

fn split(
    root: &Path,
    project: &CompositionProject,
    scene: &SceneDocument,
    report: &ConflictReport,
    input: &Value,
    cancel: &AtomicBool,
) -> Result<Value> {
    let mode = input["splitMode"].as_str().unwrap_or("boxes");
    let bounds = report.bounds.context("组合为空")?;
    let boxes: Vec<SplitBox> = match mode {
        "regions" => {
            let mut result = Vec::new();
            for part in project.state.parts.iter().filter(|p| p.visible) {
                let source = source(root, &part.snapshot_id)?;
                for (name, r) in &source.root.regions {
                    if !choose_region(part, name) {
                        continue;
                    }
                    let b = nbt::region_bounds(r);
                    let mut b = SceneBounds {
                        min: [b.min_x, b.min_y, b.min_z],
                        max: [b.max_x, b.max_y, b.max_z],
                    };
                    if let Some(selection) = part.selection {
                        if !b.intersects(selection) {
                            continue;
                        }
                        b = SceneBounds {
                            min: std::array::from_fn(|a| b.min[a].max(selection.min[a])),
                            max: std::array::from_fn(|a| b.max[a].min(selection.max[a])),
                        };
                    }
                    let a = part.transform.block(b.min)?;
                    let z = part.transform.block(b.max)?;
                    let safe = name
                        .chars()
                        .map(|c| {
                            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                                '_'
                            } else {
                                c
                            }
                        })
                        .collect::<String>();
                    result.push(SplitBox {
                        // Instances may share an ID prefix and distinct region
                        // names may become equal after filename sanitization.
                        // A deterministic output ordinal disambiguates both.
                        name: format!("part_{:03}_{}", result.len() + 1, safe),
                        bounds: SceneBounds {
                            min: std::array::from_fn(|a2| a[a2].min(z[a2])),
                            max: std::array::from_fn(|a2| a[a2].max(z[a2])),
                        },
                    });
                }
            }
            result
        }
        "plane" => {
            let axis = match input["axis"].as_str().unwrap_or("y") {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                _ => bail!("切割轴无效"),
            };
            let at = i32::try_from(input["at"].as_i64().context("请输入整数切割位置")?)?;
            ensure!(
                at > bounds.min[axis] && at <= bounds.max[axis],
                "切割平面必须位于投影内部"
            );
            let mut a = bounds;
            let mut b = bounds;
            a.max[axis] = at - 1;
            b.min[axis] = at;
            vec![
                SplitBox {
                    name: "part_1".into(),
                    bounds: a,
                },
                SplitBox {
                    name: "part_2".into(),
                    bounds: b,
                },
            ]
        }
        "boxes" => serde_json::from_value(input["boxes"].clone())?,
        _ => bail!("不支持的拆分方式"),
    };
    ensure!(!boxes.is_empty(), "至少需要一个拆分区域");
    for (i, a) in boxes.iter().enumerate() {
        a.bounds.validate()?;
        ensure!(
            !a.name.is_empty()
                && !a.name.contains(['/', '\\', ':'])
                && a.name != "."
                && a.name != "..",
            "拆分名称不是有效文件名"
        );
        for b in &boxes[..i] {
            ensure!(
                a.name != b.name && !a.bounds.intersects(b.bounds),
                "拆分区域不能重名或交叉：{} / {}",
                a.name,
                b.name
            )
        }
    }
    let id = save_source(
        root,
        composition_scene::from_raw_litematic(scene.raw_litematic()?, "litematic")?,
        cancel,
    )?;
    let mut outputs = Vec::new();
    let mut pieces = Vec::new();
    for b in &boxes {
        cancelled(cancel)?;
        let mut p = project.clone();
        p.state.parts = vec![PartInstance {
            id: identifier(),
            name: b.name.clone(),
            source_path: PathBuf::new(),
            snapshot_id: id.clone(),
            selection: Some(b.bounds),
            regions: vec![],
            transform: Transform::default(),
            visible: true,
            locked: false,
            clear_air: false,
        }];
        p.state.name = b.name.clone();
        p.state.conflict_policy = ConflictPolicy::Reject;
        let result = compose(root, &p, cancel)?;
        ensure!(result.report.can_export, "拆分{}存在不支持内容", b.name);
        outputs.push(json!({"name":b.name,"bounds":b.bounds,"report":result.report}));
        pieces.push((b, result.scene));
    }
    if input["action"] == "splitReport" {
        return Ok(
            json!({"projectId":project.id,"generation":project.generation,"anchorPolicy":"entity Pos floor; complete passenger tree follows root","outputs":outputs}),
        );
    }
    if input["splitOutput"] == "project" {
        let mut parts = Vec::new();
        for (b, scene) in pieces {
            let snapshot_id = save_source(root, scene, cancel)?;
            parts.push(PartInstance {
                id: identifier(),
                name: b.name.clone(),
                source_path: PathBuf::new(),
                snapshot_id,
                selection: None,
                regions: vec![],
                transform: Transform::default(),
                visible: true,
                locked: false,
                clear_air: false,
            });
        }
        let next = CompositionProject {
            schema: 1,
            id: identifier(),
            generation: 1,
            state: ProjectState {
                name: format!("{} 拆分", project.state.name),
                parts,
                target_data_version: project.state.target_data_version,
                conflict_policy: ConflictPolicy::Reject,
            },
            undo: vec![],
            redo: vec![],
        };
        cancelled(cancel)?;
        persist(root, &next, None)?;
        let mut view = project_view(root, &next, cancel)?;
        view["outputs"] = json!(outputs);
        return Ok(view);
    }
    let destination = PathBuf::from(
        input["outputDirectory"]
            .as_str()
            .context("请选择拆分输出目录")?,
    );
    ensure!(
        destination.is_absolute() && destination.is_dir(),
        "请选择现有绝对输出目录"
    );
    let staging = tempfile::tempdir_in(&destination)?;
    let mut published = Vec::new();
    for (b, s) in &pieces {
        let final_path = destination.join(format!("{}.litematic", b.name));
        ensure!(
            !final_path.exists(),
            "拆分文件已存在，不覆盖：{}",
            final_path.display()
        );
        schematic_formats::encode(
            s,
            FormatId::Litematic,
            &staging.path().join(format!("{}.litematic", b.name)),
            cancel,
        )?;
    }
    let result = (|| -> Result<()> {
        for (b, _) in &pieces {
            cancelled(cancel)?;
            let name = format!("{}.litematic", b.name);
            let final_path = destination.join(&name);
            let mut prepared = tempfile::NamedTempFile::new_in(&destination)?;
            let mut input = fs::File::open(staging.path().join(name))?;
            std::io::copy(&mut input, prepared.as_file_mut())?;
            prepared.as_file().sync_all()?;
            cancelled(cancel)?;
            prepared
                .persist_noclobber(&final_path)
                .map_err(|e| e.error)?;
            published.push(final_path);
        }
        Ok(())
    })();
    if let Err(error) = result {
        for p in &published {
            let _ = fs::remove_file(p);
        }
        return Err(error);
    }
    Ok(
        json!({"projectId":project.id,"generation":project.generation,"outputs":outputs,"outputPaths":published,"anchorPolicy":"entity Pos floor; complete passenger tree follows root"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> SceneDocument {
        let root = Tag::Compound(HashMap::from([
            ("Version".into(), Tag::Int(6)),
            ("SubVersion".into(), Tag::Int(1)),
            ("MinecraftDataVersion".into(), Tag::Int(3465)),
            (
                "Metadata".into(),
                Tag::Compound(HashMap::from([
                    ("Name".into(), Tag::String("fixture".into())),
                    ("Author".into(), Tag::String("fixture author".into())),
                ])),
            ),
            (
                "Regions".into(),
                Tag::Compound(HashMap::from([(
                    "source".into(),
                    Tag::Compound(HashMap::from([
                        ("Position".into(), vec_tag([-17, 4, -1])),
                        ("Size".into(), vec_tag([2, 1, 1])),
                        (
                            "BlockStatePalette".into(),
                            Tag::List(vec![
                                Tag::Compound(HashMap::from([(
                                    "Name".into(),
                                    Tag::String("minecraft:stone".into()),
                                )])),
                                Tag::Compound(HashMap::from([
                                    ("Name".into(), Tag::String("minecraft:chest".into())),
                                    (
                                        "Properties".into(),
                                        Tag::Compound(HashMap::from([
                                            ("facing".into(), Tag::String("north".into())),
                                            ("type".into(), Tag::String("single".into())),
                                            ("waterlogged".into(), Tag::String("false".into())),
                                        ])),
                                    ),
                                ])),
                            ]),
                        ),
                        (
                            "BlockStates".into(),
                            tag_value(&nbt::pack_palette_indices(&[0, 1], 2)).unwrap(),
                        ),
                        (
                            "TileEntities".into(),
                            Tag::List(vec![Tag::Compound(HashMap::from([
                                ("id".into(), Tag::String("minecraft:chest".into())),
                                ("x".into(), Tag::Int(1)),
                                ("y".into(), Tag::Int(0)),
                                ("z".into(), Tag::Int(0)),
                                (
                                    "Items".into(),
                                    Tag::List(vec![Tag::Compound(HashMap::from([
                                        ("id".into(), Tag::String("minecraft:diamond".into())),
                                        ("Count".into(), Tag::Byte(7)),
                                        ("Slot".into(), Tag::Byte(2)),
                                    ]))]),
                                ),
                            ]))]),
                        ),
                        (
                            "Entities".into(),
                            Tag::List(vec![Tag::Compound(HashMap::from([
                                ("id".into(), Tag::String("minecraft:pig".into())),
                                (
                                    "Pos".into(),
                                    Tag::List(vec![
                                        Tag::Double(0.5),
                                        Tag::Double(0.0),
                                        Tag::Double(0.5),
                                    ]),
                                ),
                                (
                                    "Rotation".into(),
                                    Tag::List(vec![Tag::Float(0.0), Tag::Float(0.0)]),
                                ),
                                (
                                    "CustomName".into(),
                                    Tag::String("{\"text\":\"保留实体\"}".into()),
                                ),
                            ]))]),
                        ),
                        (
                            "PendingBlockTicks".into(),
                            Tag::List(vec![Tag::Compound(HashMap::from([
                                ("x".into(), Tag::Int(0)),
                                ("y".into(), Tag::Int(0)),
                                ("z".into(), Tag::Int(0)),
                                ("Block".into(), Tag::String("minecraft:stone".into())),
                                ("Time".into(), Tag::Int(11)),
                            ]))]),
                        ),
                    ])),
                )])),
            ),
        ]));
        composition_scene::from_raw_litematic(root, "litematic").unwrap()
    }
    fn project(root: &Path, second_offset: i32) -> CompositionProject {
        let id = save_source(root, fixture(), &AtomicBool::new(false)).unwrap();
        let first = PartInstance {
            id: "instance-a".into(),
            name: "source".into(),
            source_path: root.join("source.litematic"),
            snapshot_id: id,
            selection: None,
            regions: vec![],
            transform: Transform::default(),
            visible: true,
            locked: false,
            clear_air: false,
        };
        let mut second = first.clone();
        second.id = "instance-b".into();
        second.transform.offset[0] = second_offset;
        CompositionProject {
            schema: 1,
            id: "project-test".into(),
            generation: 1,
            state: ProjectState {
                name: "test".into(),
                parts: vec![first, second],
                target_data_version: Some(3465),
                conflict_policy: ConflictPolicy::Reject,
            },
            undo: vec![],
            redo: vec![],
        }
    }
    fn spatial(scene: &SceneDocument) -> BTreeMap<[i32; 3], String> {
        let mut result = BTreeMap::new();
        for section in scene
            .index(&AtomicBool::new(false))
            .unwrap()
            .sections
            .values()
        {
            for cells in section.blocks.values() {
                for cell in cells {
                    result.insert(cell.position, scene.state(cell).unwrap().name.clone());
                }
            }
        }
        result
    }
    #[test]
    fn duplicate_instances_keep_negative_positions_and_conflicts_require_explicit_policy() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project(dir.path(), 0);
        let c = compose(dir.path(), &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(c.report.collision_count, 2);
        assert!(!c.report.can_export);
        p.state.conflict_policy = ConflictPolicy::KeepLast;
        let c = compose(dir.path(), &p, &AtomicBool::new(false)).unwrap();
        assert!(c.report.can_export);
        assert_eq!(c.report.block_count, 2);
        assert_eq!(c.report.block_entity_count, 1);
        assert_eq!(c.report.entity_count, 2);
        p.state.parts[1].transform.offset[0] = 32;
        let c = compose(dir.path(), &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(c.report.block_count, 4);
        assert_eq!(c.report.block_entity_count, 2);
        let map = spatial(&c.scene);
        assert_eq!(map[&[-17, 4, -1]], "minecraft:stone");
        assert_eq!(map[&[16, 4, -1]], "minecraft:chest");
        let flat = composition_scene::flatten(&c.scene, &AtomicBool::new(false)).unwrap();
        assert_eq!(spatial(&flat), map);
        assert_eq!(flat.root.regions.len(), 1);
        let tiles = &flat.root.regions.values().next().unwrap().tile_entities;
        assert_eq!(tiles.len(), 2);
        assert!(format!("{tiles:?}").contains("diamond"));
    }
    #[test]
    fn project_reopens_with_undo_boundary_and_stale_updates_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), 32);
        persist(dir.path(), &p, None).unwrap();
        SOURCES.get().unwrap().lock().unwrap().clear();
        let p = load(dir.path(), &p.id).unwrap();
        let scene = source(dir.path(), &p.state.parts[0].snapshot_id).unwrap();
        assert_eq!(scene.content_id, p.state.parts[0].snapshot_id);
        let input = json!({"storageDir":dir.path(),"projectId":p.id,"generation":1,"action":"update","partId":"instance-a","patch":{"transform":{"offset":[-2,0,0],"origin":[0,0,0],"rotation":90,"mirrorX":true,"mirrorZ":false}}});
        let updated = execute(&input, &AtomicBool::new(false)).unwrap();
        assert_eq!(updated["project"]["generation"], 2);
        assert!(execute(&input, &AtomicBool::new(false)).is_err());
        execute(
            &json!({"storageDir":dir.path(),"projectId":p.id,"generation":2,"action":"undo"}),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            load(dir.path(), &p.id).unwrap().state.parts[0].transform,
            Transform::default()
        );
        let undone = load(dir.path(), &p.id).unwrap();
        assert!(!undone.redo.is_empty());
    }
    #[test]
    fn split_plane_and_original_regions_preserve_spatial_entity_and_container_results() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), 32);
        let cancel = AtomicBool::new(false);
        let c = compose(dir.path(), &p, &cancel).unwrap();
        let output = dir.path().join("outputs");
        fs::create_dir(&output).unwrap();
        let result=split(dir.path(),&p,&c.scene,&c.report,&json!({"action":"split","splitMode":"plane","axis":"x","at":0,"outputDirectory":output}),&cancel).unwrap();
        let mut found = BTreeMap::new();
        let mut entity_count = 0;
        let mut tile_count = 0;
        for path in result["outputPaths"].as_array().unwrap() {
            let scene =
                schematic_formats::decode(Path::new(path.as_str().unwrap()), &cancel).unwrap();
            for (k, v) in spatial(&scene) {
                assert!(found.insert(k, v).is_none());
            }
            for r in scene.root.regions.values() {
                entity_count += r.entities.len();
                tile_count += r.tile_entities.len();
            }
        }
        assert_eq!(found, spatial(&c.scene));
        assert_eq!(entity_count, 2);
        assert_eq!(tile_count, 2);
        let original = split(
            dir.path(),
            &p,
            &c.scene,
            &c.report,
            &json!({"action":"splitReport","splitMode":"regions"}),
            &cancel,
        )
        .unwrap();
        assert_eq!(original["outputs"].as_array().unwrap().len(), 2);
        let bad = split(
            dir.path(),
            &p,
            &c.scene,
            &c.report,
            &json!({"action":"splitReport","splitMode":"boxes","boxes":[{"name":"a","bounds":{"min":[-20,0,-2],"max":[0,10,0]}},{"name":"b","bounds":{"min":[-1,0,-2],"max":[20,10,0]}}]}),
            &cancel,
        );
        assert!(bad.is_err());
    }
    #[test]
    fn cancellation_never_publishes_or_mutates_a_project() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), 32);
        persist(dir.path(), &p, None).unwrap();
        let result = execute(
            &json!({"storageDir":dir.path(),"projectId":p.id,"generation":1,"action":"remove","partId":"instance-a"}),
            &AtomicBool::new(true),
        );
        assert!(result.is_err());
        assert_eq!(load(dir.path(), &p.id).unwrap().generation, 1);
    }
    #[test]
    fn synthetic_air_never_clears_but_explicit_air_can_be_requested() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project(dir.path(), 0);
        p.state.conflict_policy = ConflictPolicy::KeepLast;
        let mut raw = fixture().raw_litematic().unwrap();
        if let Tag::Compound(root) = &mut raw {
            if let Some(Tag::Compound(regions)) = root.get_mut("Regions") {
                if let Some(Tag::Compound(region)) = regions.get_mut("source") {
                    region.insert(
                        "BlockStatePalette".into(),
                        Tag::List(vec![
                            Tag::Compound(HashMap::from([(
                                "Name".into(),
                                Tag::String("minecraft:air".into()),
                            )])),
                            Tag::Compound(HashMap::from([(
                                "Name".into(),
                                Tag::String("minecraft:chest".into()),
                            )])),
                        ]),
                    );
                    region.insert(
                        "NovaPresenceMask".into(),
                        Tag::ByteArray(fastnbt::ByteArray::new(vec![2])),
                    );
                }
            }
        }
        p.state.parts[1].snapshot_id = save_source(
            dir.path(),
            composition_scene::from_raw_litematic(raw.clone(), "structure").unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap();
        p.state.parts[1].clear_air = true;
        let preserved = compose(dir.path(), &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(spatial(&preserved.scene)[&[-17, 4, -1]], "minecraft:stone");
        if let Tag::Compound(root) = &mut raw {
            if let Some(Tag::Compound(regions)) = root.get_mut("Regions") {
                if let Some(Tag::Compound(region)) = regions.get_mut("source") {
                    region.insert(
                        "NovaPresenceMask".into(),
                        Tag::ByteArray(fastnbt::ByteArray::new(vec![3])),
                    );
                }
            }
        }
        p.state.parts[1].snapshot_id = save_source(
            dir.path(),
            composition_scene::from_raw_litematic(raw, "structure").unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let cleared = compose(dir.path(), &p, &AtomicBool::new(false)).unwrap();
        assert!(!spatial(&cleared.scene).contains_key(&[-17, 4, -1]));
        assert_eq!(cleared.report.block_count, 1);
        let flat = composition_scene::flatten(&cleared.scene, &AtomicBool::new(false)).unwrap();
        let region = flat.root.regions.values().next().unwrap();
        assert!(composition_scene::cell_is_present(&flat, "merged", 0).unwrap());
        assert_eq!(
            region.block_state_palette[nbt::palette_index_at(
                &region.block_states,
                nbt::region_volume(&region.size).unwrap(),
                nbt::bits_for_palette(region.block_state_palette.len()),
                region.block_state_palette.len(),
                0
            )
            .unwrap()]
            .name,
            "minecraft:air"
        );
    }
    #[test]
    fn duplicated_entities_receive_stable_distinct_instance_uuids() {
        let scene = fixture();
        let raw = Tag::Compound(HashMap::from([
            ("id".into(), Tag::String("minecraft:pig".into())),
            (
                "UUID".into(),
                Tag::IntArray(fastnbt::IntArray::new(vec![1, 2, 3, 4])),
            ),
        ]));
        let mut a = scene.raw_litematic().unwrap();
        if let Tag::Compound(root) = &mut a {
            if let Some(Tag::Compound(regions)) = root.get_mut("Regions") {
                if let Some(Tag::Compound(region)) = regions.get_mut("source") {
                    region.insert("Entities".into(), Tag::List(vec![raw.clone()]));
                }
            }
        }
        let scene = composition_scene::from_raw_litematic(a, "litematic").unwrap();
        let first = instance_uuids(&scene, "a").unwrap();
        let second = instance_uuids(&scene, "b").unwrap();
        assert_ne!(first[&[1, 2, 3, 4]], second[&[1, 2, 3, 4]]);
        assert_eq!(first, instance_uuids(&scene, "a").unwrap());
        let mut transformed = raw;
        remap_uuids(&mut transformed, &first);
        let Tag::Compound(fields) = transformed else {
            panic!()
        };
        assert_eq!(entity_uuid(&fields).unwrap(), first[&[1, 2, 3, 4]]);
    }
    #[test]
    fn manual_split_can_create_a_persistent_reopenable_composition() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), 32);
        let cancel = AtomicBool::new(false);
        let c = compose(dir.path(), &p, &cancel).unwrap();
        let result=split(dir.path(),&p,&c.scene,&c.report,&json!({"action":"split","splitMode":"boxes","splitOutput":"project","boxes":[{"name":"left","bounds":{"min":[-17,4,-1],"max":[-16,4,-1]}},{"name":"right","bounds":{"min":[15,4,-1],"max":[16,4,-1]}}]}),&cancel).unwrap();
        let next = load(dir.path(), result["project"]["id"].as_str().unwrap()).unwrap();
        assert_eq!(next.state.parts.len(), 2);
        assert_eq!(
            spatial(&compose(dir.path(), &next, &cancel).unwrap().scene),
            spatial(&c.scene)
        );
    }
}
