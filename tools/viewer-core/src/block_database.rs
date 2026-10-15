//! One embedded, versioned game-data source: the imported PMBLKDB resources and
//! exact registry supplement. Reader is Nova code written against the documented
//! binary format; no reference application code or UI is incorporated.
use anyhow::{Context, Result, bail};
use base64::Engine;
use flate2::read::ZlibDecoder;
use image::{ImageEncoder, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    sync::OnceLock,
};

#[derive(Debug, Deserialize)]
pub(crate) struct Registry {
    pub blocks: BTreeMap<String, TargetBlock>,
    pub items: BTreeSet<String>,
    #[serde(default)]
    pub legacy_blocks: BTreeMap<String, String>,
    #[serde(default)]
    pub legacy_items: BTreeMap<String, String>,
    #[serde(default)]
    pub legacy_item_ids: BTreeMap<String, String>,
    #[serde(default)]
    pub legacy_state_aliases: BTreeMap<String, Vec<String>>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct TargetBlock {
    pub default: String,
    pub properties: BTreeMap<String, Vec<String>>,
    pub state_count: usize,
    #[serde(default)]
    pub legacy_states: BTreeSet<String>,
    #[serde(default)]
    pub en: String,
    #[serde(default)]
    pub category: String,
}
impl TargetBlock {
    pub fn accepts(&self, state: &str, props: &BTreeMap<String, String>) -> bool {
        if !self.legacy_states.is_empty() {
            return self.legacy_states.contains(state);
        }
        self.properties.len() == props.len()
            && self
                .properties
                .iter()
                .all(|(k, values)| props.get(k).is_some_and(|value| values.contains(value)))
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Version {
    pub data_version: i32,
    pub resource_version: String,
    pub registry: String,
    pub legacy: bool,
    pub block_count: usize,
    pub state_count: usize,
    pub output_extension: String,
}
#[derive(Deserialize)]
struct Supplement {
    schema: u32,
    versions: BTreeMap<String, Version>,
    registries: BTreeMap<String, Registry>,
    #[serde(default)]
    map_colors: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    map_color_sources: BTreeMap<String, String>,
    #[serde(default)]
    map_color_versions: BTreeMap<String, MapColorVersion>,
    #[serde(default)]
    numeric_map_colors: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    numeric_map_color_sources: BTreeMap<String, String>,
    plains: BTreeMap<String, BTreeMap<String, u32>>,
    #[serde(default)]
    special_planes: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}
#[derive(Deserialize)]
struct MapColorVersion {
    data_version: Option<i32>,
    block_count: usize,
    state_count: usize,
    #[serde(default)]
    defaults: BTreeMap<String, String>,
    #[serde(default)]
    properties: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    #[serde(default)]
    implicit_air: Option<serde_json::Value>,
}
#[derive(Clone)]
struct Face {
    side: u8,
    texture: u32,
    uv: [f32; 4],
    tint: i16,
    rotation: u16,
}
struct Element {
    from: [f32; 3],
    to: [f32; 3],
    faces: Vec<Face>,
    rotation: Option<ElementRotation>,
}
struct ElementRotation {
    kind: u8,
    origin: [f32; 3],
    axis: u8,
    angle: f32,
    rescale: bool,
    vector: [f32; 3],
}
fn rotate(point: &mut [f32; 3], axis: u8, angle: f32) {
    let (s, c) = angle.to_radians().sin_cos();
    let (a, b) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let (x, y) = (point[a], point[b]);
    point[a] = c * x - s * y;
    point[b] = s * x + c * y;
}
fn transform(mut point: [f32; 3], element: &Element, variant: &Variant) -> [f32; 3] {
    if let Some(rotation) = &element.rotation {
        for (axis, value) in point.iter_mut().enumerate() {
            *value -= rotation.origin[axis];
        }
        if rotation.kind == 0 {
            rotate(&mut point, rotation.axis, rotation.angle);
            if rotation.rescale {
                let scale = rotation.angle.to_radians().cos().abs().max(0.0001).recip();
                for (axis, value) in point.iter_mut().enumerate() {
                    if axis != usize::from(rotation.axis) {
                        *value *= scale;
                    }
                }
            }
        } else {
            for axis in 0..3 {
                rotate(&mut point, axis, rotation.vector[usize::from(axis)]);
            }
        }
        for (axis, value) in point.iter_mut().enumerate() {
            *value += rotation.origin[axis];
        }
    }
    for value in &mut point {
        *value -= 8.0;
    }
    rotate(&mut point, 0, -f32::from(variant.x) * 90.0);
    rotate(&mut point, 1, -f32::from(variant.y) * 90.0);
    for value in &mut point {
        *value += 8.0;
    }
    point
}
fn face_plane(element: &Element, variant: &Variant, side: u8, flat: bool) -> [[f32; 2]; 4] {
    let (a, b, depth, value) = match side {
        0 => (0, 2, 1, element.from[1]),
        1 => (0, 2, 1, element.to[1]),
        2 => (0, 1, 2, element.from[2]),
        3 => (0, 1, 2, element.to[2]),
        4 => (2, 1, 0, element.from[0]),
        _ => (2, 1, 0, element.to[0]),
    };
    let mut out = [[0.0; 2]; 4];
    for (index, (u, v)) in [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        let mut point = [0.0; 3];
        point[depth] = value;
        point[a] = element.from[a] + (element.to[a] - element.from[a]) * u;
        point[b] = element.from[b] + (element.to[b] - element.from[b]) * v;
        out[index] = if flat {
            [point[a] * 2.0, 32.0 - point[b] * 2.0]
        } else {
            let p = transform(point, element, variant);
            [p[0] * 2.0, 32.0 - p[1] * 2.0]
        };
    }
    out
}
fn plane_area(quad: &[[f32; 2]; 4]) -> f32 {
    ((quad[1][0] - quad[0][0]) * (quad[3][1] - quad[0][1])
        - (quad[1][1] - quad[0][1]) * (quad[3][0] - quad[0][0]))
        .abs()
}
fn select_plane<'a>(element: &'a Element, variant: &Variant) -> Option<(&'a Face, [[f32; 2]; 4])> {
    let mut selected = None;
    let mut largest = 0.0;
    for face in &element.faces {
        let quad = face_plane(element, variant, face.side, false);
        let area = plane_area(&quad);
        let mut normal = match face.side {
            0 => [0.0, -1.0, 0.0],
            1 => [0.0, 1.0, 0.0],
            2 => [0.0, 0.0, -1.0],
            3 => [0.0, 0.0, 1.0],
            4 => [-1.0, 0.0, 0.0],
            _ => [1.0, 0.0, 0.0],
        };
        if let Some(rotation) = &element.rotation {
            if rotation.kind == 0 {
                rotate(&mut normal, rotation.axis, rotation.angle);
            } else {
                for axis in 0..3 {
                    rotate(&mut normal, axis, rotation.vector[usize::from(axis)]);
                }
            }
        }
        rotate(&mut normal, 0, -f32::from(variant.x) * 90.0);
        rotate(&mut normal, 1, -f32::from(variant.y) * 90.0);
        if normal[2] < -0.0001 && area > largest {
            selected = Some((face, quad));
            largest = area;
        }
    }
    if selected.is_none() {
        for face in &element.faces {
            let quad = face_plane(element, variant, face.side, true);
            let area = plane_area(&quad);
            if area > largest {
                selected = Some((face, quad));
                largest = area;
            }
        }
    }
    selected
}
struct Variant {
    condition: String,
    model: usize,
    x: u8,
    y: u8,
}
struct ResourceBlock {
    variants: Vec<Variant>,
    multipart: Vec<Variant>,
}
struct ResourceVersion {
    textures: BTreeMap<String, usize>,
    blocks: BTreeMap<String, ResourceBlock>,
    names: BTreeMap<String, String>,
}
struct Texture {
    width: u16,
    height: u16,
    offset: usize,
    len: usize,
}
pub struct BlockDatabase {
    supplement: Supplement,
    strings: Vec<String>,
    textures: Vec<Texture>,
    pixels: Vec<u8>,
    models: Vec<Vec<Element>>,
    resources: BTreeMap<String, ResourceVersion>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).context("数据库读取溢出")?;
        let out = self.bytes.get(self.pos..end).context("方块数据库截断")?;
        self.pos = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn float(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn vector(&mut self) -> Result<[f32; 3]> {
        Ok([self.float()?, self.float()?, self.float()?])
    }
    fn count(&mut self, max: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        if n > max {
            bail!("方块数据库计数异常")
        }
        Ok(n)
    }
    fn string(&mut self) -> Result<String> {
        let tail = self.bytes.get(self.pos..).context("方块数据库字符串截断")?;
        let len = tail
            .iter()
            .take(65537)
            .position(|b| *b == 0)
            .context("方块数据库字符串缺少结束符")?;
        let value = std::str::from_utf8(self.take(len)?)?.to_owned();
        self.take(1)?;
        Ok(value)
    }
}
fn inflate(bytes: &[u8], limit: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    ZlibDecoder::new(bytes)
        .take(limit + 1)
        .read_to_end(&mut out)?;
    if out.len() as u64 > limit {
        bail!("方块数据库解压长度异常")
    }
    Ok(out)
}
fn text(strings: &[String], index: u32) -> Result<String> {
    Ok(strings
        .get(index as usize)
        .context("方块数据库字符串索引异常")?
        .clone())
}
fn variants(r: &mut Reader<'_>, strings: &[String]) -> Result<Vec<Variant>> {
    let count = r.u16()?;
    let mut values = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let condition = text(strings, r.u32()?)?;
        let model = r.u32()? as usize;
        let x = r.u8()?;
        let y = r.u8()?;
        r.u8()?;
        values.push(Variant {
            condition,
            model,
            x,
            y,
        });
    }
    Ok(values)
}
impl BlockDatabase {
    fn load() -> Result<Self> {
        let bytes = inflate(
            include_bytes!("../../../data/blockdb/blocks.dat.z"),
            64 * 1024 * 1024,
        )?;
        let mut r = Reader {
            bytes: &bytes,
            pos: 0,
        };
        if r.take(8)? != b"PMBLKDB\0" || r.u32()? != 3 {
            bail!("方块数据库格式不匹配")
        }
        let version_count = r.count(100000)?;
        let texture_count = r.count(2000000)?;
        let model_count = r.count(5000000)?;
        let lang_count = r.count(100000)?;
        let count = r.count(20000000)?;
        let mut strings = Vec::with_capacity(count);
        for _ in 0..count {
            strings.push(r.string()?);
        }
        if r.count(100000)? != lang_count {
            bail!("方块数据库语言计数不匹配")
        }
        let mut languages = Vec::new();
        for _ in 0..lang_count {
            let count = r.count(20000000)?;
            let mut names = BTreeMap::new();
            for _ in 0..count {
                let name = text(&strings, r.u32()?)?;
                let _en = text(&strings, r.u32()?)?;
                let zh = text(&strings, r.u32()?)?;
                if !zh.is_empty() {
                    names.insert(name, zh);
                }
            }
            languages.push(names);
        }
        if r.count(2000000)? != texture_count {
            bail!("方块数据库贴图计数不匹配")
        }
        let mut textures = Vec::new();
        for _ in 0..texture_count {
            let width = r.u16()?;
            let height = r.u16()?;
            let offset = r.u32()? as usize;
            let len = r.u32()? as usize;
            r.u8()?;
            textures.push(Texture {
                width,
                height,
                offset,
                len,
            });
        }
        let raw_size = r.u32()? as usize;
        let compressed = r.u32()? as usize;
        let pixels = inflate(r.take(compressed)?, 16 * 1024 * 1024)?;
        if raw_size != pixels.len() {
            bail!("方块数据库贴图长度不匹配")
        }
        for t in &textures {
            if t.offset
                .checked_add(t.len)
                .is_none_or(|end| end > pixels.len())
                || t.len != usize::from(t.width) * usize::from(t.height) * 4
            {
                bail!("方块数据库贴图范围不匹配")
            }
        }
        if r.count(5000000)? != model_count {
            bail!("方块数据库模型计数不匹配")
        }
        let mut models = Vec::new();
        for _ in 0..model_count {
            r.u8()?;
            let count = r.u16()?;
            let mut elements = Vec::new();
            for _ in 0..count {
                let from = r.vector()?;
                let to = r.vector()?;
                let rotation = if r.u8()? != 0 {
                    Some(ElementRotation {
                        kind: r.u8()?,
                        origin: r.vector()?,
                        axis: r.u8()?,
                        angle: r.float()?,
                        rescale: r.u8()? != 0,
                        vector: r.vector()?,
                    })
                } else {
                    None
                };
                let mask = r.u8()?;
                let mut faces = Vec::new();
                for side in 0..6 {
                    if mask & (1 << side) != 0 {
                        let texture = r.u32()?;
                        let uv = [r.float()?, r.float()?, r.float()?, r.float()?];
                        r.u8()?;
                        let tint = r.u16()? as i16;
                        let rotation = r.u16()?;
                        faces.push(Face {
                            side,
                            texture,
                            uv,
                            tint,
                            rotation,
                        });
                    }
                }
                elements.push(Element {
                    from,
                    to,
                    faces,
                    rotation,
                });
            }
            models.push(elements);
        }
        if r.count(100000)? != version_count {
            bail!("方块数据库版本计数不匹配")
        }
        let mut resources = BTreeMap::new();
        for _ in 0..version_count {
            let name = text(&strings, r.u32()?)?;
            r.u32()?;
            let language = r.u32()? as usize;
            let count = r.count(2000000)?;
            let mut texture_map = BTreeMap::new();
            for _ in 0..count {
                texture_map.insert(text(&strings, r.u32()?)?, r.u32()? as usize);
            }
            let count = r.count(500000)?;
            let mut blocks = BTreeMap::new();
            for _ in 0..count {
                let id = text(&strings, r.u32()?)?;
                let entries = variants(&mut r, &strings)?;
                let multipart = variants(&mut r, &strings)?;
                blocks.insert(
                    id,
                    ResourceBlock {
                        variants: entries,
                        multipart,
                    },
                );
            }
            resources.insert(
                name,
                ResourceVersion {
                    textures: texture_map,
                    blocks,
                    names: languages
                        .get(language)
                        .context("方块数据库语言索引异常")?
                        .clone(),
                },
            );
        }
        if r.take(8)? != b"NOVAST04" {
            bail!("方块数据库缺少精确注册表")
        }
        let len = r.count(32 * 1024 * 1024)?;
        let supplement: Supplement =
            serde_json::from_slice(&inflate(r.take(len)?, 64 * 1024 * 1024)?)?;
        if supplement.schema != 4 || r.pos != bytes.len() {
            bail!("方块数据库注册表格式不匹配")
        }
        Ok(Self {
            supplement,
            strings,
            textures,
            pixels,
            models,
            resources,
        })
    }
    pub fn version(&self, version: &str) -> Result<&Version> {
        self.supplement
            .versions
            .get(version)
            .context("该版本缺少真实方块数据库")
    }
    pub fn version_for_data_version(&self, version: i32) -> Option<&str> {
        self.supplement
            .versions
            .iter()
            .find(|(_, v)| v.data_version == version)
            .map(|(name, _)| name.as_str())
    }
    pub(crate) fn registry(&self, version: &str) -> Result<&Registry> {
        self.supplement
            .registries
            .get(&self.version(version)?.registry)
            .context("方块注册表缺失")
    }
    pub(crate) fn registry_by_data_version(&self, version: i32) -> Result<&Registry> {
        let (_, entry) = self
            .supplement
            .versions
            .iter()
            .find(|(_, v)| v.data_version == version)
            .context("数据版本缺少真实注册表")?;
        self.supplement
            .registries
            .get(&entry.registry)
            .context("方块注册表缺失")
    }
    pub fn name(&self, version: &str, id: &str) -> String {
        self.version(version)
            .ok()
            .and_then(|v| self.resources.get(&v.resource_version))
            .and_then(|v| {
                v.names
                    .get(id)
                    .or_else(|| v.names.get(id.strip_prefix("minecraft:").unwrap_or(id)))
            })
            .or_else(|| {
                self.resources.get("26.3").and_then(|v| {
                    v.names
                        .get(id)
                        .or_else(|| v.names.get(id.strip_prefix("minecraft:").unwrap_or(id)))
                })
            })
            .cloned()
            .unwrap_or_else(|| {
                self.registry(version)
                    .ok()
                    .and_then(|registry| registry.blocks.get(id))
                    .map(|block| block.en.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| {
                        id.strip_prefix("minecraft:")
                            .unwrap_or(id)
                            .replace('_', " ")
                    })
            })
    }
    pub(crate) fn category(&self, name: &str) -> Option<&str> {
        self.registry("26.3")
            .ok()?
            .blocks
            .get(name)
            .map(|block| block.category.as_str())
            .filter(|value| !value.is_empty())
    }
    pub fn query(
        &self,
        version: Option<&str>,
        query: &str,
        state: Option<&str>,
    ) -> Result<serde_json::Value> {
        let Some(version) = version else {
            let versions:Vec<_>=self.supplement.versions.iter().map(|(name,v)|serde_json::json!({"version":name,"dataVersion":v.data_version,"blockCount":v.block_count,"stateCount":v.state_count,"legacy":v.legacy,"outputExtension":v.output_extension})).collect();
            let colors:Vec<_>=self.supplement.map_color_versions.iter().map(|(name,v)|serde_json::json!({"version":name,"dataVersion":v.data_version,"blockCount":v.block_count,"stateCount":v.state_count,"colorOnly":!self.supplement.versions.contains_key(name)})).collect();
            return Ok(serde_json::json!({"versions":versions,"mapColorVersions":colors}));
        };
        if let Some(state) = state {
            return self.query_states(version, &[state.to_owned()])?["states"].get(state).cloned().context("方块颜色结果缺失");
        }
        let registry = self.registry(version)?;
        let query = query.to_lowercase();
        let mut blocks = Vec::new();
        for (id, block) in &registry.blocks {
            let name = self.name(version, id);
            if query.is_empty()
                || id.contains(&query)
                || name.to_lowercase().contains(&query)
                || block.en.to_lowercase().contains(&query)
            {
                blocks.push(serde_json::json!({"id":id,"name":name,"defaultState":block.default,"properties":block.properties,"stateCount":block.state_count,"iconState":block.default}));
            }
        }
        Ok(serde_json::json!({"version":version,"blocks":blocks}))
    }
    pub fn query_states(&self, version: &str, states: &[String]) -> Result<serde_json::Value> {
        if !self.supplement.map_color_versions.contains_key(version) { self.version(version)?; }
        let mut values = BTreeMap::new();
        for state in states {
            if !values.contains_key(state) {
                // A missing side model must not discard a independently verified
                // map color (technical/fluid blocks do not have cube models).
                let mut value=self.side(version,state).unwrap_or_else(|error|serde_json::json!({"state":state,"name":crate::version_conversion::parse_key(state).ok().map(|(id,_)|self.name(version,&id)),"sideTextureDataUrl":null,"projection":"missing","missing":true,"error":error.to_string()}));
                let color = self.map_color(version, state);
                value["mapColorStatus"] = serde_json::json!(match &color {
                    Some(color) if color["id"] == 0 => "none",
                    Some(_) => "mapped",
                    None => "missing",
                });
                value["mapColor"] = color.unwrap_or(serde_json::Value::Null);
                values.insert(state.clone(), value);
            }
        }
        Ok(serde_json::json!({"version":version,"states":values}))
    }
    fn map_color(&self, version: &str, state: &str) -> Option<serde_json::Value> {
        if let Some(source) = self.supplement.numeric_map_color_sources.get(version) {
            let table = self.supplement.numeric_map_colors.get(source)?;
            if let Some(color) = table.get(state) { return Some(color.clone()); }
            if state.split_once(':').is_some_and(|(id, meta)|id=="0" && meta.parse::<u8>().is_ok_and(|meta|meta<16)) {
                return self.supplement.map_color_versions.get(version)?.implicit_air.clone();
            }
            return None;
        }
        let source = self.supplement.map_color_sources.get(version).map(String::as_str).unwrap_or(version);
        let colors = self.supplement.map_colors.get(source)?;
        let (id, mut properties) = crate::version_conversion::parse_key(state).ok()?;
        let registry = self.registry(version).ok();
        let block = registry.and_then(|registry|registry.blocks.get(&id));
        // NBT may omit default values and property iteration order is not stable.
        // Complete only from this exact target registry, never from newest data.
        let default = self.supplement.map_color_versions.get(version).and_then(|version|version.defaults.get(&id)).map(String::as_str).or_else(||block.map(|block|block.default.as_str()));
        if let Some(default) = default {
            let (_, defaults) = crate::version_conversion::parse_key(default).ok()?;
            for (name, value) in defaults { properties.entry(name).or_insert(value); }
        }
        let key = if properties.is_empty() { id.clone() } else {
            format!("{id}[{}]", properties.iter().map(|(name,value)|format!("{name}={value}")).collect::<Vec<_>>().join(","))
        };
        if let Some(color) = colors.get(&key) { return Some(color.clone()); }
        // Import stores a block ID only after comparing *every* runtime state
        // and proving one identical color. Registry validation is still needed
        // so an invalid state cannot inherit a valid block's color.
        let validated = block.is_some_and(|block|block.accepts(&key, &properties)) || self.supplement.map_color_versions.get(version)
            .and_then(|version|version.properties.get(&id)).is_some_and(|definition|definition.len()==properties.len() && definition.iter().all(|(name,values)|properties.get(name).is_some_and(|value|values.contains(value))));
        if validated {
            if let Some(color) = colors.get(&id) { return Some(color.clone()); }
        }
        // Legacy packed states use their recorded numeric identity: only an
        // observed alias with an equal numeric state may supply its real color.
        for (alias, mapped) in registry.into_iter().flat_map(|registry|&registry.legacy_state_aliases) {
            if mapped.contains(&key) {
                if let Some(color) = colors.get(alias).or_else(||colors.get(alias.split('[').next()?)) { return Some(color.clone()); }
            }
        }
        None
    }
fn face_tint(&self, version: &str, state: &str, index: i16) -> Result<u32> {
        if index == -1 { return Ok(0xffff_ffff); }
        if index < -1 { bail!("无效侧纹理染色索引：{index}"); }
        self.version(version)?;
        let (id, properties) = crate::version_conversion::parse_key(state)?;
        let state_key = if properties.is_empty() { id } else {
            format!("{id}[{}]", properties.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(","))
        };
        let data = plains_tint_metadata()?;
        let table = data.versions.get(version).and_then(|key| data.tables.get(key))
            .with_context(|| format!("缺少版本 {version} 的实际侧纹理染色数据"))?;
        table.get(&format!("{state_key}|{index}")).copied()
            .with_context(|| format!("缺少实际侧纹理染色结果：{version} {state_key} tint={index}"))
    }
    fn side(&self, version: &str, state: &str) -> Result<serde_json::Value> {
        let (id, properties) = crate::version_conversion::parse_key(state)?;
        let special_key = if [
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:ender_chest",
        ]
        .contains(&id.as_str())
        {
            format!(
                "{id}[{}]",
                properties
                    .get("type")
                    .map(String::as_str)
                    .unwrap_or("single")
            )
        } else {
            id.clone()
        };
        if let Some(plane) = self
            .supplement
            .special_planes
            .get(version)
            .and_then(|planes| planes.get(&special_key))
        {
            return Ok(
                serde_json::json!({"state":state,"name":self.name(version,&id),"sideTextureDataUrl":format!("data:image/png;base64,{}",plane["png"].as_str().context("特殊模型平面数据损坏")?),"mapColor":self.map_color(version,state),"tintIndices":[],"faces":[{"texture":plane["texture"],"rule":plane["rule"]}],"projection":"model-side","biomeProfile":"minecraft:plains","tintProfileVersion":version}),
            );
        }
        let resource = self
            .resources
            .get(&self.version(version)?.resource_version)
            .context("该版本没有模型资源")?;
        let block = resource
            .blocks
            .get(&id)
            .or_else(|| {
                resource
                    .blocks
                    .get(id.strip_prefix("minecraft:").unwrap_or(&id))
            })
            .context("方块没有平面模型")?;
        let matches = |condition: &str| {
            condition.is_empty()
                || condition.split('|').any(|alternative| {
                    alternative.split(',').all(|pair| {
                        pair.split_once('=').is_some_and(|(k, v)| {
                            properties.get(k).is_some_and(|value| value == v)
                        })
                    })
                })
        };
        let mut selected: Vec<&Variant> = block
            .variants
            .iter()
            .filter(|v| matches(&v.condition))
            .take(1)
            .collect();
        selected.extend(block.multipart.iter().filter(|v| matches(&v.condition)));
        let mut image = RgbaImage::new(32, 32);
        let mut tints = BTreeSet::new();
        let mut used = Vec::new();
        for variant in selected {
            let Some(model) = self.models.get(variant.model) else {
                continue;
            };
            for element in model {
                let Some((face, quad)) = select_plane(element, variant) else {
                    continue;
                };
                let Some(path) = self.strings.get(face.texture as usize) else {
                    continue;
                };
                let Some(index) = resource.textures.get(path) else {
                    continue;
                };
                let Some(texture) = self.textures.get(*index) else {
                    continue;
                };
                let pixels = &self.pixels[texture.offset..texture.offset + texture.len];
                let tint = self.face_tint(version, state, face.tint)?;
                let x0 = quad
                    .iter()
                    .map(|v| v[0])
                    .fold(f32::INFINITY, f32::min)
                    .floor()
                    .clamp(0.0, 32.0) as u32;
                let x1 = quad
                    .iter()
                    .map(|v| v[0])
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil()
                    .clamp(0.0, 32.0) as u32;
                let y0 = quad
                    .iter()
                    .map(|v| v[1])
                    .fold(f32::INFINITY, f32::min)
                    .floor()
                    .clamp(0.0, 32.0) as u32;
                let y1 = quad
                    .iter()
                    .map(|v| v[1])
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil()
                    .clamp(0.0, 32.0) as u32;
                let e1 = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
                let e3 = [quad[3][0] - quad[0][0], quad[3][1] - quad[0][1]];
                let determinant = e1[0] * e3[1] - e1[1] * e3[0];
                if !determinant.is_finite() || determinant.abs() < 0.0001 {
                    continue;
                }
                for y in y0..y1 {
                    for x in x0..x1 {
                        let point = [x as f32 + 0.5 - quad[0][0], y as f32 + 0.5 - quad[0][1]];
                        let (mut u, mut v) = (
                            (point[0] * e3[1] - point[1] * e3[0]) / determinant,
                            (e1[0] * point[1] - e1[1] * point[0]) / determinant,
                        );
                        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                            continue;
                        }
                        for _ in 0..(face.rotation / 90) % 4 {
                            (u, v) = (v, 1.0 - u);
                        }
                        let tx = ((face.uv[0] + u * (face.uv[2] - face.uv[0]))
                            * f32::from(texture.width)
                            / 16.0)
                            .clamp(0.0, f32::from(texture.width) - 1.0)
                            as usize;
                        let ty = ((face.uv[1] + v * (face.uv[3] - face.uv[1]))
                            * f32::from(texture.height)
                            / 16.0)
                            .clamp(0.0, f32::from(texture.height) - 1.0)
                            as usize;
                        let offset = (ty * usize::from(texture.width) + tx) * 4;
                        let src = &pixels[offset..offset + 4];
                        let dst = image.get_pixel_mut(x, y);
                        let alpha = f32::from(src[3]) / 255.0;
                        let out = alpha + f32::from(dst[3]) / 255.0 * (1.0 - alpha);
                        if out > 0.0 {
                            for c in 0..3 {
                                dst[c] = (((f32::from(src[c])
                                    * f32::from(((tint >> ((2 - c) * 8)) & 255) as u8)
                                    / 255.0)
                                    * alpha
                                    + f32::from(dst[c]) * f32::from(dst[3]) / 255.0
                                        * (1.0 - alpha))
                                    / out)
                                    .round() as u8;
                            }
                            dst[3] = (out * 255.0).round() as u8;
                        }
                    }
                }
                if face.tint >= 0 {
                    tints.insert(face.tint);
                }
                used.push(serde_json::json!({"texture":path,"tintIndex":face.tint,"side":face.side,"rotationX":u16::from(variant.x)*90,"rotationY":u16::from(variant.y)*90}));
            }
        }
        if used.is_empty() {
            return Ok(
                serde_json::json!({"state":state,"name":self.name(version,&id),"sideTextureDataUrl":null,"mapColor":self.map_color(version,state),"tintIndices":[],"faces":[],"projection":"no-side-model"}),
            );
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(
            image.as_raw(),
            32,
            32,
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(
            serde_json::json!({"state":state,"name":self.name(version,&id),"sideTextureDataUrl":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png)),"mapColor":self.map_color(version,state),"tintIndices":tints,"faces":used,"projection":"model-side","biomeProfile":"minecraft:plains","tintProfileVersion":if self.supplement.plains.contains_key(version){version}else{"26.3"}}),
        )
    }
}

