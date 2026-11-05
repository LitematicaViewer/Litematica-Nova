//! Exact-version assets and explicit pack precedence for the independent 2D view.
//! This does not change the game-data snapshot used by Fast or the private core.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::{BTreeMap, VecDeque}, fs::{self, File}, io::Read,
    path::{Component, Path, PathBuf}, sync::{Arc, Mutex, OnceLock}, time::SystemTime};

pub trait ResourceAccess {
    fn identity(&self) -> &str;
    fn read(&self, relative: &str) -> Result<Option<Vec<u8>>>;
}
impl ResourceAccess for crate::game_data::GameDataContext {
    fn identity(&self) -> &str { &self.content_id }
    fn read(&self, relative: &str) -> Result<Option<Vec<u8>>> { crate::game_data::read_resource_from(self, relative) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp { path: PathBuf, bytes: u64, modified: SystemTime }
impl Stamp {
    fn read(path: &Path) -> Result<Self> {
        let path = path.canonicalize().with_context(||format!("Layer resource unavailable: {}",path.display()))?;
        let metadata = fs::metadata(&path)?;
        Ok(Self { path, bytes: metadata.len(), modified: metadata.modified()? })
    }
    fn current(&self) -> bool { Self::read(&self.path).is_ok_and(|value|value==*self) }
}
type ResourceTable = BTreeMap<String,String>;
enum Resources { Legacy(Arc<ResourceTable>), Compact(usize) }
struct VersionAssets { source_sha256: String, resources: Resources }
struct ArchiveIndex { identity: String, versions: BTreeMap<String,VersionAssets>, compact: Option<CompactMaps> }
struct CompactMaps {
    names: Vec<String>, blobs: Vec<String>,
    anchors: Vec<Vec<u32>>, tables: Vec<(usize, Vec<(u32, u32)>)>,
}
// A table is either a sparse override for a resource, or a single lookup in a
// full anchor. No release recursively loads another release or builds a large map.
impl CompactMaps {
    fn validate(&self) -> Result<()> {
        ensure!(!self.names.is_empty() && self.names.windows(2).all(|v| v[0] < v[1])
            && self.names.iter().all(|name| safe_relative(name).is_ok()), "Invalid layer name dictionary");
        ensure!(!self.blobs.is_empty() && self.blobs.windows(2).all(|v| v[0] < v[1])
            && self.blobs.iter().all(|s| valid_sha(s)), "Invalid layer SHA dictionary");
        let valid_blob = |id: u32| (id as usize) <= self.blobs.len(); // 0 denotes missing
        for a in &self.anchors {
            ensure!(a.len() == self.names.len() && a.iter().copied().all(valid_blob), "Invalid full layer anchor");
        }
        for (anchor, changes) in &self.tables {
            ensure!(*anchor < self.anchors.len() && changes.windows(2).all(|c| c[0].0 < c[1].0)
                && changes.iter().all(|&(p,id)| (p as usize) < self.names.len() && valid_blob(id)),
                "Invalid sparse layer table");
        }
        Ok(())
    }
    fn blob(&self, table: usize, key: &str) -> Option<(&str, u32)> {
        let pos = self.names.binary_search_by(|s| s.as_str().cmp(key)).ok()?;
        let (anchor, changes) = &self.tables[table];
        let id = match changes.binary_search_by_key(&(pos as u32), |p| p.0) {
            Ok(i) => changes[i].1, Err(_) => self.anchors[*anchor][pos],
        };
        id.checked_sub(1).map(|i| (self.blobs[i as usize].as_str(), id))
    }
}
fn valid_sha(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl ArchiveIndex {
    fn blob(&self, version: &str, key: &str) -> Option<(&str, Option<u32>)> {
        match &self.versions.get(version)?.resources {
            Resources::Legacy(map) => map.get(key).map(|s| (s.as_str(), None)),
            Resources::Compact(t) => self.compact.as_ref()?.blob(*t, key).map(|(sha,member)| (sha,Some(member))),
        }
    }
}
fn decode_index(bytes: &[u8]) -> Result<ArchiveIndex> {
    #[derive(Deserialize)]
    struct Header { schema: u32 }
    if serde_json::from_slice::<Header>(bytes)?.schema == 3 {
        #[derive(Deserialize)]
        struct V3 {
            identity: String, versions: BTreeMap<String, (String, usize)>,
            names: Vec<String>, blobs: Vec<String>, anchors: Vec<Vec<u32>>, tables: Vec<(usize, Vec<(u32,u32)>)>,
        }
        let v: V3 = serde_json::from_slice(bytes)?;
        ensure!(valid_sha(&v.identity) && !v.versions.is_empty(), "Invalid layer archive identity or versions");
        let compact = CompactMaps { names:v.names, blobs:v.blobs, anchors:v.anchors, tables:v.tables };
        compact.validate()?;
        let mut versions = BTreeMap::new();
        for (name, (source_sha256, table)) in v.versions {
            ensure!(valid_sha(&source_sha256) && table < compact.tables.len(), "Invalid layer resource table for {name}");
            versions.insert(name, VersionAssets {source_sha256, resources: Resources::Compact(table)});
        }
        return Ok(ArchiveIndex {identity:v.identity, versions, compact:Some(compact)});
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Table { Shared(String), Original(ResourceTable) }
    #[derive(Deserialize)]
    struct Version { source_sha256: String, resources: Table }
    #[derive(Deserialize)]
    struct Stored { schema: u32, identity: String, versions: BTreeMap<String,Version>,
        #[serde(default)] tables: BTreeMap<String,ResourceTable> }
    let stored: Stored = serde_json::from_slice(bytes)?;
    ensure!(matches!(stored.schema, 1|2) && stored.identity.len()==64, "Invalid layer asset index");
    let tables: BTreeMap<_,_> = stored.tables.into_iter().map(|(id, table)| (id, Arc::new(table))).collect();
    let mut versions = BTreeMap::new();
    for (version, record) in stored.versions {
        let resources = match (stored.schema, record.resources) {
            (1, Table::Original(table)) => Arc::new(table),
            (2, Table::Shared(id)) => tables.get(&id).with_context(||format!("Missing layer resource table for {version}: {id}"))?.clone(),
            _ => bail!("Invalid layer resource table for {version}"),
        };
        versions.insert(version, VersionAssets { source_sha256: record.source_sha256, resources:Resources::Legacy(resources) });
    }
    Ok(ArchiveIndex { identity: stored.identity, versions, compact:None })
}
struct AssetArchive { stamp: Stamp, index_stamp: Stamp, index: ArchiveIndex, zip: Mutex<zip::ZipArchive<File>> }
fn archive() -> Result<Arc<AssetArchive>> {
    static CACHE: OnceLock<Mutex<Option<Arc<AssetArchive>>>> = OnceLock::new();
    let root = crate::runtime_paths::app_root()?.join("pack-in/layer-resources");
    let stamp = Stamp::read(&root.join("assets.zip"))?;
    let index_stamp = Stamp::read(&root.join("index.json.z"))?;
    let mut cache = CACHE.get_or_init(Default::default).lock().map_err(|_|anyhow::anyhow!("Layer asset archive lock poisoned"))?;
    if let Some(value)=cache.as_ref().filter(|value|value.stamp==stamp && value.index_stamp==index_stamp) { return Ok(value.clone()); }
    let compressed=fs::read(root.join("index.json.z"))?;
    let mut bytes=Vec::new();
    flate2::read::ZlibDecoder::new(compressed.as_slice()).take(128*1024*1024+1).read_to_end(&mut bytes)?;
    ensure!(bytes.len()<=128*1024*1024,"Layer asset index exceeds its resource limit");
    let index=decode_index(&bytes)?;
    let zip=zip::ZipArchive::new(File::open(&stamp.path)?)?;
    let value=Arc::new(AssetArchive{stamp,index_stamp,index,zip:Mutex::new(zip)});*cache=Some(value.clone());Ok(value)
}
fn safe_relative(value:&str)->Result<&str> {
    ensure!((value.starts_with("assets/") || (value.starts_with("data/") && value.contains("/worldgen/biome/") && value.ends_with(".json"))) && !value.contains('\\') && !value.contains('\0')
        && Path::new(value).components().all(|part|matches!(part,Component::Normal(_))),"Invalid layer resource key: {value}");
    Ok(value)
}
fn limited_read(mut file:impl Read)->Result<Vec<u8>> {
    let mut bytes=Vec::new();file.by_ref().take(64*1024*1024+1).read_to_end(&mut bytes)?;
    ensure!(bytes.len()<=64*1024*1024,"Layer resource exceeds its size limit");Ok(bytes)
}
fn files(root:&Path)->Result<Vec<Stamp>> {
    fn walk(root:&Path,path:&Path,out:&mut Vec<Stamp>)->Result<()> {
        for entry in fs::read_dir(path)? { let entry=entry?;let kind=entry.file_type()?;
            ensure!(!kind.is_symlink(),"Layer resource pack contains a symlink: {}",entry.path().display());
            if kind.is_dir(){walk(root,&entry.path(),out)?}else if kind.is_file(){let stamp=Stamp::read(&entry.path())?;
                ensure!(stamp.path.starts_with(root),"Layer resource leaves its pack");out.push(stamp);}
        }Ok(())
    }
    let mut out=Vec::new();let assets=root.join("assets");if assets.is_dir(){walk(root,&assets,&mut out)?}out.sort_by(|a,b|a.path.cmp(&b.path));Ok(out)
}
struct Pack { stamp:Stamp, identity:String, directory:Option<Vec<Stamp>>, zip:Option<Mutex<zip::ZipArchive<File>>> }
impl Pack {
    fn open(path:&Path)->Result<Arc<Self>> {
        static CACHE:OnceLock<Mutex<VecDeque<Arc<Pack>>>>=OnceLock::new();
        let stamp=Stamp::read(path)?;let directory=stamp.path.is_dir().then(||files(&stamp.path)).transpose()?;
        let mut cache=CACHE.get_or_init(Default::default).lock().map_err(|_|anyhow::anyhow!("Layer pack cache lock poisoned"))?;
        if let Some(value)=cache.iter().find(|value|value.stamp==stamp&&value.directory==directory){return Ok(value.clone())}
        let mut hash=Sha256::new();let zip=if let Some(entries)=&directory {
            for entry in entries {hash.update(entry.path.strip_prefix(&stamp.path)?.to_string_lossy().replace('\\',"/").as_bytes());
                hash.update(fs::read(&entry.path)?);}
            None
        }else{
            let mut file=File::open(&stamp.path)?;let mut chunk=[0;64*1024];loop{let count=file.read(&mut chunk)?;if count==0{break}hash.update(&chunk[..count]);}
            Some(Mutex::new(zip::ZipArchive::new(File::open(&stamp.path)?)?))
        };
        ensure!(stamp.current(),"Layer resource pack changed while reading its identity");
        let value=Arc::new(Self{stamp,identity:format!("{:x}",hash.finalize()),directory,zip});
        while cache.len()>=16{cache.pop_front();}cache.push_back(value.clone());Ok(value)
    }
    fn current(&self)->Result<bool>{Ok(self.stamp.current()&&match &self.directory{Some(known)=>files(&self.stamp.path)?==*known,None=>true})}
    fn read(&self,key:&str)->Result<Option<Vec<u8>>> {
        if let Some(entries)=&self.directory {
            // Match actual stored spelling, including on case-insensitive Windows.
            let path=self.stamp.path.join(key);
            if entries.iter().any(|entry|entry.path==path){return Ok(Some(limited_read(File::open(path)?)?))}return Ok(None)
        }
        let mut zip=self.zip.as_ref().context("Layer pack reader missing")?.lock().map_err(|_|anyhow::anyhow!("Layer pack reader poisoned"))?;
        match zip.by_name(key){Ok(file)=>Ok(Some(limited_read(file)?)),Err(zip::result::ZipError::FileNotFound)=>Ok(None),Err(error)=>Err(error.into())}
    }
}

pub struct LayerResourceContext { version:String, identity:String, selected:crate::game_data::GameDataContext,
    base:Option<crate::game_data::GameDataContext>, archive:Arc<AssetArchive>, packs:Vec<Arc<Pack>> }
impl LayerResourceContext {
    pub fn resolve(version:&str,paths:&[PathBuf])->Result<Arc<Self>> {
        let selected=crate::game_data::active_context()?;let archive=archive()?;
        ensure!(archive.index.versions.contains_key(version),"Exact layer resource version is unavailable: {version}");
        let base=(selected.game_version==version).then(||selected.clone());
        let packs=paths.iter().map(|path|Pack::open(path)).collect::<Result<Vec<_>>>()?;
        let mut hash=Sha256::new();hash.update(b"nova.layer.resources.v2\0");hash.update(version);hash.update(&archive.index.identity);
        hash.update(&archive.index.versions[version].source_sha256);hash.update(&selected.content_id);
        for pack in &packs{hash.update([0]);hash.update(&pack.identity);}
        Ok(Arc::new(Self{version:version.into(),identity:format!("{:x}",hash.finalize()),selected,base,archive,packs}))
    }
    pub fn identity(&self)->&str{&self.identity}
    pub fn allows_baked_fallback(&self)->bool{self.base.is_none()}
    pub fn is_current(&self)->Result<bool>{
        if !self.archive.stamp.current() || !self.archive.index_stamp.current() || crate::game_data::active_context()?.content_id!=self.selected.content_id{return Ok(false)}
        for pack in &self.packs{if !pack.current()?{return Ok(false)}}Ok(true)
    }
    pub fn origin(&self,key:&str)->Result<Option<String>> {
        safe_relative(key)?;
        if key.starts_with("assets/") { for pack in self.packs.iter().rev(){if pack.read(key)?.is_some(){return Ok(Some(format!("pack:{}",pack.identity)))}} }
        if let Some(base)=&self.base{return Ok(base.read(key)?.map(|_|format!("game:{}",base.content_id)))}
        Ok(self.archive.index.blob(&self.version, key).is_some().then(||format!("vanilla:{}:{}",self.version,self.archive.index.versions[&self.version].source_sha256)))
    }
}
impl ResourceAccess for LayerResourceContext {
    fn identity(&self)->&str{&self.identity}
    fn read(&self,key:&str)->Result<Option<Vec<u8>>>{
        safe_relative(key)?;
        if key.starts_with("assets/") { for pack in self.packs.iter().rev(){if let Some(bytes)=pack.read(key)?{return Ok(Some(bytes))}} }
        // A selected imported JAR is authoritative, including genuine omissions.
        // Never fill a modified package's missing files from a different vanilla.
        if let Some(base)=&self.base{return base.read(key)}
        let Some((sha,member))=self.archive.index.blob(&self.version,key) else { return Ok(None) };
        let mut zip=self.archive.zip.lock().map_err(|_|anyhow::anyhow!("Layer asset reader lock poisoned"))?;
        Ok(Some(read_blob(&mut zip, sha, member, key)?))
    }
}
thread_local!{static CONTEXT:RefCell<Option<Arc<LayerResourceContext>>>=const{RefCell::new(None)};}
fn read_blob<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, sha: &str, member: Option<u32>, key: &str) -> Result<Vec<u8>> {
    let short = member.map(|id| id.to_string());
    let file = zip.by_name(short.as_deref().unwrap_or(sha)).with_context(||format!("Layer source blob missing: {key}"))?;
    let bytes = limited_read(file)?;
    ensure!(format!("{:x}",Sha256::digest(&bytes)) == sha, "Layer source blob identity mismatch: {key}");
    Ok(bytes)
}

pub struct LayerResourceScope(Option<Arc<LayerResourceContext>>);
pub fn enter(context:Arc<LayerResourceContext>)->LayerResourceScope{LayerResourceScope(CONTEXT.with(|slot|slot.replace(Some(context))))}
impl Drop for LayerResourceScope{fn drop(&mut self){CONTEXT.with(|slot|{slot.replace(self.0.take());});}}
pub fn current(version:&str)->Result<Arc<LayerResourceContext>>{
    if let Some(context)=CONTEXT.with(|slot|slot.borrow().clone()){
        if context.version!=version{bail!("Layer resource scope version mismatch: {version}")};return Ok(context)
    }
    LayerResourceContext::resolve(version,&[])
}


#[cfg(test)]
mod compact_tests {
    use super::*;
    use serde_json::json;
    use std::io::{Cursor, Write};

    #[test]
    fn compact_layer_single_hop_keeps_absence_and_validates_sparse_ids_and_blob_bytes() {
        let first = format!("{:x}", Sha256::digest(b"alpha"));
        let second = format!("{:x}", Sha256::digest(b"beta"));
        let mut hashes = vec![first.clone(), second.clone()];
        hashes.sort();
        let (a,b) = if first == hashes[0] { (1,2) } else { (2,1) };
        let keys = ["assets/a/textures/first.png", "assets/a/textures/second.png"];
        let wire = json!({"schema":3, "identity":first, "names":keys, "blobs":hashes, "anchors":[[a,0]],
            "tables":[[0,[]],[0,[[0,0],[1,b]]]], "versions":{"old":[first,0],"new":[second,1]}});
        let decode = |v: &serde_json::Value| decode_index(&serde_json::to_vec(v).unwrap());
        let index=decode(&wire).unwrap();
        assert_eq!(index.blob("old",keys[0]),Some((first.as_str(),Some(a))));
        assert_eq!(index.blob("old",keys[1]),None);
        assert_eq!(index.blob("new",keys[0]),None);
        assert_eq!(index.blob("new",keys[1]),Some((second.as_str(),Some(b))));
        for changes in [json!([[1,3]]),json!([[2,1]]),json!([[1,1],[0,2]]),json!([[0,0],[0,1]])] {
            let mut bad=wire.clone();bad["tables"][1][1]=changes;assert!(decode(&bad).is_err());
        }
        let mut bad=wire.clone();bad["tables"][0][0]=json!(1);assert!(decode(&bad).is_err());
        let mut bad=wire.clone();bad["versions"]["new"][1]=json!(2);assert!(decode(&bad).is_err());
        let mut writer=zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file(a.to_string(), zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(b"alpha").unwrap();
        let mut zip=zip::ZipArchive::new(writer.finish().unwrap()).unwrap();
        assert_eq!(read_blob(&mut zip,&first,Some(a),keys[0]).unwrap(),b"alpha");
        assert!(read_blob(&mut zip,&second,Some(a),keys[0]).is_err());
        assert!(read_blob(&mut zip,&second,Some(b),keys[1]).is_err());
    }
}
