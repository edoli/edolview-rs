use crate::{
    model::{MatImage, SocketAsset},
    util::concurrency::NotifierSender,
};
use color_eyre::eyre::Result;
use flate2::read::ZlibDecoder;
use opencv::core::{Mat, MatExprTraitConst, MatTraitManual, Size};
use std::{
    io::{self, Read},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const SOCKET_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub struct SocketState {
    pub is_socket_active: AtomicBool,
    pub is_socket_receiving: AtomicBool,
}

impl SocketState {
    pub fn new() -> Self {
        Self {
            is_socket_active: AtomicBool::new(true),
            is_socket_receiving: AtomicBool::new(false),
        }
    }
}

pub struct SocketInfo {
    pub address: String,
    pub host: String,
    pub port: u16,
}

impl SocketInfo {
    pub fn new() -> Self {
        Self {
            address: String::from(""),
            host: String::from(""),
            port: 0,
        }
    }
}

pub struct SocketServer {
    stop: Arc<AtomicBool>,
    active_stream: Arc<Mutex<Option<TcpStream>>>,
    addr: SocketAddr,
    handle: Option<JoinHandle<io::Result<()>>>,
}

impl SocketServer {
    #[cfg(test)]
    pub fn address(&self) -> SocketAddr {
        self.addr
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(stream) = self.active_stream.lock().unwrap().take() {
            let _ = stream.shutdown(Shutdown::Both);
        }

        if let Some(handle) = self.handle.take() {
            let (tx, rx) = std::sync::mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(handle.join());
            });
            match rx.recv_timeout(SOCKET_SHUTDOWN_TIMEOUT) {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(io::Error::other("socket listener thread panicked")),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    Err(io::Error::new(io::ErrorKind::TimedOut, "socket listener shutdown timed out"))
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    Err(io::Error::other("socket listener shutdown result was disconnected"))
                }
            }
        } else {
            Ok(())
        }
    }
}

pub fn start_socket_listener(
    addr: &str,
    tx: NotifierSender<SocketAsset>,
    socket_state: Arc<SocketState>,
) -> io::Result<SocketServer> {
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let active_stream = Arc::new(Mutex::new(None::<TcpStream>));
    let active_stream_thread = active_stream.clone();

    let handle = thread::spawn(move || -> io::Result<()> {
        loop {
            if stop_thread.load(Ordering::Relaxed) {
                socket_state.is_socket_receiving.store(false, Ordering::Relaxed);
                return Ok(());
            }

            if socket_state.is_socket_active.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        eprintln!("[socket_comm] connected: {peer}");
                        stream.set_nonblocking(false)?;

                        socket_state.is_socket_receiving.store(true, Ordering::Relaxed);
                        match stream.try_clone() {
                            Ok(cloned) => {
                                *active_stream_thread.lock().unwrap() = Some(cloned);
                            }
                            Err(err) => {
                                eprintln!("[socket_comm] failed to track client stream {peer}: {err}");
                                socket_state.is_socket_receiving.store(false, Ordering::Relaxed);
                                continue;
                            }
                        }
                        if stop_thread.load(Ordering::Relaxed) {
                            active_stream_thread.lock().unwrap().take();
                            let _ = stream.shutdown(Shutdown::Both);
                            socket_state.is_socket_receiving.store(false, Ordering::Relaxed);
                            return Ok(());
                        }

                        match handle_client(&mut stream) {
                            Ok(asset) => {
                                if tx.send(asset).is_err() {
                                    socket_state.is_socket_receiving.store(false, Ordering::Relaxed);
                                    eprintln!("[socket_comm] receiver dropped");
                                    continue;
                                }
                            }
                            Err(err) => {
                                socket_state.is_socket_receiving.store(false, Ordering::Relaxed);
                                eprintln!("[socket_comm] failed to handle client {peer}: {err}");
                            }
                        }

                        active_stream_thread.lock().unwrap().take();
                        socket_state.is_socket_receiving.store(false, Ordering::Relaxed);
                        eprintln!("[socket_comm] disconnected: {peer}");
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => return Err(e),
                }
            } else {
                thread::sleep(Duration::from_millis(100));
            }
        }
    });

    Ok(SocketServer {
        stop,
        active_stream,
        addr,
        handle: Some(handle),
    })
}

