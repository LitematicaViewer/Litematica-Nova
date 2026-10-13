//! Deterministic Faithful-style 64x to 32x resource-pack derivation.
//!
//! Version 2 applies the following closed algorithm to every regular file in a directory tree:
//!
//! - paths must be UTF-8, contain only normal components, and be unique under ASCII case folding;
//!   symbolic links and other special entries are rejected; processing order is ascending UTF-8
//!   path bytes after `/` separator normalization;
//! - files whose final extension is ASCII-case-insensitively `png` are decoded as PNG and must have
//!   positive even width and height; both dimensions are divided by two, including animation
//!   strips, GUI textures and `pack.png`; decoded channels are always interpreted as sRGB and PNG
//!   ancillary chunks are intentionally not copied to the newly encoded RGBA8 image; decoded
//!   8-bit grayscale/RGB/RGBA samples and losslessly expanded low-bit indexed palettes are accepted,
//!   while 16-bit/float samples are rejected instead of truncated;
//! - all color math is signed Q30 fixed point. `SRGB_TO_LINEAR_Q30` freezes the 256 IEC 61966-2-1
//!   inputs; `LANCZOS3_HALF_WEIGHTS_Q30` freezes the normalized 12-tap, center-sampled, scale-aware
//!   2:1 Lanczos3 kernel at source offsets `-5..=6`, sums exactly to `1<<30`, and uses clamped edge
//!   extension. Each multiply/divide rounds to nearest with half away from zero;
//! - linear RGB is premultiplied before the two separable passes. Filtered alpha is clamped to Q30
//!   `[0, 1]`; RGB is clamped to `[0, alpha]`, unpremultiplied, then mapped to RGBA8 by the frozen
//!   `LINEAR_TO_SRGB_THRESHOLDS_Q30` table. No runtime transcendental function participates;
//! - output PNGs are RGBA8 encoded with the locked `image`/`png` dependency using `Best`
//!   compression and `NoFilter`; for `.png.mcmeta`, only integer `animation.width` and
//!   `animation.height` are divided by two (they must be positive and even), while frame indices,
//!   timing and all other sections such as `gui.scaling` retain their values. Animation frame grids
//!   follow MC 26.2 `calculateFrameSize`: width-only uses the full image height, height-only uses the
//!   full image width, and no dimensions uses a square `min(width,height)` frame. Every physical
//!   frame is filtered independently to prevent cross-frame bleed. Metadata without an explicit
//!   animation dimension and every other non-PNG file are copied byte for byte; rewritten metadata
//!   uses the locked `serde_json` compact serializer followed by one LF byte;
//! - an existing output path, overlapping input/output trees, links and odd PNG dimensions are hard
//!   errors. A complete sibling `*.partial-*` tree is built first and installed with Windows
//!   `MoveFileW` or Linux `renameat2(RENAME_NOREPLACE)`; unsupported platforms fail closed. Failures
//!   remove the partial tree rather than exposing partial output.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::BufWriter;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ColorType, ImageEncoder, ImageFormat, ImageReader, RgbaImage};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn MoveFileW(existing: *const u16, destination: *const u16) -> i32;
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn renameat2(
        old_directory: i32,
        old_path: *const std::ffi::c_char,
        new_directory: i32,
        new_path: *const std::ffi::c_char,
        flags: u32,
    ) -> i32;
}

