//! The transport seam: moves length-prefixed message frames over a per-user
//! channel (Unix domain socket on POSIX, named pipe on Windows). Payload bytes
//! are opaque; the JSON envelope lives one layer up. Async (tokio).

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};

use crate::errors::IpcError;

// Cap frame size so a desynced peer fails fast instead of making us allocate
// gigabytes from a bogus length prefix.
const MAX_FRAME: u32 = 16 * 1024 * 1024;

/// Any full-duplex byte stream we can frame over.
pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

type BoxStream = Box<dyn Stream>;

/// The read half of a framed connection, driven by a single reader.
pub struct FrameReader {
    inner: ReadHalf<BoxStream>,
}

/// The write half. Send is serialized by the caller holding `&mut self`.
pub struct FrameWriter {
    inner: WriteHalf<BoxStream>,
}

impl FrameReader {
    /// Block for the next inbound frame; `Ok(None)` once the peer closes.
    pub async fn recv(&mut self) -> Result<Option<String>, IpcError> {
        let mut len_buf = [0u8; 4];
        match self.inner.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_be_bytes(len_buf);
        if len > MAX_FRAME {
            return Err(IpcError::FrameTooLarge(len));
        }
        if len == 0 {
            return Ok(Some(String::new()));
        }
        let mut buf = vec![0u8; len as usize];
        match self.inner.read_exact(&mut buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        String::from_utf8(buf)
            .map(Some)
            .map_err(|e| IpcError::Malformed(e.to_string()))
    }
}

impl FrameWriter {
    /// Send one frame. Errors if the peer is gone.
    pub async fn send(&mut self, frame: &str) -> Result<(), IpcError> {
        let len = u32::try_from(frame.len()).map_err(|_| IpcError::FrameTooLarge(u32::MAX))?;
        self.inner.write_all(&len.to_be_bytes()).await?;
        self.inner.write_all(frame.as_bytes()).await?;
        self.inner.flush().await?;
        Ok(())
    }
}

/// A full-duplex frame pipe. Split it to drive the reader on its own task while
/// sending from elsewhere.
pub struct Connection {
    reader: FrameReader,
    writer: FrameWriter,
}

impl Connection {
    /// Frame over any full-duplex byte stream, not only a socket.
    pub fn from_io<S: Stream + 'static>(stream: S) -> Self {
        Connection::from_stream(Box::new(stream))
    }

    fn from_stream(stream: BoxStream) -> Self {
        let (r, w) = tokio::io::split(stream);
        Connection {
            reader: FrameReader { inner: r },
            writer: FrameWriter { inner: w },
        }
    }

    pub fn into_split(self) -> (FrameReader, FrameWriter) {
        (self.reader, self.writer)
    }
}

/// The verified identity of an accepted connection's peer. The seam where a
/// future remote transport carries a token/cert instead of a local uid.
#[derive(Debug, Clone, Copy)]
pub struct Peer {
    pub local: bool,
    pub uid: u32,
}

impl Peer {
    /// Only the user running the daemon may drive it.
    #[cfg(unix)]
    pub fn authorized(&self) -> bool {
        // SAFETY: getuid has no preconditions and always succeeds.
        self.uid == unsafe { libc::getuid() }
    }

    #[cfg(not(unix))]
    pub fn authorized(&self) -> bool {
        self.local
    }
}

pub use platform::{bind, connect, Listener};

#[cfg(unix)]
mod platform {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use tokio::net::{UnixListener, UnixStream};

    use super::{Connection, Peer};
    use crate::errors::IpcError;

    pub async fn connect(endpoint: &Path) -> Result<Connection, IpcError> {
        let stream = UnixStream::connect(endpoint).await?;
        Ok(Connection::from_stream(Box::new(stream)))
    }

    // Is a daemon actually answering on `path`? Tells a live daemon (refuse to
    // start) from a stale socket left by a crash (reclaim it).
    async fn endpoint_alive(path: &Path) -> bool {
        UnixStream::connect(path).await.is_ok()
    }

