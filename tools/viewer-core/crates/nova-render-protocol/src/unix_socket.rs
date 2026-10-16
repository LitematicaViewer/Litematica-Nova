//! Local duplex transport with the existing named-pipe API on Unix.
//! A logical endpoint maps to a short, user-owned socket under /tmp. The
//! listener exclusively creates its directory; it never reclaims stale or
//! other-instance endpoints. Hello/token authentication remains above this layer.
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{self, Metadata};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

fn endpoint_path(name: &OsStr) -> io::Result<PathBuf> {
    let bytes = name.as_bytes();
    if !bytes.starts_with(br"\\.\pipe\") || bytes.len() > 4096 || bytes.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid local IPC endpoint name"));
    }
    let hash = format!("{:x}", Sha256::digest(bytes));
    // macOS's usual per-user temp directory is too long for sockaddr_un.
    Ok(PathBuf::from(format!("/tmp/nova-ipc-{}-{}", unsafe { libc::geteuid() }, &hash[..32])).join("s"))
}

fn validate_private_directory(path: &Path) -> io::Result<Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "local IPC directory is not private and owned by this user"));
    }
    Ok(metadata)
}

#[derive(Debug)]
struct EndpointOwner {
    directory: PathBuf,
    directory_identity: (u64, u64),
    socket_identity: Option<(u64, u64)>,
}

impl Drop for EndpointOwner {
    fn drop(&mut self) {
        let Ok(metadata) = validate_private_directory(&self.directory) else { return; };
        if (metadata.dev(), metadata.ino()) != self.directory_identity { return; }
        let path = self.directory.join("s");
        if let (Some(expected), Ok(metadata)) = (self.socket_identity, fs::symlink_metadata(&path)) {
            if metadata.file_type().is_socket() && (metadata.dev(), metadata.ino()) == expected {
                let _ = fs::remove_file(path);
            }
        }
        // Never recurse: another file or replaced socket is not ours to remove.
        let _ = fs::remove_dir(&self.directory);
    }
}

#[derive(Debug)]
pub struct NamedPipeStream {
    stream: UnixStream,
}

impl NamedPipeStream {
    pub fn connect(name: &OsStr) -> io::Result<Self> {
        let path = endpoint_path(name)?;
        validate_private_directory(path.parent().unwrap())?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "local IPC socket is not private and owned by this user"));
        }
        UnixStream::connect(&path).map(|stream| Self { stream })
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        self.stream.try_clone().map(|stream| Self { stream })
    }
}

impl Read for NamedPipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> { self.stream.read(buffer) }
}

impl Write for NamedPipeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> { self.stream.write(buffer) }
    fn flush(&mut self) -> io::Result<()> { self.stream.flush() }
}

pub struct NamedPipeListener {
    listener: UnixListener,
    _owner: EndpointOwner,
}

impl NamedPipeListener {
    pub fn bind(name: &OsStr) -> io::Result<Self> {
        let path = endpoint_path(name)?;
        let directory = path.parent().unwrap();
        // Atomic ownership acquisition, not create_dir_all/chmod/reuse.
        fs::DirBuilder::new().mode(0o700).create(directory).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                io::Error::new(io::ErrorKind::AddrInUse, "local IPC endpoint is already owned; it was not removed")
            } else { error }
        })?;
        let metadata = validate_private_directory(directory)?;
        let mut owner = EndpointOwner {
            directory: directory.to_owned(), directory_identity: (metadata.dev(), metadata.ino()), socket_identity: None,
        };
        let listener = UnixListener::bind(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        owner.socket_identity = Some((metadata.dev(), metadata.ino()));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        Ok(Self { listener, _owner: owner })
    }

    pub fn accept(&self) -> io::Result<NamedPipeStream> {
        self.listener.accept().map(|(stream, _)| NamedPipeStream { stream })
    }
}

pub fn disconnect(stream: &NamedPipeStream) {
    // Shutdown affects cloned descriptors too, waking blocked local/peer reads.
    let _ = stream.stream.shutdown(Shutdown::Both);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn name(label: &str) -> String {
        format!(r"\\.\pipe\nova-unix-{label}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())
    }

    #[test]
    fn unix_endpoint_is_short_private_and_removed_by_owner_only() {
        let name = name("private");
        let path = endpoint_path(OsStr::new(&name)).unwrap();
        let listener = NamedPipeListener::bind(OsStr::new(&name)).unwrap();
        assert!(path.as_os_str().as_bytes().len() < 100);
        assert_eq!(fs::symlink_metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(validate_private_directory(path.parent().unwrap()).unwrap().mode() & 0o777, 0o700);
        assert!(matches!(NamedPipeListener::bind(OsStr::new(&name)), Err(error) if error.kind() == io::ErrorKind::AddrInUse));
        assert!(path.is_file() || fs::symlink_metadata(&path).unwrap().file_type().is_socket());
        drop(listener);
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn unix_duplex_clone_sends_while_reader_blocks_and_disconnect_wakes() {
        let name = name("duplex");
        let listener = NamedPipeListener::bind(OsStr::new(&name)).unwrap();
        let mut client = NamedPipeStream::connect(OsStr::new(&name)).unwrap();
        let mut server = listener.accept().unwrap();
        let mut writer = server.try_clone().unwrap();
        let control = server.try_clone().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || { let mut buffer = [0; 1]; tx.send(server.read(&mut buffer)).unwrap(); });
        writer.write_all(b"actual duplex").unwrap();
        let mut bytes = [0; 13];
        client.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"actual duplex");
        disconnect(&control);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap(), 0);
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
        reader.join().unwrap();
    }

    #[test]
    fn unix_cleanup_preserves_replaced_socket_and_directory() {
        let name = name("replacement");
        let path = endpoint_path(OsStr::new(&name)).unwrap();
        let listener = NamedPipeListener::bind(OsStr::new(&name)).unwrap();
        fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        drop(listener);
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_socket());
        drop(replacement);
        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn unix_transport_preserves_existing_hello_framing() {
        let name = name("hello");
        let listener = NamedPipeListener::bind(OsStr::new(&name)).unwrap();
        let mut client = NamedPipeStream::connect(OsStr::new(&name)).unwrap();
        let mut server = listener.accept().unwrap();
        let hello = crate::Envelope::new(0, crate::Payload::Hello(crate::ClientHello {
            protocol_version: crate::PROTOCOL_VERSION, auth_token: "fixture-token-never-trusted-by-transport".into(),
            role: crate::ClientRole::ApplicationHost, instance_id: "fixture-host".into(), pid: std::process::id(), application_owner_epoch: Some(19),
        }));
        crate::write_envelope(&mut client, &hello).unwrap();
        let received = crate::read_envelope(&mut server).unwrap().unwrap();
        let crate::Payload::Hello(received) = received.payload else { panic!("Hello was not preserved"); };
        assert_eq!(received.application_owner_epoch, Some(19));
        assert_eq!(received.protocol_version, crate::PROTOCOL_VERSION);
        assert_eq!(crate::STREAM_WINDOW_MAX_INFLIGHT_SECTIONS, 3);
    }
}
