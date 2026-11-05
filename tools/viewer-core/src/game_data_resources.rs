//! The immutable package index covers both JAR entries and loose derived data.
//! Seek cursors share the file and ZIP directory, not a mutable file position.
use super::*;
use std::{collections::{HashMap, VecDeque}, io::{self, Seek, SeekFrom}, sync::{Arc, Mutex, OnceLock}};

#[derive(Clone)]
struct FileCursor { file: Arc<File>, length: u64, pos: u64 }
impl Read for FileCursor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(&*self.file, buf, self.pos)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(&*self.file, buf, self.pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}
impl Seek for FileCursor {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(v) => Some(v),
            SeekFrom::End(v) => self.length.checked_add_signed(v),
            SeekFrom::Current(v) => self.pos.checked_add_signed(v),
        }.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid resource JAR seek"))?;
        self.pos = pos;
        Ok(pos)
    }
}
struct JarReader(BufReader<FileCursor>);
impl Read for JarReader { fn read(&mut self, b: &mut [u8]) -> io::Result<usize> { self.0.read(b) } }
impl Seek for JarReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> { self.0.seek(pos) }
    fn stream_position(&mut self) -> io::Result<u64> { self.0.stream_position() }
}
impl Clone for JarReader {
    fn clone(&self) -> Self {
        // A ZIP lookup always seeks to its own entry before reading.
        let mut cursor = self.0.get_ref().clone();
        cursor.pos -= self.0.buffer().len() as u64;
        Self(BufReader::new(cursor))
    }
}

#[derive(Default)]
struct HotAssets { bytes: HashMap<usize, Vec<u8>>, order: VecDeque<usize> }
struct Reader {
    index: BTreeMap<String, ResourceRecord>,
    jar: Option<zip::ZipArchive<JarReader>>,
    hot: Mutex<HotAssets>,
}
impl Reader {
    fn open(context: &GameDataContext) -> Result<Self> {
        let index = read_json(&context.root.join("index.json"))?;
        let jar = match File::open(&context.resource_jar) {
            Ok(file) => {
                let cursor = FileCursor { length: file.metadata()?.len(), file: Arc::new(file), pos: 0 };
                Some(zip::ZipArchive::new(JarReader(BufReader::new(cursor))).context("invalid resource JAR")?)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None, // legacy indexed loose resources
            Err(e) => return Err(e.into()),
        };
        Ok(Self { index, jar, hot: Mutex::new(HotAssets::default()) })
    }
}
fn reader(context: &GameDataContext) -> Result<Arc<Reader>> {
    type Slot = Arc<Mutex<Option<Arc<Reader>>>>;
    type Cache = VecDeque<(PathBuf, String, Slot)>;
    static READERS: OnceLock<Mutex<Cache>> = OnceLock::new();
    let slot = {
        let mut cache = READERS.get_or_init(Default::default).lock().map_err(|_| anyhow::anyhow!("resource cache poisoned"))?;
        if let Some((_, _, slot)) = cache.iter().find(|(path, id, _)| path == &context.root && id == &context.content_id) {
            slot.clone()
        } else {
            let slot = Slot::default();
            if cache.len() == 4 { cache.pop_front(); }
            cache.push_back((context.root.clone(), context.content_id.clone(), slot.clone()));
            slot
        }
    };
    let mut local = slot.lock().map_err(|_| anyhow::anyhow!("package reader poisoned"))?;
    if let Some(reader) = local.as_ref() { return Ok(reader.clone()); }
    let opened = Arc::new(Reader::open(context).with_context(|| format!("game resource package {}", context.game_version))?);
    *local = Some(opened.clone());
    Ok(opened)
}

fn loose(context: &GameDataContext, relative: &str) -> Result<Option<Vec<u8>>> {
    let path = context.root.join(relative);
    if !path.is_file() { return Ok(None); }
    ensure!(path.canonicalize()?.starts_with(context.root.canonicalize()?), "game-resource symlink leaves its package");
    Ok(Some(fs::read(path)?))
}

pub(super) fn read(context: &GameDataContext, relative: &str) -> Result<Option<Vec<u8>>> {
    let relative = normalized(relative)?;
    if relative.starts_with("derived/") { return loose(context, &relative); }
    ensure!(resource_path(&relative), "not a resource/data package path");
    let reader = reader(context)?;
    let Some(record) = reader.index.get(&relative) else { return Ok(None); };
    let entry = reader.jar.as_ref().and_then(|jar| jar.index_for_name(&relative).map(|i| (jar, i)));
    let cacheable = relative.starts_with("assets/") && record.bytes <= 64 * 1024;
    let bytes = if let Some((jar, i)) = entry {
        if cacheable {
            if let Some(bytes) = reader.hot.lock().map_err(|_| anyhow::anyhow!("asset cache poisoned"))?.bytes.get(&i) { return Ok(Some(bytes.clone())); }
        }
        let mut jar = jar.clone();
        let entry = jar.by_index(i).with_context(|| format!("{}: {relative}", context.game_version))?;
        ensure!(entry.size() == record.bytes && record.bytes <= 1024 * 1024 * 1024, "{}: invalid JAR resource size: {relative}", context.game_version);
        let mut bytes = Vec::with_capacity(record.bytes.min(65536) as usize);
        entry.take(record.bytes + 1).read_to_end(&mut bytes).with_context(|| format!("{}: corrupt JAR resource: {relative}", context.game_version))?;
        bytes
    } else {
        loose(context, &relative)?.with_context(|| format!("{}: indexed game resource missing: {relative}", context.game_version))?
    };
    ensure!(bytes.len() as u64 == record.bytes && format!("{:x}", Sha256::digest(&bytes)) == record.sha256,
        "{}: resource differs from package index: {relative}", context.game_version);
    if let Some((_, i)) = entry.filter(|_| cacheable) {
        let mut hot = reader.hot.lock().map_err(|_| anyhow::anyhow!("asset cache poisoned"))?;
        if !hot.bytes.contains_key(&i) {
            if hot.order.len() == 128 { if let Some(old) = hot.order.pop_front() { hot.bytes.remove(&old); } }
            hot.order.push_back(i);
            hot.bytes.insert(i, bytes.clone());
        }
    }
    Ok(Some(bytes))
}

pub(super) fn list(context: &GameDataContext, prefix: &str) -> Result<Vec<String>> {
    let reader = reader(context)?;
    if prefix.is_empty() { return Ok(reader.index.keys().cloned().collect()); }
    let prefix = normalized(prefix.trim_end_matches('/'))?;
    let directory = format!("{prefix}/");
    let exact = reader.index.get_key_value(&prefix).map(|(key, _)| key.clone());
    Ok(exact.into_iter().chain(reader.index.range(directory.clone()..)
        .take_while(|(path, _)| path.starts_with(&directory)).map(|(path, _)| path.clone())).collect())
}