    fn peer_uid(fd: std::os::fd::RawFd) -> Option<u32> {
        #[cfg(target_os = "linux")]
        {
            let mut cred = libc::ucred {
                pid: 0,
                uid: 0,
                gid: 0,
            };
            let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
            // SAFETY: fd is a valid connected socket; cred/len are correctly sized.
            let rc = unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    (&mut cred as *mut libc::ucred).cast(),
                    &mut len,
                )
            };
            if rc == 0 {
                Some(cred.uid)
            } else {
                None
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut uid: libc::uid_t = 0;
            let mut gid: libc::gid_t = 0;
            // SAFETY: fd is a valid connected socket.
            let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
            if rc == 0 {
                Some(uid)
            } else {
                None
            }
        }
    }

    /// Server side, owned by the daemon. One instance per endpoint.
    pub struct Listener {
        inner: UnixListener,
        path: PathBuf,
    }

    impl Listener {
        /// Accept the next connection with its verified peer identity.
        pub async fn accept(&self) -> Result<(Connection, Peer), IpcError> {
            let (stream, _addr) = self.inner.accept().await?;
            let uid = peer_uid(stream.as_raw_fd()).unwrap_or(u32::MAX);
            let peer = Peer { local: true, uid };
            Ok((Connection::from_stream(Box::new(stream)), peer))
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            // Best-effort cleanup of our own socket.
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Bind a listener to `endpoint`, failing fast if another daemon owns it.
    pub async fn bind(endpoint: &Path) -> Result<Listener, IpcError> {
        if let Some(parent) = endpoint.parent() {
            std::fs::create_dir_all(parent)?;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }

        let listener = match UnixListener::bind(endpoint) {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if endpoint_alive(endpoint).await {
                    return Err(IpcError::Io(std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        "hestiad is already running",
                    )));
                }
                std::fs::remove_file(endpoint)?; // reclaim the stale socket
                UnixListener::bind(endpoint)?
            }
            Err(e) => return Err(e.into()),
        };

        let _ = std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600));
        Ok(Listener {
            inner: listener,
            path: endpoint.to_path_buf(),
        })
    }
}

#[cfg(windows)]
mod platform {
    use std::path::Path;
    use std::time::Duration;

    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

    use super::{Connection, Peer};
    use crate::errors::IpcError;
    use crate::identity::{ensure_served_by_current_user, PipeSecurity};

    const ERROR_PIPE_BUSY: i32 = 231;
    const BUSY_RETRIES: u32 = 20;
    const BUSY_BACKOFF: Duration = Duration::from_millis(25);

    pub async fn connect(endpoint: &Path) -> Result<Connection, IpcError> {
        let name = endpoint.to_string_lossy().to_string();
        let mut attempt = 0;
        loop {
            match ClientOptions::new().open(&name) {
                Ok(client) => {
                    ensure_served_by_current_user(&client)?;
                    return Ok(Connection::from_stream(Box::new(client)));
                }
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempt < BUSY_RETRIES => {
                    attempt += 1;
                    tokio::time::sleep(BUSY_BACKOFF).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Server side: a fresh pipe instance is created per accepted connection.
    /// Every instance carries the same access list (the current user and the
    /// system) and refuses remote clients.
    pub struct Listener {
        name: String,
        security: PipeSecurity,
        next: std::sync::Mutex<Option<NamedPipeServer>>,
    }

    fn create(
        name: &str,
        security: &PipeSecurity,
        first: bool,
    ) -> std::io::Result<NamedPipeServer> {
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first)
            .reject_remote_clients(true);
        // SAFETY: the attributes point at a descriptor that `security` keeps alive.
        unsafe { options.create_with_security_attributes_raw(name, security.attributes()) }
    }

    impl Listener {
        pub async fn accept(&self) -> Result<(Connection, Peer), IpcError> {
            let server = self.armed()?;
            let connected = server.connect().await;
            // Without a listening instance the pipe name vanishes: clients read the daemon as down.
            *self.next.lock().unwrap() = Some(create(&self.name, &self.security, false)?);
            connected?;
            let peer = Peer {
                local: true,
                uid: 0,
            };
            Ok((Connection::from_stream(Box::new(server)), peer))
        }

        fn armed(&self) -> std::io::Result<NamedPipeServer> {
            let parked = self.next.lock().unwrap().take();
            parked.map_or_else(|| create(&self.name, &self.security, false), Ok)
        }
    }

    pub async fn bind(endpoint: &Path) -> Result<Listener, IpcError> {
        let name = endpoint.to_string_lossy().to_string();
        let security = PipeSecurity::current_user()?;
        let first = create(&name, &security, true).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                IpcError::Io(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "hestiad is already running",
                ))
            } else {
                IpcError::Io(e)
            }
        })?;
        Ok(Listener {
            name,
            security,
            next: std::sync::Mutex::new(Some(first)),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn a_pipe_round_trips_frames_and_has_one_owner() {
            let name = format!(
                r"{}test-{}",
                common::app::PIPE_NAMESPACE,
                std::process::id()
            );
            let endpoint = Path::new(&name);
            let listener = bind(endpoint).await.unwrap();
            assert!(bind(endpoint).await.is_err(), "a second owner is refused");

            let served = tokio::spawn(async move {
                let (connection, _) = listener.accept().await.unwrap();
                let (mut reader, mut writer) = connection.into_split();
                let frame = reader.recv().await.unwrap().unwrap();
                writer.send(&format!("re:{frame}")).await.unwrap();
                listener
            });
            let (mut reader, mut writer) = connect(endpoint).await.unwrap().into_split();
            writer.send("ping").await.unwrap();
            assert_eq!(reader.recv().await.unwrap().as_deref(), Some("re:ping"));
            served.await.unwrap();
        }
    }
}