pub const ALGORITHM_ID: &str = "lba.resource-pack-64x-to-32x.lanczos3-fixed-premul.v2";
const TREE_HASH_DOMAIN: &[u8] = b"lba.directory-tree.v1\0";
const FIXED_ONE: i64 = 1_i64 << 30;
// For t = {-2.75,-2.25,...,2.75}, freeze
// [sin(pi*t)/(pi*t)] * [sin(pi*t/3)/(pi*t/3)], normalize the 12 values, then round to Q30. The
// rounded constants sum exactly to FIXED_ONE, so no runtime normalization or transcendental
// function is required.
const LANCZOS3_HALF_WEIGHTS_Q30: [i64; 12] = [
    3_961_179,
    16_166_410,
    -36_505_753,
    -71_551_275,
    145_497_691,
    479_302_660,
    479_302_660,
    145_497_691,
    -71_551_275,
    -36_505_753,
    16_166_410,
    3_961_179,
];
const LANCZOS3_HALF_OFFSETS: [i64; 12] = [-5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6];
// Entry i = floor(IEC_sRGB_to_linear(i / 255) * FIXED_ONE + 0.5).
const SRGB_TO_LINEAR_Q30: [i64; 256] = [
    0, 325910, 651819, 977729, 1303638, 1629548, 1955458, 2281367, 2607277, 2933187, 3259096,
    3593315, 3947620, 4321507, 4715275, 5129215, 5563611, 6018743, 6494885, 6992304, 7511264,
    8052024, 8614838, 9199955, 9807621, 10438078, 11091563, 11768311, 12468553, 13192516, 13940425,
    14712500, 15508959, 16330018, 17175889, 18046782, 18942904, 19864460, 20811652, 21784679,
    22783739, 23809028, 24860738, 25939060, 27044185, 28176297, 29335584, 30522228, 31736409,
    32978309, 34248105, 35545972, 36872087, 38226621, 39609746, 41021632, 42462447, 43932358,
    45431531, 46960130, 48518318, 50106255, 51724102, 53372019, 55050162, 56758688, 58497751,
    60267507, 62068108, 63899705, 65762450, 67656491, 69581978, 71539057, 73527875, 75548577,
    77601309, 79686212, 81803430, 83953105, 86135376, 88350384, 90598267, 92879164, 95193211,
    97540545, 99921301, 102335613, 104783616, 107265443, 109781225, 112331094, 114915182,
    117533617, 120186529, 122874047, 125596298, 128353410, 131145508, 133972719, 136835167,
    139732978, 142666274, 145635179, 148639815, 151680304, 154756767, 157869325, 161018098,
    164203205, 167424766, 170682897, 173977717, 177309344, 180677893, 184083480, 187526222,
    191006232, 194523626, 198078517, 201671019, 205301244, 208969304, 212675312, 216419380,
    220201616, 224022134, 227881041, 231778447, 235714462, 239689194, 243702751, 247755241,
    251846770, 255977446, 260147374, 264356662, 268605413, 272893733, 277221727, 281589499,
    285997153, 290444792, 294932518, 299460435, 304028645, 308637249, 313286350, 317976047,
    322706442, 327477635, 332289726, 337142814, 342036999, 346972379, 351949053, 356967119,
    362026675, 367127818, 372270646, 377455255, 382681741, 387950201, 393260730, 398613424,
    404008378, 409445687, 414925445, 420447746, 426012685, 431620355, 437270848, 442964258,
    448700678, 454480200, 460302916, 466168917, 472078296, 478031143, 484027549, 490067605,
    496151400, 502279026, 508450572, 514666128, 520925782, 527229623, 533577741, 539970223,
    546407158, 552888633, 559414737, 565985557, 572601179, 579261691, 585967179, 592717729,
    599513428, 606354362, 613240616, 620172276, 627149426, 634172152, 641240538, 648354668,
    655514628, 662720500, 669972369, 677270318, 684614430, 692004789, 699441477, 706924577,
    714454171, 722030342, 729653171, 737322740, 745039131, 752802424, 760612702, 768470045,
    776374533, 784326247, 792325268, 800371676, 808465549, 816606969, 824796014, 833032764,
    841317297, 849649694, 858030031, 866458389, 874934845, 883459477, 892032363, 900653581,
    909323208, 918041323, 926808001, 935623320, 944487357, 953400188, 962361890, 971372539,
    980432212, 989540983, 998698930, 1007906126, 1017162649, 1026468572, 1035823971, 1045228922,
    1054683498, 1064187774, 1073741824,
];
// Entry i = ceil(IEC_sRGB_to_linear((i + 0.5) / 255) * FIXED_ONE). The encoded byte is the
// number of thresholds less than or equal to the unpremultiplied Q30 linear sample.
const LINEAR_TO_SRGB_THRESHOLDS_Q30: [i64; 255] = [
    162955, 488865, 814775, 1140684, 1466594, 1792503, 2118413, 2444323, 2770232, 3096142, 3423413,
    3768039, 4132097, 4515888, 4919706, 5343839, 5788569, 6254172, 6740919, 7249076, 7778904,
    8330659, 8904593, 9500955, 10119986, 10761928, 11427015, 12115482, 12827556, 13563464,
    14323428, 15107668, 15916401, 16749839, 17608196, 18491677, 19400491, 20334839, 21294924,
    22280943, 23293093, 24331568, 25396561, 26488261, 27606856, 28752533, 29925475, 31125865,
    32353884, 33609709, 34893519, 36205488, 37545791, 38914599, 40312084, 41738413, 43193756,
    44678277, 46192143, 47735516, 49308558, 50911430, 52544293, 54207303, 55900618, 57624393,
    59378784, 61163943, 62980023, 64827175, 66705550, 68615295, 70556560, 72529490, 74534232,
    76570931, 78639731, 80740774, 82874202, 85040158, 87238780, 89470208, 91734581, 94032036,
    96362709, 98726737, 101124254, 103555395, 106020294, 108519082, 111051891, 113618853,
    116220098, 118855756, 121525955, 124230823, 126970489, 129745078, 132554717, 135399531,
    138279645, 141195183, 144146268, 147133023, 150155571, 153214032, 156308527, 159439178,
    162606103, 165809422, 169049253, 172325714, 175638923, 178988996, 182376050, 185800200,
    189261562, 192760250, 196296378, 199870060, 203481409, 207130538, 210817558, 214542582,
    218305720, 222107084, 225946782, 229824925, 233741622, 237696982, 241691113, 245724123,
    249796119, 253907208, 258057497, 262247092, 266476098, 270744621, 275052765, 279400635,
    283788335, 288215968, 292683638, 297191447, 301739498, 306327892, 310956732, 315626118,
    320336152, 325086933, 329878562, 334711139, 339584763, 344499534, 349455548, 354452906,
    359491705, 364572043, 369694016, 374857722, 380063258, 385310719, 390600201, 395931801,
    401305613, 406721733, 412180255, 417681272, 423224881, 428811173, 434440243, 440112183,
    445827087, 451585046, 457386153, 463230501, 469118179, 475049280, 481023896, 487042115,
    493104030, 499209730, 505359304, 511552844, 517790437, 524072174, 530398142, 536768431,
    543183129, 549642323, 556146102, 562694552, 569287762, 575925819, 582608808, 589336816,
    596109930, 602928236, 609791819, 616700765, 623655160, 630655087, 637700632, 644791880,
    651928915, 659111820, 666340680, 673615579, 680936599, 688303824, 695717337, 703177221,
    710683558, 718236430, 725835919, 733482108, 741175078, 748914910, 756701686, 764535486,
    772416391, 780344482, 788319840, 796342544, 804412675, 812530311, 820695534, 828908421,
    837169053, 845477508, 853833866, 862238203, 870690600, 879191134, 887739884, 896336926,
    904982339, 913676200, 922418587, 931209576, 940049244, 948937669, 957874926, 966861092,
    975896244, 984980456, 994113805, 1003296367, 1012528218, 1021809431, 1031140083, 1040520249,
    1049950002, 1059429419, 1068958573,
];
static NEXT_PARTIAL_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeriveReport {
    pub algorithm: &'static str,
    pub file_count: u64,
    pub png_count: u64,
    pub animation_metadata_count: u64,
    pub copied_count: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub tree_sha256: String,
}