/// Concrete Plains/origin color outputs for the ordinary block-data browser.
/// This is a metadata lookup, not a renderer tint solver or a rule fallback.
#[derive(Deserialize)]
struct PlainsTintMetadata {
    schema: u32,
    #[serde(rename = "blocksDatSha256")]
    blocks_dat_sha256: String,
    purpose: String,
    versions: BTreeMap<String, String>,
    tables: BTreeMap<String, BTreeMap<String, u32>>,
}

fn plains_tint_metadata() -> Result<&'static PlainsTintMetadata> {
    use sha2::{Digest, Sha256};
    static DATA: OnceLock<Result<PlainsTintMetadata, String>> = OnceLock::new();
    DATA.get_or_init(|| (|| -> Result<PlainsTintMetadata> {
        let data: PlainsTintMetadata = serde_json::from_slice(&inflate(
            include_bytes!("../../../data/blockdb/plains-tints.dat.z"), 16 * 1024 * 1024,
        )?)?;
        let blocks_sha = format!("{:x}", Sha256::digest(include_bytes!("../../../data/blockdb/blocks.dat.z")));
        if data.schema != 1 || data.purpose != "block-database-side-plains-origin"
            || data.blocks_dat_sha256 != blocks_sha || data.versions.is_empty()
            || data.versions.values().any(|key| !data.tables.contains_key(key)) {
            bail!("侧纹理染色元数据版本或原方块数据库身份不匹配");
        }
        Ok(data)
    })().map_err(|error| format!("{error:#}")))
        .as_ref().map_err(|error| anyhow::anyhow!("读取侧纹理染色元数据：{error}"))
}

