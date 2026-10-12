use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::Arc;
use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_BROKEN_PIPE, ERROR_NO_DATA,
    ERROR_IO_PENDING, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_GENERIC_WRITE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{CreateEventW, GetCurrentProcess, INFINITE, WaitForSingleObject};

const PIPE_BUFFER_BYTES: u32 = 64 * 1024;

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

#[derive(Debug)]
struct OwnedPipe(HANDLE);

unsafe impl Send for OwnedPipe {}
unsafe impl Sync for OwnedPipe {}

impl Drop for OwnedPipe {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn new_overlapped() -> io::Result<(OwnedPipe, OVERLAPPED)> {
    let event = unsafe { CreateEventW(null(), 1, 0, null()) };
    if event.is_null() {
        return Err(io::Error::last_os_error());
    }
    let event = OwnedPipe(event);
    let mut overlapped = unsafe { std::mem::zeroed::<OVERLAPPED>() };
    overlapped.hEvent = event.0;
    Ok((event, overlapped))
}

fn finish_overlapped(
    handle: HANDLE,
    overlapped: &mut OVERLAPPED,
    start_error: Option<io::Error>,
) -> io::Result<u32> {
    if let Some(error) = start_error {
        if error.raw_os_error().map(|value| value as u32) != Some(ERROR_IO_PENDING) {
            return Err(error);
        }
        let wait = unsafe { WaitForSingleObject(overlapped.hEvent, INFINITE) };
        if wait != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
    }
    let mut transferred = 0_u32;
    let completed = unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, 0) };
    if completed == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(transferred)
}

#[derive(Debug)]
pub struct NamedPipeStream {
    handle: Arc<OwnedPipe>,
}

unsafe impl Send for NamedPipeStream {}
unsafe impl Sync for NamedPipeStream {}

impl NamedPipeStream {
    pub fn connect(pipe_name: &OsStr) -> io::Result<Self> {
        let pipe_name = wide(pipe_name);
        loop {
            let handle = unsafe {
                CreateFileW(
                    pipe_name.as_ptr(),
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    0,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(Self {
                    handle: Arc::new(OwnedPipe(handle)),
                });
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error().map(|value| value as u32) != Some(ERROR_PIPE_BUSY) {
                return Err(error);
            }
            let waited = unsafe { WaitNamedPipeW(pipe_name.as_ptr(), 5_000) };
            if waited == 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }

    /// Creates a distinct Windows handle for the same duplex pipe instance.
    ///
    /// Sharing one synchronous handle between a blocking reader and a writer
    /// serializes their I/O inside Windows. A pending `ReadFile` can therefore
    /// prevent the writer from sending the response that would satisfy that
    /// read. Every reader/writer pair must use duplicated handles instead.
    pub fn try_clone(&self) -> io::Result<Self> {
        let process = unsafe { GetCurrentProcess() };
        let mut duplicated = INVALID_HANDLE_VALUE;
        let result = unsafe {
            DuplicateHandle(
                process,
                self.handle.0,
                process,
                &mut duplicated,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle: Arc::new(OwnedPipe(duplicated)),
        })
    }

    fn read_with_pending_hook<F>(&mut self, buffer: &mut [u8], pending_hook: F) -> io::Result<usize>
    where
        F: FnOnce(),
    {
        if buffer.is_empty() {
            return Ok(0);
        }
        let (_event, mut overlapped) = new_overlapped()?;
        let result = unsafe {
            ReadFile(
                self.handle.0,
                buffer.as_mut_ptr(),
                buffer.len().min(u32::MAX as usize) as u32,
                null_mut(),
                &mut overlapped,
            )
        };
        let start_error = (result == 0).then(io::Error::last_os_error);
        match start_error
            .as_ref()
            .and_then(|error| error.raw_os_error())
            .map(|value| value as u32)
        {
            Some(ERROR_IO_PENDING) => {
                pending_hook();
            }
            Some(ERROR_BROKEN_PIPE) | Some(ERROR_NO_DATA) => return Ok(0),
            Some(_) => return Err(start_error.expect("failed ReadFile must include its error")),
            None => {}
        }
        match finish_overlapped(self.handle.0, &mut overlapped, start_error) {
            Ok(read) => Ok(read as usize),
            Err(error)
                if matches!(
                    error.raw_os_error().map(|value| value as u32),
                    Some(ERROR_BROKEN_PIPE) | Some(ERROR_NO_DATA)
                ) =>
            {
                Ok(0)
            }
            Err(error) => Err(error),
        }
    }
}

impl Read for NamedPipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.read_with_pending_hook(buffer, || {})
    }
}

impl Write for NamedPipeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let (_event, mut overlapped) = new_overlapped()?;
        let result = unsafe {
            WriteFile(
                self.handle.0,
                buffer.as_ptr(),
                buffer.len().min(u32::MAX as usize) as u32,
                null_mut(),
                &mut overlapped,
            )
        };
        let start_error = (result == 0).then(io::Error::last_os_error);
        finish_overlapped(self.handle.0, &mut overlapped, start_error)
            .map(|written| written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct NamedPipeListener {
    pipe_name: Vec<u16>,
}

impl NamedPipeListener {
    pub fn bind(pipe_name: &OsStr) -> io::Result<Self> {
        let pipe_name = wide(pipe_name);
        if pipe_name.len() <= 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "named pipe path must not be empty",
            ));
        }
        Ok(Self { pipe_name })
    }

    pub fn accept(&self) -> io::Result<NamedPipeStream> {
        let handle = unsafe {
            CreateNamedPipeW(
                self.pipe_name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                PIPE_BUFFER_BYTES,
                PIPE_BUFFER_BYTES,
                0,
                null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let (_event, mut overlapped) = match new_overlapped() {
            Ok(value) => value,
            Err(error) => {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                return Err(error);
            }
        };
        let connected = unsafe { ConnectNamedPipe(handle, &mut overlapped) };
        if connected == 0 {
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|value| value as u32) {
                Some(ERROR_PIPE_CONNECTED) => {}
                Some(ERROR_IO_PENDING) => {
                    if let Err(error) = finish_overlapped(handle, &mut overlapped, Some(error)) {
                        unsafe {
                            let _ = CloseHandle(handle);
                        }
                        return Err(error);
                    }
                }
                _ => {
                    unsafe {
                        let _ = CloseHandle(handle);
                    }
                    return Err(error);
                }
            }
        }
        Ok(NamedPipeStream {
            handle: Arc::new(OwnedPipe(handle)),
        })
    }
}

