use crate::model::{MatImage, SocketAsset};
use color_eyre::eyre::Result;
use flate2::read::ZlibDecoder;
use opencv::core::Size;
use std::{
    io::{self, Read},
    net::{TcpListener, TcpStream},
    sync::{atomic::AtomicBool, mpsc::Sender, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone)]
pub struct SocketState {
    pub is_socket_active: bool,
    pub is_socket_receiving: bool,
    pub socket_address: String,
}

impl SocketState {
    pub fn new() -> Self {
        Self {
            is_socket_active: false,
            is_socket_receiving: false,
            socket_address: String::from(""),
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
    socket_state: Arc<Mutex<SocketState>>,
) -> io::Result<SocketServer> {
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;

    let stop = Arc::new(AtomicBool::new(false));

    let handle = thread::spawn(move || -> io::Result<()> {
        loop {
            if socket_state.lock().unwrap().is_socket_active {
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        eprintln!("[socket_comm] connected: {peer}");

                        socket_state.lock().unwrap().is_socket_receiving = true;

                        if let Ok(asset) = handle_client(&mut stream) {
                            if tx.send(asset).is_err() {
                                socket_state.lock().unwrap().is_socket_receiving = false;
                                eprintln!("[socket_comm] receiver dropped");
                                continue;
                            }
                        }

                        socket_state.lock().unwrap().is_socket_receiving = false;
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
fn start_server_with_retry(
    host: String,
    mut port: u16,
    tx: Sender<SocketAsset>,
    socket_state: Arc<Mutex<SocketState>>,
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
            eprintln!("Socket listener started on {addr}");
            return Ok(());
        }
    }
}

struct Extra {
    compression: String, // "png" | "zlib"
    nbytes: usize,
    shape: Vec<usize>,
    dtype: String,
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

fn parse_extra(json_bytes: &[u8]) -> Result<Extra> {
    let text = std::str::from_utf8(json_bytes)?;
    let parsed = json::parse(text)?;

    let compression = parsed["compression"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing compression"))?
        .to_string();

    let nbytes = parsed["nbytes"]
        .as_u64()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing nbytes"))? as usize;

    let shape = parsed["shape"]
        .members()
        .as_slice()
        .iter()
        .map(|x| x.as_u64().unwrap_or(0) as usize)
        .collect::<Vec<_>>();

    let dtype = parsed["dtype"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing dtype"))?
        .to_string();

    Ok(Extra {
        compression,
        nbytes,
        shape,
        dtype,
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

    if extra.nbytes == 0 || extra.shape.is_empty() || extra.dtype.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid extra metadata").into());
    }

    let mat = match extra.compression.as_str() {
        "zlib" => {
            let mut z = ZlibDecoder::new(payload.as_slice());
            let mut raw = Vec::with_capacity(extra.nbytes);
            z.read_to_end(&mut raw)?;

            let depth = crate::util::cv_ext::parse_cv_depth(&extra.dtype);
            let channel = if extra.shape.len() == 3 {
                extra.shape[2] as i32
            } else {
                1
            };
            let cv_type = crate::util::cv_ext::cv_make_type(depth, channel);

            MatImage::from_bytes_size_type(&raw, Size::new(extra.shape[1] as i32, extra.shape[0] as i32), cv_type)?
        }
        "png" => MatImage::from_bytes(&payload)?,
        "exr" => MatImage::from_bytes(&payload)?,
        "cv" => MatImage::from_bytes(&payload)?,
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