// Retry with next port if bind fails
pub fn start_server_with_retry(
    host: &str,
    mut port: u16,
    tx: NotifierSender<SocketAsset>,
    socket_state: Arc<SocketState>,
    socket_info: Arc<Mutex<SocketInfo>>,
) -> io::Result<SocketServer> {
    loop {
        let addr = format!("{}:{}", host, port);
        match start_socket_listener(addr.as_str(), tx.clone(), socket_state.clone()) {
            Ok(server) => {
                let mut socket_info = socket_info.lock().unwrap();
                socket_info.address = addr.clone();
                socket_info.host = host.to_string();
                socket_info.port = port;
                eprintln!("Socket listener started on {addr}");
                return Ok(server);
            }
            Err(e) => {
                eprintln!("bind {}:{} failed: {e}. trying next port ...", host, port);
                port = port.wrapping_add(1);
                if port == 0 {
                    return Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "port range exhausted"));
                }
            }
        }
    }
}

struct Extra {
    nbytes: u64,
    dtype: u32,
    shape: [u32; 3],
    compression: String, // "png" | "zlib"
}

fn read_exact_len(stream: &mut TcpStream, len: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_u64(stream: &mut TcpStream) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    stream.read_exact(&mut buf)?;
    Ok(u64::from_be_bytes(buf))
}

fn parse_extra(bytes: &[u8]) -> Result<Extra> {
    let nbytes = u64::from_be_bytes(bytes[0..8].try_into()?);
    let shape = [
        u32::from_be_bytes(bytes[8..12].try_into()?),
        u32::from_be_bytes(bytes[12..16].try_into()?),
        u32::from_be_bytes(bytes[16..20].try_into()?),
    ];
    let dtype = u32::from_be_bytes(bytes[20..24].try_into()?);
    let compression = String::from_utf8(bytes[24..bytes.len()].to_vec())?
        .trim_end_matches(char::from(0))
        .to_string();

    Ok(Extra {
        nbytes,
        dtype,
        shape,
        compression,
    })
}

fn handle_client(stream: &mut TcpStream) -> Result<SocketAsset> {
    let name_len = read_u64(stream)?;
    let extra_len = read_u64(stream)?;
    let buf_len = read_u64(stream)?;

    // 2) name, extra(json), buf(bytes)
    let name_bytes = read_exact_len(stream, name_len as usize)?;
    let name = String::from_utf8(name_bytes)?;

    let extra_bytes = read_exact_len(stream, extra_len as usize)?;
    let extra = parse_extra(&extra_bytes)?;

    let payload = read_exact_len(stream, buf_len as usize)?;

    let mat = match extra.compression.as_str() {
        "zlib" => {
            validate_raw_extra(&extra)?;
            let dtype = extra.dtype as i32;
            #[cfg(debug_assertions)]
            let _timer = crate::util::timer::ScopedTimer::new("Zlib decode");

            let channel = if extra.shape.len() == 3 {
                extra.shape[2] as i32
            } else {
                1
            };
            let cv_type = crate::util::cv_ext::cv_make_type(dtype, channel);

            let mut z = ZlibDecoder::new(payload.as_slice());
            let mut mat = Mat::zeros(extra.shape[0] as i32, extra.shape[1] as i32, cv_type)?.to_mat()?;
            let raw = mat.data_bytes_mut()?;
            z.read_exact(raw)?;

            MatImage::new(MatImage::postprocess(mat, 1.0, false)?, dtype)
        }
        "png" => MatImage::from_bytes(&payload)?,
        "exr" => MatImage::from_bytes(&payload)?,
        "cv" => MatImage::from_bytes(&payload)?,
        "raw" => {
            validate_raw_extra(&extra)?;
            let dtype = extra.dtype as i32;
            let channel = if extra.shape.len() == 3 {
                extra.shape[2] as i32
            } else {
                1
            };
            let cv_type = crate::util::cv_ext::cv_make_type(dtype, channel);

            MatImage::from_bytes_size_type(&payload, Size::new(extra.shape[1] as i32, extra.shape[0] as i32), cv_type)?
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported compression: {}", extra.compression),
            )
            .into())
        }
    };

    Ok(SocketAsset::new(name, mat))
}

fn validate_raw_extra(extra: &Extra) -> io::Result<()> {
    if extra.nbytes == 0 || extra.dtype > 7 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid extra metadata"));
    }

    let [height, width, channels] = extra.shape;
    if height == 0 || width == 0 || channels == 0 || channels > 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid extra metadata"));
    }

    Ok(())
}