pub fn database() -> Result<&'static BlockDatabase> {
    static DB: OnceLock<Result<BlockDatabase, String>> = OnceLock::new();
    DB.get_or_init(|| BlockDatabase::load().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| anyhow::anyhow!("读取多版本方块数据库：{e}"))
}
pub fn run_cli(args: &[String]) -> Result<()> {
    let mut version = None;
    let mut query = "";
    let mut state = None;
    let mut states_file = None;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--versions" => {}
            "--version" => version = Some(iter.next().context("missing --version value")?.as_str()),
            "--query" => query = iter.next().context("missing --query value")?,
            "--state" => state = Some(iter.next().context("missing --state value")?.as_str()),
            "--states-json" => {
                states_file = Some(iter.next().context("missing --states-json value")?.as_str())
            }
            _ => bail!("unknown block-db argument: {flag}"),
        }
    }
    let result = if let Some(path) = states_file {
        let states: Vec<String> = serde_json::from_slice(&std::fs::read(path)?)?;
        database()?.query_states(version.context("states query requires --version")?, &states)?
    } else {
        database()?.query(version, query, state)?
    };
    println!("{}", result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_pack_and_actual_registries_are_complete() {
        let db = database().unwrap();
        assert_eq!(db.resources.len(), 49);
        assert_eq!(db.textures.len(), 1685);
        assert_eq!(db.models.len(), 2927);
        assert_eq!(db.version("26.3").unwrap().state_count, 35723);
        assert_eq!(db.version("26.2").unwrap().state_count, 32366);
        assert!(db.version("1.12.2").unwrap().legacy);
        assert!(db.version("1.0").is_err());
    }
    #[test]
    fn names_and_target_filter_use_one_pack() {
        let db = database().unwrap();
        assert_eq!(db.name("1.20.1", "minecraft:oak_log"), "橡木原木");
        let q = db.query(Some("1.20.1"), "pale_oak", None).unwrap();
        assert!(q["blocks"].as_array().unwrap().is_empty());
    }
    #[test]
    fn map_colors_use_exact_version_state_and_survive_missing_side_models() {
        let db = database().unwrap();
        for version in db.supplement.map_color_sources.keys() {
            // The actual 1.8 runtime still uses historical DIRT 0xb76a2f;
            // later runtime tables use 0x976d4d. Never rewrite old game data
            // to the current palette just to make the versions look uniform.
            let dirt_rgb = if version == "1.8" { 12020271 } else { 9923917 };
            assert_eq!(db.map_color(version, "minecraft:dirt").unwrap()["rgb"], dirt_rgb, "{version}");
            assert!(db.map_color(version, "minecraft:oak_log[axis=invalid]").is_none());
        }
        let result = db.query_states("1.21.1", &["minecraft:water[level=0]".into(), "minecraft:structure_void".into()]).unwrap();
        assert_eq!(result["states"]["minecraft:water[level=0]"]["mapColor"]["id"], 12);
        assert_eq!(result["states"]["minecraft:structure_void"]["mapColorStatus"], "none");
        let a = db.map_color("1.21.1", "minecraft:oak_stairs[half=bottom,facing=north,shape=straight,waterlogged=false]");
        let b = db.map_color("1.21.1", "minecraft:oak_stairs[waterlogged=false,shape=straight,facing=north,half=bottom]");
        assert!(a.is_some());
        assert_eq!(a, b);
    }
    #[test]
    fn early_numeric_colors_are_explicit_and_do_not_create_fake_conversion_targets() {
        let db = database().unwrap();
        assert_eq!(db.supplement.map_color_versions.len(), 103);
        assert!(db.supplement.map_color_versions["1.0"].data_version.is_none());
        assert!(db.version("1.0").is_err());
        let result = db.query_states("1.0", &["1:0".into(), "3:0".into(), "0:0".into(), "9999:0".into()]).unwrap();
        assert_eq!(result["states"]["1:0"]["mapColor"]["rgb"], 7368816);
        assert_eq!(result["states"]["3:0"]["mapColor"]["rgb"], 12020271);
        assert_eq!(result["states"]["0:0"]["mapColorStatus"], "none");
        assert_eq!(result["states"]["9999:0"]["mapColorStatus"], "missing");
        let query = db.query(None, "", None).unwrap();
        assert_eq!(query["mapColorVersions"].as_array().unwrap().len(), 103);
        assert!(!query["versions"].as_array().unwrap().iter().any(|row|row["version"]=="1.0"));
    }
    #[test]
    fn representative_side_is_actual_texture_not_item_icon() {
        let value = database()
            .unwrap()
            .side("26.3", "minecraft:oak_log[axis=y]")
            .unwrap();
        assert_eq!(value["projection"], "model-side");
        assert!(
            value["sideTextureDataUrl"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(
            value["faces"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["texture"] == "block/oak_log")
        );
    }
    #[test]
    fn builtin_chest_and_skull_use_actual_flat_entity_atlas_and_batch_has_no_process_per_state() {
        let db = database().unwrap();
        let states = vec![
            "minecraft:chest[facing=north,type=single,waterlogged=false]".into(),
            "minecraft:skeleton_skull[rotation=0]".into(),
            "mod:missing".into(),
        ];
        let result = db.query_states("26.3", &states).unwrap();
        assert_eq!(result["states"][&states[0]]["projection"], "model-side");
        assert_eq!(
            result["states"][&states[0]]["faces"][0]["rule"],
            "ChestModel SOUTH lid/bottom/lock"
        );
        assert_eq!(result["states"][&states[1]]["projection"], "model-side");
        assert_eq!(result["states"][&states[2]]["missing"], true);
        assert!(result["states"][&states[2]]["sideTextureDataUrl"].is_null());
    }
    fn pixels(state: &str) -> RgbaImage {
        let result = database().unwrap().side("26.3", state).unwrap();
        let url = result["sideTextureDataUrl"].as_str().expect("actual plane");
        let png = base64::engine::general_purpose::STANDARD
            .decode(url.split_once(',').unwrap().1)
            .unwrap();
        image::load_from_memory(&png).unwrap().to_rgba8()
    }
    fn opaque_bounds(image: &RgbaImage) -> (u32, u32) {
        let mut bounds = [32, 32, 0, 0];
        for (x, y, pixel) in image.enumerate_pixels() {
            if pixel[3] != 0 {
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x);
                bounds[3] = bounds[3].max(y);
            }
        }
        assert!(bounds[0] <= bounds[2] && bounds[1] <= bounds[3]);
        (bounds[2] - bounds[0] + 1, bounds[3] - bounds[1] + 1)
    }
    #[test]
    fn variant_and_element_rotation_change_actual_planar_shape_and_flat_specials_keep_alpha() {
        let vertical = opaque_bounds(&pixels("minecraft:end_rod[facing=up]"));
        let horizontal = opaque_bounds(&pixels("minecraft:end_rod[facing=east]"));
        assert!(vertical.1 > vertical.0);
        assert!(horizontal.0 > horizontal.1);
        assert!(pixels("minecraft:poppy").pixels().any(|p| p[3] == 0));
        assert!(
            pixels("minecraft:wall_torch[facing=north]")
                .pixels()
                .any(|p| p[3] != 0)
        );
        let low =
            pixels("minecraft:redstone_wire[east=none,north=none,power=0,south=none,west=none]");
        let high =
            pixels("minecraft:redstone_wire[east=none,north=none,power=15,south=none,west=none]");
        assert!(
            high.pixels().map(|p| u64::from(p[0])).sum::<u64>()
                > low.pixels().map(|p| u64::from(p[0])).sum::<u64>()
        );
        assert!(high.pixels().any(|p| p[3] == 0));
    }
}

#[cfg(test)]
mod concrete_tint_metadata_tests {
    use super::*;
    #[test]
    fn metadata_is_bound_to_the_real_block_database_and_known_versions() {
        let data = plains_tint_metadata().unwrap();
        let db = database().unwrap();
        assert_eq!(data.versions.len(), db.supplement.versions.len());
        for version in db.supplement.versions.keys() { assert!(data.versions.contains_key(version)); }
    }
    #[test]
    fn side_tints_use_canonical_properties_and_never_invent_missing_positive_results() {
        let db = database().unwrap();
        assert_eq!(db.face_tint("26.3", "minecraft:stone", -1).unwrap(), 0xffff_ffff);
        let a = db.face_tint("26.3", "minecraft:redstone_wire[power=15,east=none,north=none,south=none,west=none]", 0).unwrap();
        let b = db.face_tint("26.3", "minecraft:redstone_wire[west=none,south=none,power=15,north=none,east=none]", 0).unwrap();
        assert_eq!(a, b);
        assert!(db.face_tint("26.3", "minecraft:stone", 0).is_err());
        assert!(db.face_tint("26.3", "minecraft:redstone_wire[power=15]", 127).is_err());
        assert!(db.face_tint("26.3", "minecraft:stone", -2).is_err());
    }
}