#[derive(Debug)]
struct InputFile {
    normalized: String,
    source: PathBuf,
    length: u64,
    kind: InputKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputKind {
    Png,
    PngMetadata,
    Copy,
}

#[derive(Clone, Copy, Debug, Default)]
struct BuildCounts {
    png: u64,
    animation_metadata: u64,
    copied: u64,
    input_bytes: u64,
    output_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct FixedPremul {
    red: i64,
    green: i64,
    blue: i64,
    alpha: i64,
}

pub fn derive_resource_pack_32x(input: &Path, output: &Path) -> Result<DeriveReport> {
    let input_link_metadata = fs::symlink_metadata(input)
        .with_context(|| format!("cannot inspect input resource pack {}", input.display()))?;
    if input_link_metadata.file_type().is_symlink() || !input_link_metadata.is_dir() {
        bail!(
            "input resource pack must be a real directory, not a link or special file: {}",
            input.display()
        );
    }
    let input_root = fs::canonicalize(input).with_context(|| {
        format!(
            "cannot canonicalize input resource pack {}",
            input.display()
        )
    })?;
    ensure_output_absent(output)?;

    let output_absolute = lexical_absolute(output)?;
    if paths_overlap(&output_absolute, &input_root) {
        bail!(
            "input and output resource-pack trees overlap: {} <-> {}",
            input_root.display(),
            output_absolute.display()
        );
    }

    let output_parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_metadata = fs::metadata(output_parent)
        .with_context(|| format!("cannot inspect output parent {}", output_parent.display()))?;
    if !parent_metadata.is_dir() {
        bail!(
            "output parent is not a directory: {}",
            output_parent.display()
        );
    }
    let canonical_output_parent = fs::canonicalize(output_parent).with_context(|| {
        format!(
            "cannot canonicalize output parent {}",
            output_parent.display()
        )
    })?;
    let canonical_output = canonical_output_parent.join(
        output
            .file_name()
            .ok_or_else(|| anyhow!("output path must name a directory: {}", output.display()))?,
    );
    if paths_overlap(&canonical_output, &input_root) {
        bail!(
            "input and canonical output resource-pack trees overlap: {} <-> {}",
            input_root.display(),
            canonical_output.display()
        );
    }

    let files = collect_files(&input_root)?;
    let partial = create_partial_directory(
        output_parent,
        output
            .file_name()
            .ok_or_else(|| anyhow!("output path must name a directory: {}", output.display()))?,
    )?;

    let build_result = build_partial_tree(&files, &partial);
    let counts = match build_result {
        Ok(counts) => counts,
        Err(error) => return Err(cleanup_after_failure(&partial, error)),
    };
    let tree_sha256 = match directory_tree_v1_sha256(&partial) {
        Ok(hash) => hash,
        Err(error) => return Err(cleanup_after_failure(&partial, error)),
    };

    if let Err(error) = ensure_output_absent(output) {
        return Err(cleanup_after_failure(&partial, error));
    }
    if let Err(error) = atomic_install_noreplace(&partial, output) {
        return Err(cleanup_after_failure(&partial, error));
    }

    Ok(DeriveReport {
        algorithm: ALGORITHM_ID,
        file_count: files.len() as u64,
        png_count: counts.png,
        animation_metadata_count: counts.animation_metadata,
        copied_count: counts.copied,
        input_bytes: counts.input_bytes,
        output_bytes: counts.output_bytes,
        tree_sha256,
    })
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        fn windows_key(path: &Path) -> String {
            let mut value = path.to_string_lossy().replace('/', "\\");
            if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
                value = format!("\\\\{rest}");
            } else if let Some(rest) = value.strip_prefix("\\\\?\\") {
                value = rest.to_owned();
            }
            value.trim_end_matches('\\').to_ascii_lowercase()
        }

