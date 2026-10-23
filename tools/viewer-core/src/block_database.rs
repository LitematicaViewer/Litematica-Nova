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
    sync::{Mutex, OnceLock},
};

/// The view plane used when a block state is rasterised for the layer canvas.
///
/// Minecraft's vertical axis is Y, so a top-down layer canvas must look at the
/// +Y face. The legacy `-Z` side plane is kept for callers that want a front
/// elevation, but it is not a substitute for the top view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    Top,
    Side,
}

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
fn face_plane(
    element: &Element,
    variant: &Variant,
    side: u8,
    plane: Plane,
    flat: bool,
) -> [[f32; 2]; 4] {
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
            // The fallback keeps the face's own axes so a model without the
            // requested facing still produces a usable texture.
            [point[a] * 2.0, 32.0 - point[b] * 2.0]
        } else {
            let p = transform(point, element, variant);
            match plane {
                // Minecraft's height axis is Y: looking down projects X/Z.
                Plane::Top => [p[0] * 2.0, 32.0 - p[2] * 2.0],
                // Front elevation keeps the historical X/Y projection.
                Plane::Side => [p[0] * 2.0, 32.0 - p[1] * 2.0],
            }
        };
    }
    out
}
fn plane_area(quad: &[[f32; 2]; 4]) -> f32 {
    ((quad[1][0] - quad[0][0]) * (quad[3][1] - quad[0][1])
        - (quad[1][1] - quad[0][1]) * (quad[3][0] - quad[0][0]))
        .abs()
}
fn select_plane<'a>(
    element: &'a Element,
    variant: &Variant,
    plane: Plane,
) -> Option<(&'a Face, [[f32; 2]; 4])> {
    let mut selected = None;
    let mut largest = 0.0;
    for face in &element.faces {
        let quad = face_plane(element, variant, face.side, plane, false);
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
        let faces_camera = match plane {
            Plane::Top => normal[1] > 0.0001,
            Plane::Side => normal[2] < -0.0001,
        };
        if faces_camera && area > largest {
            selected = Some((face, quad));
            largest = area;
        }
    }
    // A model can be rotated so the requested face normal is not present.
    // Prefer horizontal faces for the top view before accepting any face.
    if selected.is_none() && plane == Plane::Top {
        for face in element.faces.iter().filter(|face| face.side <= 1) {
            let quad = face_plane(element, variant, face.side, plane, true);
            let area = plane_area(&quad);
            if area > largest {
                selected = Some((face, quad));
                largest = area;
            }
        }
    }
    if selected.is_none() {
        largest = 0.0;
        for face in &element.faces {
            let quad = face_plane(element, variant, face.side, plane, true);
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
fn namespaced(value: &str) -> String {
    if value.contains(':') { value.to_owned() } else { format!("minecraft:{value}") }
}
pub(crate) fn selected_texture(context: &crate::game_data::GameDataContext, name: &str) -> Result<std::sync::Arc<RgbaImage>> {
    use std::sync::Arc;
    static TEXTURES: OnceLock<Mutex<BTreeMap<(String, String), Arc<RgbaImage>>>> = OnceLock::new();
    let key = (context.content_id.clone(), name.to_owned());
    let cache = TEXTURES.get_or_init(Default::default);
    if let Some(value) = cache.lock().map_err(|_| anyhow::anyhow!("selected texture cache lock poisoned"))?.get(&key).cloned() { return Ok(value); }
    let (namespace, local) = name.split_once(':').context("texture requires namespace")?;
    let path = format!("assets/{namespace}/textures/{local}.png");
    let bytes = crate::game_data::read_resource_from(context, &path)?.with_context(|| format!("selected texture missing: {path}"))?;
    let image = image::load_from_memory(&bytes)?.to_rgba8();
    // Preserve the actual first animation frame, not a stretched sprite strip.
    let mut width = image.width();
    let mut height = image.height();
    if let Some(metadata) = crate::game_data::read_resource_from(context, &format!("{path}.mcmeta"))? {
        let metadata: serde_json::Value = serde_json::from_slice(&metadata)?;
        if let Some(animation) = metadata.get("animation") {
            width = animation.get("width").and_then(serde_json::Value::as_u64).map(|v|v as u32).unwrap_or(image.width());
            height = animation.get("height").and_then(serde_json::Value::as_u64).map(|v|v as u32).unwrap_or(width.min(image.height()));
        }
    }
    if width == 0 || height == 0 || width > image.width() || height > image.height() { bail!("invalid selected animated texture frame: {path}"); }
    let image = Arc::new(image::imageops::crop_imm(&image, 0, 0, width, height).to_image());
    cache.lock().map_err(|_| anyhow::anyhow!("selected texture cache lock poisoned"))?.insert(key, image.clone());
    Ok(image)
}

pub(crate) fn resource_model(context: &crate::game_data::GameDataContext, name: &str, visiting: &mut BTreeSet<String>) -> Result<serde_json::Value> {
    let name = namespaced(name);
    if !visiting.insert(name.clone()) { bail!("selected model inheritance cycle: {name}"); }
    let (namespace, local) = name.split_once(':').context("model requires namespace")?;
    let path = format!("assets/{namespace}/models/{local}.json");
    let bytes = crate::game_data::read_resource_from(context, &path)?.with_context(|| format!("selected model missing: {path}"))?;
    let own: serde_json::Value = serde_json::from_slice(&bytes).with_context(||format!("invalid selected model: {path}"))?;
    let mut model = if let Some(parent) = own.get("parent").and_then(serde_json::Value::as_str).filter(|parent|!parent.starts_with("builtin/") && !parent.starts_with("minecraft:builtin/")) { resource_model(context, parent, visiting)? } else { serde_json::json!({}) };
    let object = own.as_object().context("selected model is not an object")?;
    for (key, value) in object {
        if key == "textures" {
            let textures = model.as_object_mut().context("model object missing")?.entry("textures").or_insert_with(||serde_json::json!({}));
            for (key, value) in value.as_object().context("model textures must be an object")? { textures.as_object_mut().context("model textures must be an object")?.insert(key.clone(),value.clone()); }
        } else { model.as_object_mut().context("model object missing")?.insert(key.clone(),value.clone()); }
    }
    visiting.remove(&name);
    Ok(model)
}
fn model_vector(value: Option<&serde_json::Value>, fallback: [f32;3]) -> Result<[f32;3]> {
    let Some(value) = value else { return Ok(fallback); };
    let values = value.as_array().filter(|values|values.len()==3).context("model vector must contain three values")?;
    Ok([values[0].as_f64().context("model coordinate missing")? as f32, values[1].as_f64().context("model coordinate missing")? as f32, values[2].as_f64().context("model coordinate missing")? as f32])
}
fn model_elements(model: &serde_json::Value) -> Result<(Vec<Element>, Vec<String>)> {
    let textures = model.get("textures").and_then(serde_json::Value::as_object);
    let mut strings = Vec::new();
    let mut elements = Vec::new();
    for raw in model.get("elements").and_then(serde_json::Value::as_array).into_iter().flatten() {
        let from = model_vector(raw.get("from"), [0.0;3])?;
        let to = model_vector(raw.get("to"), [16.0;3])?;
        let mut faces = Vec::new();
        for (direction, face) in raw.get("faces").and_then(serde_json::Value::as_object).into_iter().flatten() {
            let side = match direction.as_str() { "down"=>0,"up"=>1,"north"=>2,"south"=>3,"west"=>4,"east"=>5,_=>bail!("invalid selected model face: {direction}") };
            let mut texture = face.get("texture").and_then(serde_json::Value::as_str).context("model face texture missing")?.to_owned();
            let mut seen = BTreeSet::new();
            while let Some(variable) = texture.strip_prefix('#') {
                if !seen.insert(variable.to_owned()) { bail!("selected texture variable cycle"); }
                texture = textures.and_then(|textures| textures.get(variable)).and_then(serde_json::Value::as_str).with_context(||format!("selected texture variable missing: {variable}"))?.to_owned();
            }
            let texture = namespaced(&texture);
            let index = strings.len() as u32; strings.push(texture);
            let automatic = match side { 0=>[from[0],16.0-to[2],to[0],16.0-from[2]],1=>[from[0],from[2],to[0],to[2]],2=>[16.0-to[0],16.0-to[1],16.0-from[0],16.0-from[1]],3=>[from[0],16.0-to[1],to[0],16.0-from[1]],4=>[from[2],16.0-to[1],to[2],16.0-from[1]],_=>[16.0-to[2],16.0-to[1],16.0-from[2],16.0-from[1]] };
            let uv = if let Some(uv)=face.get("uv").and_then(serde_json::Value::as_array) { if uv.len()!=4 {bail!("model UV requires four values");} [uv[0].as_f64().context("UV missing")? as f32,uv[1].as_f64().context("UV missing")? as f32,uv[2].as_f64().context("UV missing")? as f32,uv[3].as_f64().context("UV missing")? as f32] } else {automatic};
            faces.push(Face { side,texture:index,uv,tint:face.get("tintindex").and_then(serde_json::Value::as_i64).unwrap_or(-1) as i16,rotation:face.get("rotation").and_then(serde_json::Value::as_u64).unwrap_or(0) as u16 });
        }
        let rotation = raw.get("rotation").map(|value| -> Result<ElementRotation> {
            let axis = match value.get("axis").and_then(serde_json::Value::as_str) {Some("x")=>0,Some("y")=>1,Some("z")=>2,_=>bail!("invalid model rotation axis")};
            Ok(ElementRotation { kind:0,origin:model_vector(value.get("origin"),[8.0;3])?,axis,angle:value.get("angle").and_then(serde_json::Value::as_f64).unwrap_or(0.0) as f32,rescale:value.get("rescale").and_then(serde_json::Value::as_bool).unwrap_or(false),vector:[0.0;3] })
        }).transpose()?;
        elements.push(Element {from,to,faces,rotation});
    }
    Ok((elements,strings))
}
fn model_condition(value: &serde_json::Value, properties: &BTreeMap<String,String>) -> bool {
    let Some(object) = value.as_object() else {return false;};
    object.iter().all(|(key,value)| {
        if key=="OR" { return value.as_array().is_some_and(|values|values.iter().any(|value|model_condition(value,properties))); }
        if key=="AND" { return value.as_array().is_some_and(|values|values.iter().all(|value|model_condition(value,properties))); }
        value.as_str().is_some_and(|expected|properties.get(key).is_some_and(|actual|expected.split('|').any(|value|value==actual)))
    })
}
type SelectedPlaneModels = Vec<(Variant, Vec<Element>, Vec<String>)>;
fn selected_plane_models(context: &crate::game_data::GameDataContext, id: &str, properties: &BTreeMap<String,String>) -> Result<std::sync::Arc<SelectedPlaneModels>> {
    use std::sync::Arc;
    static MODELS: OnceLock<Mutex<BTreeMap<(String,String), Arc<SelectedPlaneModels>>>> = OnceLock::new();
    let key = (context.content_id.clone(), format!("{id}:{}", serde_json::to_string(properties)?));
    let cache = MODELS.get_or_init(Default::default);
    if let Some(value) = cache.lock().map_err(|_| anyhow::anyhow!("selected model cache lock poisoned"))?.get(&key).cloned() { return Ok(value); }
    let (namespace,local)=id.split_once(':').context("block state requires namespace")?;
    let path=format!("assets/{namespace}/blockstates/{local}.json");
    let bytes=crate::game_data::read_resource_from(context,&path)?.with_context(||format!("selected blockstate missing: {path}"))?;
    let state:serde_json::Value=serde_json::from_slice(&bytes)?;
    let mut choices=Vec::new();
    if let Some(variants)=state.get("variants").and_then(serde_json::Value::as_object) {
        for (condition,value) in variants {
            if condition.is_empty() || condition.split(',').all(|part|part.split_once('=').is_some_and(|(key,expected)|properties.get(key).is_some_and(|actual|expected.split('|').any(|value|value==actual)))) {choices.push(value); break;}
        }
    }
    for part in state.get("multipart").and_then(serde_json::Value::as_array).into_iter().flatten() {
        if part.get("when").is_none_or(|condition|model_condition(condition,properties)) {choices.push(part.get("apply").context("multipart apply missing")?);}
    }
    let mut result=Vec::new();
    for value in choices {
        let value=if let Some(values)=value.as_array(){values.first().context("empty weighted model alternatives")?}else{value};
        let name=value.get("model").and_then(serde_json::Value::as_str).context("blockstate model reference missing")?;
        let model=resource_model(context,name,&mut BTreeSet::new())?;
        let (elements,strings)=model_elements(&model)?;
        result.push((Variant {condition:String::new(),model:0,x:(value.get("x").and_then(serde_json::Value::as_u64).unwrap_or(0)/90) as u8,y:(value.get("y").and_then(serde_json::Value::as_u64).unwrap_or(0)/90) as u8},elements,strings));
    }
    let result = Arc::new(result);
    cache.lock().map_err(|_| anyhow::anyhow!("selected model cache lock poisoned"))?.insert(key, result.clone());
    Ok(result)
}

/// Fast keeps its existing flat-color geometry/shading. Its representative
/// material color is now sampled from the selected models and real textures.
pub(crate) fn selected_palette_color(id: &str, properties: &BTreeMap<String,String>) -> Result<Option<[f32;3]>> {
    let context = crate::game_data::active_context()?;
    let models = selected_plane_models(&context, id, properties)?;
    let mut textures = BTreeSet::new();
    for (_, _, names) in models.iter() { textures.extend(names.iter().cloned()); }
    let mut sum = [0.0f64;3]; let mut weight = 0.0f64;
    for name in textures {
        let texture = selected_texture(&context, &name)?;
        for pixel in texture.pixels() {
            let alpha = f64::from(pixel[3]);
            for channel in 0..3 { sum[channel] += f64::from(pixel[channel]) * alpha; }
            weight += alpha;
        }
    }
    Ok((weight > 0.0).then(|| [0,1,2].map(|channel| (sum[channel]/weight/255.0) as f32)))
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
fn registry_adapter_root(context: &crate::game_data::GameDataContext) -> Result<std::path::PathBuf> {
    let own = context.root.join("derived/blockdb");
    if own.join("blocks.dat.z").is_file() { return Ok(own); }
    let common = crate::runtime_paths::app_root()?.join("pack-in/game-data/default/derived/blockdb");
    if common.join("blocks.dat.z").is_file() { return Ok(common); }
    bail!("selected Minecraft {} is missing the required public version-registry adapter", context.game_version)
}

impl BlockDatabase {
    fn load() -> Result<Self> {
        let context = crate::game_data::active_context()?;
        let path = registry_adapter_root(&context)?.join("blocks.dat.z");
        let bytes = inflate(&std::fs::read(&path).with_context(|| format!("selected Minecraft {} requires its block registry adapter: {}", context.game_version, path.display()))?, 64 * 1024 * 1024)?;
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
        let local = id.replace(':', ".").replace('/', ".");
        for key in [format!("block.{local}"), format!("item.{local}")] {
            if let Ok(Some(value)) = crate::item_names::selected_name(&key, "zh_cn") { return value; }
        }
        self.registry(version).ok().and_then(|registry| registry.blocks.get(id))
            .map(|block| block.en.clone()).filter(|name| !name.is_empty()).unwrap_or_else(|| id.to_owned())
    }
    pub(crate) fn category(&self, name: &str) -> Option<&str> {
        let context = crate::game_data::active_context().ok()?;
        self.registry(&context.game_version)
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
            return self.query_states(version, &[state.to_owned()], Some(Plane::Side))?["states"].get(state).cloned().context("方块颜色结果缺失");
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
    /// `plane` selects the rasterised view. `None` skips model rasterisation
    /// entirely and returns only the cheap metadata (name + map colour), which
    /// is what a layer canvas needs before it knows which blocks have a 2D icon.
    pub fn query_states(
        &self,
        version: &str,
        states: &[String],
        plane: Option<Plane>,
    ) -> Result<serde_json::Value> {
        if !self.supplement.map_color_versions.contains_key(version) { self.version(version)?; }
        let mut values = BTreeMap::new();
        for state in states {
            if !values.contains_key(state) {
                let name = crate::version_conversion::parse_key(state)
                    .ok()
                    .map(|(id, _)| self.name(version, &id));
                // A missing model must not discard an independently verified map
                // colour (technical/fluid blocks do not have cube models).
                let mut value = match plane {
                    Some(plane) => self.plane(version, state, plane).unwrap_or_else(|error| serde_json::json!({
                        "state": state,
                        "name": name,
                        "sideTextureDataUrl": null,
                        "topTextureDataUrl": null,
                        "tintIndices": [],
                        "faces": [],
                        "projection": "missing",
                        "missing": true,
                        "error": error.to_string(),
                    })),
                    None => serde_json::json!({
                        "state": state,
                        "name": name,
                        "sideTextureDataUrl": null,
                        "topTextureDataUrl": null,
                        "tintIndices": [],
                        "faces": [],
                        "projection": "color-only",
                    }),
                };
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
        self.plane(version, state, Plane::Side)
    }
    fn top(&self, version: &str, state: &str) -> Result<serde_json::Value> {
        self.plane(version, state, Plane::Top)
    }
    fn plane(&self, version: &str, state: &str, plane: Plane) -> Result<serde_json::Value> {
        let (id, properties) = crate::version_conversion::parse_key(state)?;
        let (texture_key, projection, no_model_projection) = match plane {
            Plane::Top => ("topTextureDataUrl", "model-top", "no-top-model"),
            Plane::Side => ("sideTextureDataUrl", "model-side", "no-side-model"),
        };
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
        if let Some(special) = self
            .supplement
            .special_planes
            .get(version)
            .and_then(|planes| planes.get(&special_key))
        {
            let mut value = serde_json::json!({
                "state": state,
                "name": self.name(version, &id),
                "sideTextureDataUrl": null,
                "topTextureDataUrl": null,
                "mapColor": self.map_color(version, state),
                "tintIndices": [],
                "faces": [{"texture": special["texture"], "rule": special["rule"]}],
                "projection": projection,
                "biomeProfile": "minecraft:plains",
                "tintProfileVersion": version,
            });
            value[texture_key] = serde_json::json!(format!(
                "data:image/png;base64,{}",
                special["png"].as_str().context("特殊模型平面数据损坏")?
            ));
            return Ok(value);
        }
        let context = crate::game_data::active_context()?;
        let selected = selected_plane_models(&context, &id, &properties)?;
        let mut image = RgbaImage::new(32, 32);
        let mut tints = BTreeSet::new();
        let mut used = Vec::new();
        for (variant, model, strings) in selected.iter() {
            for element in model {
                let Some((face, quad)) = select_plane(element, variant, plane) else {
                    continue;
                };
                let Some(path) = strings.get(face.texture as usize) else {
                    continue;
                };
                let texture = selected_texture(&context, path)?;
                let pixels = texture.as_raw();
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
                            * texture.width() as f32
                            / 16.0)
                            .clamp(0.0, texture.width() as f32 - 1.0)
                            as usize;
                        let ty = ((face.uv[1] + v * (face.uv[3] - face.uv[1]))
                            * texture.height() as f32
                            / 16.0)
                            .clamp(0.0, texture.height() as f32 - 1.0)
                            as usize;
                        let offset = (ty * texture.width() as usize + tx) * 4;
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
            return Ok(serde_json::json!({
                "state": state,
                "name": self.name(version, &id),
                "sideTextureDataUrl": null,
                "topTextureDataUrl": null,
                "mapColor": self.map_color(version, state),
                "tintIndices": [],
                "faces": [],
                "projection": no_model_projection,
            }));
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(
            image.as_raw(),
            32,
            32,
            image::ExtendedColorType::Rgba8,
        )?;
        let mut value = serde_json::json!({
            "state": state,
            "name": self.name(version, &id),
            "sideTextureDataUrl": null,
            "topTextureDataUrl": null,
            "mapColor": self.map_color(version, state),
            "tintIndices": tints,
            "faces": used,
            "projection": projection,
            "biomeProfile": "minecraft:plains",
            "tintProfileVersion": if self.supplement.plains.contains_key(version) { version } else { "26.3" },
        });
        value[texture_key] = serde_json::json!(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ));
        Ok(value)
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
    static DATA: OnceLock<Mutex<BTreeMap<String, &'static PlainsTintMetadata>>> = OnceLock::new();
    let context = crate::game_data::active_context()?;
    let mut cache = DATA.get_or_init(Default::default).lock().map_err(|_| anyhow::anyhow!("tint adapter cache lock poisoned"))?;
    if let Some(value) = cache.get(&context.content_id) { return Ok(*value); }
    let directory = registry_adapter_root(&context)?;
    let data: PlainsTintMetadata = serde_json::from_slice(&inflate(&std::fs::read(directory.join("plains-tints.dat.z"))?, 16 * 1024 * 1024)?)?;
    let blocks_sha = format!("{:x}", Sha256::digest(std::fs::read(directory.join("blocks.dat.z"))?));
    if data.schema != 1 || data.purpose != "block-database-side-plains-origin" || data.blocks_dat_sha256 != blocks_sha || data.versions.is_empty() || data.versions.values().any(|key| !data.tables.contains_key(key)) { bail!("selected tint adapter does not match its actual block database"); }
    let data: &'static PlainsTintMetadata = Box::leak(Box::new(data));
    cache.insert(context.content_id, data);
    Ok(data)
}

pub fn database() -> Result<&'static BlockDatabase> {
    static DB: OnceLock<Mutex<BTreeMap<String, &'static BlockDatabase>>> = OnceLock::new();
    let context = crate::game_data::active_context()?;
    let mut cache = DB.get_or_init(Default::default).lock().map_err(|_| anyhow::anyhow!("block registry cache lock poisoned"))?;
    if let Some(value) = cache.get(&context.content_id) { return Ok(*value); }
    let value: &'static BlockDatabase = Box::leak(Box::new(BlockDatabase::load()?));
    cache.insert(context.content_id, value);
    Ok(value)
}

pub fn normalized_registry(version: &str) -> Result<serde_json::Value> {
    let database = database()?;
    let registry = database.registry(version)?;
    let mut blocks = BTreeMap::new();
    for (id, block) in &registry.blocks {
        let (_, defaults) = crate::version_conversion::parse_key(&block.default)?;
        blocks.insert(id.clone(), serde_json::json!({"properties":block.properties,"default_properties":defaults}));
    }
    Ok(serde_json::json!({"version_id":version,"blocks":blocks}))
}

pub fn run_cli(args: &[String]) -> Result<()> {
    let mut version = None;
    let mut query = "";
    let mut state = None;
    let mut states_file = None;
    let mut no_textures = false;
    let mut top_plane = false;
    let mut normalized = false;
    let mut registry_items = false;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--versions" => {}
            "--normalized" => normalized = true,
            "--registry-items" => registry_items = true,
            "--no-textures" => no_textures = true,
            "--top-plane" => top_plane = true,
            "--version" => version = Some(iter.next().context("missing --version value")?.as_str()),
            "--query" => query = iter.next().context("missing --query value")?,
            "--state" => state = Some(iter.next().context("missing --state value")?.as_str()),
            "--states-json" => {
                states_file = Some(iter.next().context("missing --states-json value")?.as_str())
            }
            _ => bail!("unknown block-db argument: {flag}"),
        }
    }
    let plane = if no_textures {
        None
    } else if top_plane {
        Some(Plane::Top)
    } else {
        Some(Plane::Side)
    };
    let result = if normalized {
        normalized_registry(version.context("normalized registry requires --version")?)?
    } else if registry_items {
        serde_json::json!({"version_id": version, "items": database()?.registry(version.context("item registry requires --version")?)?.items})
    } else if let Some(path) = states_file {
        let states: Vec<String> = serde_json::from_slice(&std::fs::read(path)?)?;
        database()?.query_states(version.context("states query requires --version")?, &states, plane)?
    } else if let Some(state) = state {
        let version = version.context("state query requires --version")?;
        database()?.query_states(version, &[state.to_owned()], plane)?["states"]
            .get(state)
            .cloned()
            .context("方块颜色结果缺失")?
    } else {
        database()?.query(version, query, None)?
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
        let result = db.query_states("1.21.1", &["minecraft:water[level=0]".into(), "minecraft:structure_void".into()], None).unwrap();
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
        let result = db.query_states("1.0", &["1:0".into(), "3:0".into(), "0:0".into(), "9999:0".into()], None).unwrap();
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
    fn top_plane_uses_the_up_face_and_distinguishes_log_from_wood() {
        let db = database().unwrap();
        let log = db.top("26.3", "minecraft:oak_log[axis=y]").unwrap();
        assert_eq!(log["projection"], "model-top");
        assert!(
            log["topTextureDataUrl"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(
            log["faces"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["texture"] == "block/oak_log_top")
        );
        // Oak wood has bark on every face; its top plane must not become the
        // log's ring texture.
        let wood = db.top("26.3", "minecraft:oak_wood[axis=y]").unwrap();
        assert_ne!(log["topTextureDataUrl"], wood["topTextureDataUrl"]);
        let button = pixels_top("minecraft:stone_button[face=floor,facing=north,powered=false]");
        let (width, height) = opaque_bounds(&button);
        assert!(
            width >= 8 && height >= 8,
            "button top plane collapsed to {width}x{height}"
        );
    }
    #[test]
    fn color_only_queries_skip_model_rasterisation() {
        let db = database().unwrap();
        let result = db
            .query_states("26.3", &["minecraft:oak_log[axis=y]".into()], None)
            .unwrap();
        let state = &result["states"]["minecraft:oak_log[axis=y]"];
        assert_eq!(state["projection"], "color-only");
        assert!(state["sideTextureDataUrl"].is_null());
        assert!(state["topTextureDataUrl"].is_null());
        assert!(state["mapColor"].is_object());
    }
    #[test]
    fn builtin_chest_and_skull_use_actual_flat_entity_atlas_and_batch_has_no_process_per_state() {
        let db = database().unwrap();
        let states = vec![
            "minecraft:chest[facing=north,type=single,waterlogged=false]".into(),
            "minecraft:skeleton_skull[rotation=0]".into(),
            "mod:missing".into(),
        ];
        let result = db.query_states("26.3", &states, Some(Plane::Side)).unwrap();
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
    fn pixels_top(state: &str) -> RgbaImage {
        let result = database().unwrap().top("26.3", state).unwrap();
        let url = result["topTextureDataUrl"].as_str().expect("actual top plane");
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
