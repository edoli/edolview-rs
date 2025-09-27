use crate::model::{MatImage, SocketAsset};
use color_eyre::eyre::Result;
use flate2::read::ZlibDecoder;
use opencv::core::Size;
use std::{
    io::{self, Read},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

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
    handle: JoinHandle<io::Result<()>>,
}

pub fn start_socket_listener(
    addr: &str,
    tx: Sender<SocketAsset>,
    socket_state: Arc<SocketState>,
) -> io::Result<SocketServer> {
    let listener = TcpListener::bind(addr)?;

    let stop = Arc::new(AtomicBool::new(false));

    let handle = thread::spawn(move || -> io::Result<()> {
        loop {
            if socket_state.is_socket_active.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        eprintln!("[socket_comm] connected: {peer}");

                        socket_state.is_socket_receiving.store(true, Ordering::Relaxed);

                        if let Ok(asset) = handle_client(&mut stream) {
                            if tx.send(asset).is_err() {
                                socket_state.is_socket_receiving.store(false, Ordering::Relaxed);
                                eprintln!("[socket_comm] receiver dropped");
                                continue;
                            }
                        }

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

    Ok(SocketServer { stop, handle })
}

// Retry with next port if bind fails
pub fn start_server_with_retry(
    host: &str,
    mut port: u16,
    tx: Sender<SocketAsset>,
    socket_state: Arc<SocketState>,
    socket_info: Arc<Mutex<SocketInfo>>,
) -> io::Result<()> {
    loop {
        let addr = format!("{}:{}", host, port);
        if let Err(e) = start_socket_listener(addr.as_str(), tx.clone(), socket_state.clone()) {
            eprintln!("bind {}:{} failed: {e}. trying next port ...", host, port);
            port = port.wrapping_add(1);
            if port == 0 {
                return Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "port range exhausted"));
            }
        } else {
            let mut socket_info = socket_info.lock().unwrap();
            socket_info.address = addr.clone();
            socket_info.host = host.to_string();
            socket_info.port = port;
            eprintln!("Socket listener started on {addr}");
            return Ok(());
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

fn read_i32(stream: &mut TcpStream) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf)?;
    let n = i32::from_be_bytes(buf);
    if n < 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "negative length encountered"));
    }
    Ok(n as u32)
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
    let name_len = read_i32(stream)?;
    let extra_len = read_i32(stream)?;
    let buf_len = read_i32(stream)?;

    // 2) name, extra(json), buf(bytes)
    let name_bytes = read_exact_len(stream, name_len as usize)?;
    let name = String::from_utf8(name_bytes)?;

    let extra_bytes = read_exact_len(stream, extra_len as usize)?;
    let extra = parse_extra(&extra_bytes)?;

    let payload = read_exact_len(stream, buf_len as usize)?;

    if extra.nbytes == 0 || extra.shape.is_empty() || extra.dtype > 7 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid extra metadata").into());
    }

    let dtype = extra.dtype as i32;

    let mat = match extra.compression.as_str() {
        "zlib" => {
            let mut z = ZlibDecoder::new(payload.as_slice());
            let mut raw = Vec::with_capacity(extra.nbytes as usize);
            z.read_to_end(&mut raw)?;

            let channel = if extra.shape.len() == 3 {
                extra.shape[2] as i32
            } else {
                1
            };
            let cv_type = crate::util::cv_ext::cv_make_type(dtype, channel);

            MatImage::from_bytes_size_type(&raw, Size::new(extra.shape[1] as i32, extra.shape[0] as i32), cv_type)?
        }
        "png" => MatImage::from_bytes(&payload)?,
        "exr" => MatImage::from_bytes(&payload)?,
        "cv" => MatImage::from_bytes(&payload)?,
        "raw" => {
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