        fn contains(ancestor: &str, candidate: &str) -> bool {
            candidate == ancestor
                || candidate
                    .strip_prefix(ancestor)
                    .is_some_and(|suffix| suffix.starts_with('\\'))
        }

        let left = windows_key(left);
        let right = windows_key(right);
        contains(&left, &right) || contains(&right, &left)
    }
    #[cfg(not(windows))]
    {
        left.starts_with(right) || right.starts_with(left)
    }
}

pub fn directory_tree_v1_sha256(root: &Path) -> Result<String> {
    let root_metadata = fs::symlink_metadata(root)
        .with_context(|| format!("cannot inspect directory tree {}", root.display()))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        bail!(
            "directory-tree-v1 root must be a real directory: {}",
            root.display()
        );
    }
    let root = fs::canonicalize(root)
        .with_context(|| format!("cannot canonicalize directory tree {}", root.display()))?;
    let files = collect_files(&root)?;
    let mut digest = Sha256::new();
    digest.update(TREE_HASH_DOMAIN);
    for file in files {
        let path_bytes = file.normalized.as_bytes();
        let content = fs::read(&file.source)
            .with_context(|| format!("cannot read tree file {}", file.source.display()))?;
        digest.update((path_bytes.len() as u64).to_be_bytes());
        digest.update(path_bytes);
        digest.update((content.len() as u64).to_be_bytes());
        digest.update(content);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn ensure_output_absent(output: &Path) -> Result<()> {
    match fs::symlink_metadata(output) {
        Ok(_) => bail!("output path already exists: {}", output.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("cannot inspect output path {}", output.display()))
        }
    }
}

fn lexical_absolute(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("cannot resolve current directory")?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!("path escapes its filesystem root: {}", path.display());
                }
            }
        }
    }
    Ok(normalized)
}

