use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Thin filesystem wrapper retained for cache and pack-loader type contracts.
///
/// It deliberately performs no global sampling, sleeping, duty cycling, or
/// request-size limiting. Product-side pressure control belongs in bounded
/// producers, queues, and streamed encoders rather than around every syscall.
pub(crate) struct ProcessIo<T> {
    inner: T,
}

impl<T> ProcessIo<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }

    pub(crate) const fn get_ref(&self) -> &T {
        &self.inner
    }
}

impl ProcessIo<File> {
    pub(crate) fn metadata(&self) -> io::Result<Metadata> {
        self.inner.metadata()
    }

    pub(crate) fn sync_all(&self) -> io::Result<()> {
        self.inner.sync_all()
    }
}

impl<T: Read> Read for ProcessIo<T> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.inner.read(output)
    }
}

impl<T: Write> Write for ProcessIo<T> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        self.inner.write(input)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<T: Seek> Seek for ProcessIo<T> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

pub(crate) fn wrap_cache_file(file: File) -> ProcessIo<File> {
    ProcessIo::new(file)
}

pub(crate) fn wrap_process_io<T>(inner: T) -> ProcessIo<T> {
    ProcessIo::new(inner)
}

pub(crate) fn run_process_io<T>(operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    operation()
}

pub(crate) fn open_cache_read_file(path: &Path) -> io::Result<ProcessIo<File>> {
    open_process_read_file(path)
}

pub(crate) fn open_process_read_file(path: &Path) -> io::Result<ProcessIo<File>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        // Growing cache readers retain the same file object across the final
        // atomic rename. Make that Windows lifetime contract explicit instead
        // of relying on OpenOptions' platform default share mask.
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
        options.custom_flags(FILE_FLAG_SEQUENTIAL_SCAN);
    }
    options.open(path).map(ProcessIo::new)
}

fn open_process_create_new_file(path: &Path) -> io::Result<ProcessIo<File>> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(ProcessIo::new)
}

/// Replaces a small process-coordination file with the latest observable state.
///
/// Unlike cache payload publication this deliberately avoids `sync_all`: the
/// file is a live heartbeat, not durable user data, and forcing storage on every
/// update would turn progress reporting into avoidable device latency.
pub(crate) fn write_process_state(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.flush()
}

pub(crate) fn read_process_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = open_process_read_file(path)?;
    let expected = file.metadata()?.len();
    let mut bytes = Vec::new();
    if let Ok(capacity) = usize::try_from(expected) {
        bytes.try_reserve_exact(capacity).map_err(|source| {
            io::Error::other(format!(
                "cannot reserve {capacity} bytes while reading {}: {source}",
                path.display()
            ))
        })?;
    }
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

