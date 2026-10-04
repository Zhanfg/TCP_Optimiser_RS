use serde::Serialize;
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const RUNTIME_DIR: &str = "runtime";
const CONTROL_SOCKET: &str = "control.sock";
const MAX_REQUEST: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    ApplyFast,
    ApplyFull,
    Ping,
}

#[derive(Debug, Serialize)]
pub struct ApplyResponse<'a> {
    pub ok: bool,
    pub mode: &'a str,
    pub elapsed_ms: u128,
    pub error: Option<String>,
}

pub struct ControlServer {
    listener: UnixListener,
    path: PathBuf,
}

impl ControlServer {
    pub fn bind() -> io::Result<Self> {
        let path = socket_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
        if path.exists() {
            let _ = fs::remove_file(&path);
        }

        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        Ok(Self { listener, path })
    }

    pub fn raw_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }

    pub fn accept(&self) -> io::Result<Option<(Request, UnixStream)>> {
        let (mut stream, _) = match self.listener.accept() {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error),
        };

        stream.set_read_timeout(Some(Duration::from_millis(250)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;

        let mut buffer = [0u8; MAX_REQUEST];
        let count = stream.read(&mut buffer)?;
        let command = std::str::from_utf8(&buffer[..count])
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid control request"))?
            .trim();

        let request = match command {
            "APPLY_FAST" => Request::ApplyFast,
            "APPLY_FULL" => Request::ApplyFull,
            "PING" => Request::Ping,
            _ => {
                let _ = stream.write_all(b"{\"ok\":false,\"error\":\"unknown request\"}\n");
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unknown control request",
                ));
            }
        };
        Ok(Some((request, stream)))
    }

    pub fn reply_json<T: Serialize>(&self, mut stream: UnixStream, value: &T) -> io::Result<()> {
        let mut payload = serde_json::to_vec(value).map_err(io::Error::other)?;
        payload.push(b'\n');
        stream.write_all(&payload)
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn request_apply(full: bool) -> io::Result<String> {
    request(if full { "APPLY_FULL" } else { "APPLY_FAST" })
}

pub fn ping() -> io::Result<String> {
    request("PING")
}

fn request(command: &str) -> io::Result<String> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(Duration::from_millis(1500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(300)))?;
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "daemon returned an empty control response",
        ));
    }
    Ok(response)
}

pub fn socket_path() -> PathBuf {
    crate::config::module_dir()
        .join(RUNTIME_DIR)
        .join(CONTROL_SOCKET)
}

pub fn socket_exists() -> bool {
    Path::new(&socket_path()).exists()
}

#[cfg(test)]
mod tests {
    use super::MAX_REQUEST;

    #[test]
    fn control_protocol_is_deliberately_tiny() {
        assert!(MAX_REQUEST <= 128);
        for request in ["APPLY_FAST", "APPLY_FULL", "PING"] {
            assert!(request.len() < MAX_REQUEST);
        }
    }
}