fn collect_files(root: &Path) -> Result<Vec<InputFile>> {
    fn visit(root: &Path, current: &Path, files: &mut Vec<InputFile>) -> Result<()> {
        let entries = fs::read_dir(current)
            .with_context(|| format!("cannot enumerate directory {}", current.display()))?;
        for entry in entries {
            let entry = entry
                .with_context(|| format!("cannot read directory entry in {}", current.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("cannot inspect resource path {}", path.display()))?;
            if file_type.is_symlink() {
                bail!(
                    "resource-pack tree must not contain links: {}",
                    path.display()
                );
            }
            if file_type.is_dir() {
                visit(root, &path, files)?;
                continue;
            }
            if !file_type.is_file() {
                bail!(
                    "resource-pack tree contains a special entry: {}",
                    path.display()
                );
            }

            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("resource path escaped its root: {}", path.display()))?;
            let normalized = normalize_relative_path(relative)?;
            let length = entry
                .metadata()
                .with_context(|| format!("cannot inspect resource file {}", path.display()))?
                .len();
            let is_png = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
            let kind = if is_png {
                InputKind::Png
            } else if normalized.to_ascii_lowercase().ends_with(".png.mcmeta") {
                InputKind::PngMetadata
            } else {
                InputKind::Copy
            };
            files.push(InputFile {
                normalized,
                source: path,
                length,
                kind,
            });
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort_by(|left, right| left.normalized.as_bytes().cmp(right.normalized.as_bytes()));

    let mut folded_paths = BTreeMap::<String, String>::new();
    for file in &files {
        let folded = file.normalized.to_ascii_lowercase();
        if let Some(previous) = folded_paths.insert(folded, file.normalized.clone()) {
            bail!(
                "resource-pack paths collide under ASCII case folding: {previous} and {}",
                file.normalized
            );
        }
    }
    Ok(files)
}

fn normalize_relative_path(path: &Path) -> Result<String> {
    let mut segments = Vec::new();
    for component in path.components() {
        let Component::Normal(segment) = component else {
            bail!(
                "resource path contains a non-normal component: {}",
                path.display()
            );
        };
        let segment = segment
            .to_str()
            .ok_or_else(|| anyhow!("resource-pack paths must be UTF-8: {}", path.display()))?;
        if segment.contains('/') || segment.contains('\\') || segment.is_empty() {
            bail!("resource path segment is not portable: {}", path.display());
        }
        segments.push(segment);
    }
    if segments.is_empty() {
        bail!(
            "resource file has an empty relative path: {}",
            path.display()
        );
    }
    Ok(segments.join("/"))
}

fn create_partial_directory(parent: &Path, output_name: &std::ffi::OsStr) -> Result<PathBuf> {
    for _ in 0..128 {
        let id = NEXT_PARTIAL_ID.fetch_add(1, Ordering::Relaxed);
        let mut partial_name = OsString::from(".");
        partial_name.push(output_name);
        partial_name.push(format!(".partial-{}-{id}", std::process::id()));
        let partial = parent.join(partial_name);
        match fs::create_dir(&partial) {
            Ok(()) => return Ok(partial),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("cannot create partial output tree {}", partial.display())
                });
            }
        }
    }
    bail!(
        "cannot allocate a unique partial output directory beside {}",
        parent.display()
    )
}

fn build_partial_tree(files: &[InputFile], partial: &Path) -> Result<BuildCounts> {
    let mut counts = BuildCounts::default();
    let png_metadata: BTreeMap<_, _> = files
        .iter()
        .filter(|file| file.kind == InputKind::PngMetadata)
        .map(|file| (file.normalized.to_ascii_lowercase(), file.source.as_path()))
        .collect();
    for file in files {
        counts.input_bytes = counts
            .input_bytes
            .checked_add(file.length)
            .ok_or_else(|| anyhow!("input byte count overflow"))?;
        let destination = file
            .normalized
            .split('/')
            .fold(partial.to_path_buf(), |path, segment| path.join(segment));
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create output directory {}", parent.display()))?;
        }
        match file.kind {
            InputKind::Png => {
                let metadata_key = format!("{}.mcmeta", file.normalized).to_ascii_lowercase();
                downsample_png(
                    &file.source,
                    &destination,
                    &file.normalized,
                    png_metadata.get(&metadata_key).copied(),
                )?;
                counts.png += 1;
            }
            InputKind::PngMetadata => {
                if transform_png_metadata(&file.source, &destination, &file.normalized)? {
                    counts.animation_metadata += 1;
                } else {
                    counts.copied += 1;
                }
            }
            InputKind::Copy => {
                copy_exact(file, &destination)?;
                counts.copied += 1;
            }
        }
        let written = fs::metadata(&destination)
            .with_context(|| format!("cannot inspect output file {}", destination.display()))?
            .len();
        counts.output_bytes = counts
            .output_bytes
            .checked_add(written)
            .ok_or_else(|| anyhow!("output byte count overflow"))?;
    }
    Ok(counts)
}

fn copy_exact(file: &InputFile, destination: &Path) -> Result<()> {
    let copied = fs::copy(&file.source, destination).with_context(|| {
        format!(
            "cannot copy non-PNG resource {} to {}",
            file.source.display(),
            destination.display()
        )
    })?;
    if copied != file.length {
        bail!(
            "short copy for {}: expected {} bytes, wrote {copied}",
            file.normalized,
            file.length
        );
    }
    Ok(())
}