impl Drop for NamedPipeListener {
    fn drop(&mut self) {}
}

pub fn disconnect(stream: &NamedPipeStream) {
    unsafe {
        let _ = DisconnectNamedPipe(stream.handle.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn duplicated_handles_allow_a_response_while_the_reader_waits_for_the_next_request() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pipe_name = format!(
            r"\\.\pipe\nova-render-duplex-test-{}-{nonce}",
            std::process::id()
        );
        let listener = NamedPipeListener::bind(OsStr::new(&pipe_name)).unwrap();
        let (server_ready_tx, server_ready_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut reader = listener.accept().unwrap();
            let mut hello = [0_u8; 5];
            reader.read_exact(&mut hello).unwrap();
            assert_eq!(&hello, b"hello");
            let mut writer = reader.try_clone().unwrap();
            let (pending_tx, pending_rx) = mpsc::channel();
            let next_request = thread::spawn(move || {
                let mut next = [0_u8; 4];
                let read = reader
                    .read_with_pending_hook(&mut next, || pending_tx.send(()).unwrap())
                    .unwrap();
                assert_eq!(read, next.len());
                next
            });
            pending_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("server did not enter a pending next-request ReadFile");
            server_ready_tx.send(()).unwrap();
            writer.write_all(b"accepted").unwrap();
            assert_eq!(&next_request.join().unwrap(), b"next");
        });

        let started = Instant::now();
        let mut client = loop {
            match NamedPipeStream::connect(OsStr::new(&pipe_name)) {
                Ok(client) => break client,
                Err(_) if started.elapsed() < Duration::from_secs(2) => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("connect test pipe: {error}"),
            }
        };
        client.write_all(b"hello").unwrap();
        server_ready_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("server reader did not enter its next request wait");
        let mut client_reader = client.try_clone().unwrap();
        let (accepted_tx, accepted_rx) = mpsc::channel();
        let accepted_reader = thread::spawn(move || {
            let mut accepted = [0_u8; 8];
            let result = client_reader.read_exact(&mut accepted).map(|()| accepted);
            accepted_tx.send(result).unwrap();
        });
        let accepted = accepted_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("server response was blocked behind its pending request read")
            .unwrap();
        assert_eq!(&accepted, b"accepted");
        client.write_all(b"next").unwrap();
        accepted_reader.join().unwrap();
        server.join().unwrap();
    }
}
