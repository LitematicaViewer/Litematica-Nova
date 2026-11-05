//! Invoked only by the installer on a same-directory upgrade. The two
//! allowlists are extracted from Setup, not read from the previous installation.
use anyhow::{ensure, Context, Result};
use litematica_core::game_data::ResourceRecord;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, HashMap}, ffi::c_void, fs::{self, File, OpenOptions},
    io::{self, BufReader}, os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle},
    path::{Component, Path, PathBuf}};

const REPARSE: u32 = 0x400;
const NO_FOLLOW: u32 = 0x00200000;
const BACKUP_SEMANTICS: u32 = 0x02000000;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetFileInformationByHandle(file: *mut c_void, class: i32, data: *const c_void, size: u32) -> i32;
}

// Pin each directory without FILE_SHARE_DELETE before opening its children:
// even a junction substituted while upgrading cannot redirect deletion.
fn pin_directory(path: &Path, pinned: &mut HashMap<PathBuf, Option<File>>) -> bool {
    if let Some(value) = pinned.get(path) { return value.is_some(); }
    if let Some(parent) = path.parent() {
        if !pin_directory(parent, pinned) { return false; }
    }
    let handle = OpenOptions::new().access_mode(0x80).share_mode(3)
        .custom_flags(NO_FOLLOW | BACKUP_SEMANTICS).open(path).ok()
        .filter(|h| h.metadata().is_ok_and(|m| m.is_dir() && m.file_attributes() & REPARSE == 0));
    let ok = handle.is_some(); pinned.insert(path.to_owned(), handle); ok
}

fn remove_match(path: &Path, expected: &ResourceRecord) -> io::Result<bool> {
    let mut handle = match OpenOptions::new().access_mode(0x80000000 | 0x10000) // GENERIC_READ | DELETE
        .share_mode(1).custom_flags(NO_FOLLOW).open(path) {
        Ok(h) => h, Err(e) if e.kind()==io::ErrorKind::NotFound => return Ok(false), Err(e) => return Err(e),
    };
    let m = handle.metadata()?;
    if !m.is_file() || m.file_attributes() & REPARSE != 0 || m.len()!=expected.bytes { return Ok(false); }
    let mut hash=Sha256::new(); io::copy(&mut handle,&mut hash)?;
    if format!("{:x}",hash.finalize())!=expected.sha256 { return Ok(false); }
    // FileDispositionInfo / FILE_DISPOSITION_INFO.DeleteFile is a Win32 BOOLEAN.
    // Mark exactly the handle that was hashed, not its subsequently resolved name.
    let delete: u8=1;
    if unsafe { SetFileInformationByHandle(handle.as_raw_handle(),4,(&delete as *const u8).cast(),1) }==0 {
        return Err(io::Error::last_os_error());
    }
    Ok(true)
}

#[derive(Deserialize)]
struct Versions { format:String, legacy_archives:BTreeMap<String,ResourceRecord> }

pub fn run() -> Result<()> {
    let args:Vec<_>=std::env::args_os().skip(2).map(PathBuf::from).collect();
    ensure!(args.len()==3 && args[0].is_absolute(), "expected install directory and two Setup receipts");
    let root=args[0].join("pack-in/game-data/default");
    let mut index:BTreeMap<String,ResourceRecord>=serde_json::from_reader(BufReader::new(File::open(&args[1])?))?;
    // Only retired raw exports paired with a replacement in the Setup's own receipt are eligible.
    let retired: std::collections::HashSet<_> = ["skip_render_rows", "block_state_semantics", "block_states", "tags"].into_iter()
        .filter(|name| index.contains_key(&format!("derived/oracle/{name}.novatab")))
        .map(|name| format!("derived/oracle/{name}.jsonl")).collect();
    index.retain(|k,_| retired.contains(k) || k.starts_with("assets/") || k.starts_with("data/") || matches!(k.as_str(),"version.json"|"pack.png"|"pack.mcmeta"));
    let versions:Versions=serde_json::from_reader(BufReader::new(File::open(&args[2])?))?;
    ensure!(versions.format=="nova.version-adapter-store","invalid official archive receipt");
    for (name,entry) in versions.legacy_archives {
        ensure!(name.ends_with(".zip") && name.bytes().all(|c|c.is_ascii_alphanumeric() || b"._-".contains(&c)), "invalid archive name");
        index.insert(format!("derived/version-adapters/{name}"),entry);
    }
    let mut pinned=HashMap::new(); let (mut removed,mut busy)=(0,0);
    for (name,entry) in index {
        ensure!(!name.contains(['\\',':']) && Path::new(&name).components().all(|p|matches!(p,Component::Normal(_))),"invalid install resource path");
        let file=root.join(name); let parent=file.parent().context("expected resource directory")?;
        if !pin_directory(parent,&mut pinned) { continue; }
        match remove_match(&file,&entry) { Ok(true)=>removed+=1,Ok(false)=>(),Err(_)=>busy+=1 }
    }
    // Only non-recursive empty-directory removal inside the two superseded trees.
    let mut dirs:Vec<_>=pinned.iter().filter_map(|(p,h)| (h.is_some() && (p.starts_with(root.join("assets")) || p.starts_with(root.join("data")))).then_some(p.clone())).collect();
    dirs.sort_unstable_by_key(|p|std::cmp::Reverse(p.components().count()));
    for dir in dirs { pinned.remove(&dir); let _=fs::remove_dir(&dir); }
    println!("Replaced official resource files removed: {removed}; inaccessible files retained: {busy}.");
    Ok(())
}