fn transform_png_metadata(source: &Path, destination: &Path, normalized: &str) -> Result<bool> {
    let source_bytes = fs::read(source)
        .with_context(|| format!("cannot read PNG metadata resource {normalized}"))?;
    let mut metadata: serde_json::Value = serde_json::from_slice(&source_bytes)
        .with_context(|| format!("PNG metadata is not valid JSON: {normalized}"))?;
    let Some(animation) = metadata.get_mut("animation") else {
        fs::write(destination, source_bytes)
            .with_context(|| format!("cannot copy PNG metadata resource {normalized}"))?;
        return Ok(false);
    };
    let animation = animation
        .as_object_mut()
        .ok_or_else(|| anyhow!("animation metadata must be an object: {normalized}"))?;
    let mut changed = false;
    for dimension_name in ["width", "height"] {
        let Some(dimension) = animation.get_mut(dimension_name) else {
            continue;
        };
        let value = dimension.as_u64().ok_or_else(|| {
            anyhow!("animation.{dimension_name} must be a positive even integer: {normalized}")
        })?;
        if value == 0 || value % 2 != 0 {
            bail!(
                "animation.{dimension_name} must be a positive even integer: {normalized} has {value}"
            );
        }
        *dimension = serde_json::Value::from(value / 2);
        changed = true;
    }
    if !changed {
        fs::write(destination, source_bytes)
            .with_context(|| format!("cannot copy PNG metadata resource {normalized}"))?;
        return Ok(false);
    }

    let mut output = serde_json::to_vec(&metadata)
        .with_context(|| format!("cannot serialize derived PNG metadata {normalized}"))?;
    output.push(b'\n');
    fs::write(destination, output)
        .with_context(|| format!("cannot write derived PNG metadata {normalized}"))?;
    Ok(true)
}

fn cleanup_after_failure(partial: &Path, error: anyhow::Error) -> anyhow::Error {
    match fs::remove_dir_all(partial) {
        Ok(()) => error,
        Err(cleanup_error) if cleanup_error.kind() == std::io::ErrorKind::NotFound => error,
        Err(cleanup_error) => error.context(format!(
            "also failed to remove partial output tree {}: {cleanup_error}",
            partial.display()
        )),
    }
}

fn atomic_install_noreplace(partial: &Path, output: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        let existing: Vec<u16> = partial.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = output.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: both buffers are NUL-terminated and live for the duration of the call. MoveFileW
        // has no replace flag, so an existing destination is an atomic failure.
        if unsafe { MoveFileW(existing.as_ptr(), destination.as_ptr()) } == 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!(
                    "cannot atomically install derived resource pack without replacing {}",
                    output.display()
                )
            });
        }
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;

        const AT_FDCWD: i32 = -100;
        const RENAME_NOREPLACE: u32 = 1;
        let existing = std::ffi::CString::new(partial.as_os_str().as_bytes())
            .context("partial output path contains NUL")?;
        let destination = std::ffi::CString::new(output.as_os_str().as_bytes())
            .context("final output path contains NUL")?;
        // SAFETY: C strings are NUL-terminated and valid for the call. renameat2 with
        // RENAME_NOREPLACE is the Linux atomic no-clobber primitive.
        if unsafe {
            renameat2(
                AT_FDCWD,
                existing.as_ptr(),
                AT_FDCWD,
                destination.as_ptr(),
                RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!(
                    "cannot atomically install derived resource pack without replacing {}",
                    output.display()
                )
            });
        }
        return Ok(());
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = partial;
        let _ = output;
        bail!("atomic no-replace resource-pack installation is unsupported on this platform")
    }
}

fn downsample_png(
    source: &Path,
    destination: &Path,
    normalized: &str,
    metadata_source: Option<&Path>,
) -> Result<()> {
    let reader = ImageReader::open(source)
        .with_context(|| format!("cannot open PNG resource {normalized}"))?
        .with_guessed_format()
        .with_context(|| format!("cannot inspect PNG format for {normalized}"))?;
    if reader.format() != Some(ImageFormat::Png) {
        bail!("resource with .png extension is not PNG data: {normalized}");
    }
    let decoded = reader
        .decode()
        .with_context(|| format!("cannot decode PNG resource {normalized}"))?;
    if !matches!(
        decoded,
        image::DynamicImage::ImageLuma8(_)
            | image::DynamicImage::ImageLumaA8(_)
            | image::DynamicImage::ImageRgb8(_)
            | image::DynamicImage::ImageRgba8(_)
    ) {
        bail!("PNG resource must decode to 8-bit integer samples without truncation: {normalized}");
    }
    let image = decoded.to_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
        bail!(
            "PNG dimensions must be positive and even for 2:1 derivation: {normalized} is {width}x{height}"
        );
    }
    let frame_size = animation_frame_size(metadata_source, width, height, normalized)?;
    let output = if let Some((frame_width, frame_height)) = frame_size {
        resize_animation_frames_half(&image, frame_width, frame_height)
    } else {
        resize_half_fixed_premultiplied(&image)
    };
    let destination_file = fs::File::create(destination)
        .with_context(|| format!("cannot create derived PNG {}", destination.display()))?;
    PngEncoder::new_with_quality(
        BufWriter::new(destination_file),
        CompressionType::Best,
        FilterType::NoFilter,
    )
    .write_image(
        output.as_raw(),
        output.width(),
        output.height(),
        ColorType::Rgba8.into(),
    )
    .with_context(|| format!("cannot encode derived PNG {normalized}"))?;
    Ok(())
}

fn animation_frame_size(
    metadata_source: Option<&Path>,
    image_width: u32,
    image_height: u32,
    normalized: &str,
) -> Result<Option<(u32, u32)>> {
    let Some(metadata_source) = metadata_source else {
        return Ok(None);
    };
    let metadata_bytes = fs::read(metadata_source).with_context(|| {
        format!(
            "cannot read PNG metadata for animation grid: {}",
            metadata_source.display()
        )
    })?;
    let metadata: serde_json::Value = serde_json::from_slice(&metadata_bytes)
        .with_context(|| format!("PNG metadata is not valid JSON: {normalized}.mcmeta"))?;
    let Some(animation) = metadata.get("animation") else {
        return Ok(None);
    };
    let animation = animation
        .as_object()
        .ok_or_else(|| anyhow!("animation metadata must be an object: {normalized}.mcmeta"))?;
    let dimension = |name: &str| -> Result<Option<u32>> {
        let Some(value) = animation.get(name) else {
            return Ok(None);
        };
        let value = value.as_u64().ok_or_else(|| {
            anyhow!("animation.{name} must be a positive integer: {normalized}.mcmeta")
        })?;
        if value == 0 || value > u64::from(u32::MAX) {
            bail!("animation.{name} is out of range: {normalized}.mcmeta has {value}");
        }
        Ok(Some(value as u32))
    };
    let explicit_width = dimension("width")?;
    let explicit_height = dimension("height")?;
    if explicit_width.is_some_and(|width| width % 2 != 0) {
        bail!("animation.width must be even for 2:1 derivation: {normalized}.mcmeta");
    }
    if explicit_height.is_some_and(|height| height % 2 != 0) {
        bail!("animation.height must be even for 2:1 derivation: {normalized}.mcmeta");
    }
    let (frame_width, frame_height) = match (explicit_width, explicit_height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, image_height),
        (None, Some(height)) => (image_width, height),
        (None, None) => {
            let minimum = image_width.min(image_height);
            (minimum, minimum)
        }
    };
    if frame_width % 2 != 0 || frame_height % 2 != 0 {
        bail!(
            "animation frame dimensions must be even for 2:1 derivation: {normalized} uses {frame_width}x{frame_height}"
        );
    }
    if image_width % frame_width != 0 || image_height % frame_height != 0 {
        bail!(
            "PNG dimensions must be multiples of the animation frame grid: {normalized} is {image_width}x{image_height}, frame {frame_width}x{frame_height}"
        );
    }
    Ok(Some((frame_width, frame_height)))
}

fn resize_animation_frames_half(
    source: &RgbaImage,
    frame_width: u32,
    frame_height: u32,
) -> RgbaImage {
    let mut output = RgbaImage::new(source.width() / 2, source.height() / 2);
    for frame_y in (0..source.height()).step_by(frame_height as usize) {
        for frame_x in (0..source.width()).step_by(frame_width as usize) {
            let frame =
                image::imageops::crop_imm(source, frame_x, frame_y, frame_width, frame_height)
                    .to_image();
            let resized = resize_half_fixed_premultiplied(&frame);
            image::imageops::replace(
                &mut output,
                &resized,
                i64::from(frame_x / 2),
                i64::from(frame_y / 2),
            );
        }
    }
    output
}

fn resize_half_fixed_premultiplied(source: &RgbaImage) -> RgbaImage {
    let source_width = source.width() as usize;
    let source_height = source.height() as usize;
    let output_width = source_width / 2;
    let output_height = source_height / 2;
    let source_fixed: Vec<_> = source
        .pixels()
        .map(|pixel| {
            let alpha = round_div_i128(i128::from(pixel[3]) * i128::from(FIXED_ONE), 255);
            FixedPremul {
                red: fixed_multiply(SRGB_TO_LINEAR_Q30[pixel[0] as usize], alpha),
                green: fixed_multiply(SRGB_TO_LINEAR_Q30[pixel[1] as usize], alpha),
                blue: fixed_multiply(SRGB_TO_LINEAR_Q30[pixel[2] as usize], alpha),
                alpha,
            }
        })
        .collect();

    let mut horizontal = vec![FixedPremul::default(); output_width * source_height];
    for y in 0..source_height {
        for output_x in 0..output_width {
            let mut red = 0_i128;
            let mut green = 0_i128;
            let mut blue = 0_i128;
            let mut alpha = 0_i128;
            for (&offset, &weight) in LANCZOS3_HALF_OFFSETS.iter().zip(&LANCZOS3_HALF_WEIGHTS_Q30) {
                let source_x =
                    (output_x as i64 * 2 + offset).clamp(0, source_width as i64 - 1) as usize;
                let sample = source_fixed[y * source_width + source_x];
                red += i128::from(sample.red) * i128::from(weight);
                green += i128::from(sample.green) * i128::from(weight);
                blue += i128::from(sample.blue) * i128::from(weight);
                alpha += i128::from(sample.alpha) * i128::from(weight);
            }
            horizontal[y * output_width + output_x] = FixedPremul {
                red: round_div_i128(red, i128::from(FIXED_ONE)),
                green: round_div_i128(green, i128::from(FIXED_ONE)),
                blue: round_div_i128(blue, i128::from(FIXED_ONE)),
                alpha: round_div_i128(alpha, i128::from(FIXED_ONE)),
            };
        }
    }

    let mut output = RgbaImage::new(output_width as u32, output_height as u32);
    for output_y in 0..output_height {
        for x in 0..output_width {
            let mut red = 0_i128;
            let mut green = 0_i128;
            let mut blue = 0_i128;
            let mut alpha = 0_i128;
            for (&offset, &weight) in LANCZOS3_HALF_OFFSETS.iter().zip(&LANCZOS3_HALF_WEIGHTS_Q30) {
                let source_y =
                    (output_y as i64 * 2 + offset).clamp(0, source_height as i64 - 1) as usize;
                let sample = horizontal[source_y * output_width + x];
                red += i128::from(sample.red) * i128::from(weight);
                green += i128::from(sample.green) * i128::from(weight);
                blue += i128::from(sample.blue) * i128::from(weight);
                alpha += i128::from(sample.alpha) * i128::from(weight);
            }
            output.put_pixel(
                x as u32,
                output_y as u32,
                encode_fixed_sample(FixedPremul {
                    red: round_div_i128(red, i128::from(FIXED_ONE)),
                    green: round_div_i128(green, i128::from(FIXED_ONE)),
                    blue: round_div_i128(blue, i128::from(FIXED_ONE)),
                    alpha: round_div_i128(alpha, i128::from(FIXED_ONE)),
                }),
            );
        }
    }
    output
}

fn fixed_multiply(left: i64, right: i64) -> i64 {
    round_div_i128(i128::from(left) * i128::from(right), i128::from(FIXED_ONE))
}

fn round_div_i128(numerator: i128, denominator: i128) -> i64 {
    debug_assert!(denominator > 0);
    let rounded = if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        -((-numerator + denominator / 2) / denominator)
    };
    i64::try_from(rounded).expect("fixed-point intermediate fits i64")
}

fn encode_fixed_sample(sample: FixedPremul) -> image::Rgba<u8> {
    let alpha = sample.alpha.clamp(0, FIXED_ONE);
    if alpha == 0 {
        return image::Rgba([0, 0, 0, 0]);
    }
    let unpremultiply = |component: i64| {
        round_div_i128(
            i128::from(component.clamp(0, alpha)) * i128::from(FIXED_ONE),
            i128::from(alpha),
        )
        .clamp(0, FIXED_ONE)
    };
    image::Rgba([
        linear_fixed_to_srgb_u8(unpremultiply(sample.red)),
        linear_fixed_to_srgb_u8(unpremultiply(sample.green)),
        linear_fixed_to_srgb_u8(unpremultiply(sample.blue)),
        round_div_i128(i128::from(alpha) * 255, i128::from(FIXED_ONE)) as u8,
    ])
}

fn linear_fixed_to_srgb_u8(linear: i64) -> u8 {
    LINEAR_TO_SRGB_THRESHOLDS_Q30.partition_point(|threshold| linear >= *threshold) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_install_never_replaces_an_existing_destination() {
        let root = std::env::temp_dir().join(format!(
            "lba-resource-pack-atomic-install-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let partial = root.join("partial");
        let destination = root.join("destination");
        fs::create_dir_all(&partial).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(partial.join("new"), b"new").unwrap();

        let error = atomic_install_noreplace(&partial, &destination)
            .expect_err("existing destination must never be replaced")
            .to_string();

        assert!(destination.exists());
        assert!(!destination.join("new").exists());
        assert!(partial.join("new").exists());
        assert!(!error.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fixed_tables_are_normalized_monotonic_and_cover_both_endpoints() {
        assert_eq!(LANCZOS3_HALF_WEIGHTS_Q30.iter().sum::<i64>(), FIXED_ONE);
        assert_eq!(SRGB_TO_LINEAR_Q30[0], 0);
        assert_eq!(SRGB_TO_LINEAR_Q30[255], FIXED_ONE);
        assert!(SRGB_TO_LINEAR_Q30.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            LINEAR_TO_SRGB_THRESHOLDS_Q30
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(LINEAR_TO_SRGB_THRESHOLDS_Q30[0] > 0);
        assert!(LINEAR_TO_SRGB_THRESHOLDS_Q30[254] < FIXED_ONE);
    }
}
