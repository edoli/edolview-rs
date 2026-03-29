use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::util::concurrency::NotifierSender;

const CONTROL_HOST: &str = "127.0.0.1";
const CONTROL_PORT_START: u16 = 21740;
const CONTROL_PORT_END: u16 = 21790;

#[derive(Serialize, Deserialize)]
struct ControlOpenRequest {
    paths: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct WindowRecord {
    instance_id: String,
    pid: u32,
    control_addr: String,
    last_active_ms: u128,
}

#[derive(Default, Serialize, Deserialize)]
struct WindowRegistry {
    windows: Vec<WindowRecord>,
}

pub struct ControlInstance {
    instance_id: String,
    pid: u32,
    control_addr: String,
}

impl ControlInstance {
    pub fn address(&self) -> &str {
        &self.control_addr
    }

    pub fn touch_active(&self) -> Result<(), String> {
        let mut registry = load_registry()?;
        let now = now_millis()?;
        registry.windows.retain(|record| record.instance_id != self.instance_id);
        registry.windows.push(WindowRecord {
            instance_id: self.instance_id.clone(),
            pid: self.pid,
            control_addr: self.control_addr.clone(),
            last_active_ms: now,
        });
        save_registry(&registry)
    }

    pub fn remove(&self) -> Result<(), String> {
        let mut registry = load_registry()?;
        registry.windows.retain(|record| record.instance_id != self.instance_id);
        save_registry(&registry)
    }
}

pub fn start_control_listener(tx: NotifierSender<Vec<PathBuf>>) -> Result<ControlInstance, String> {
    for port in CONTROL_PORT_START..=CONTROL_PORT_END {
        let addr = format!("{CONTROL_HOST}:{port}");
        match TcpListener::bind(addr.as_str()) {
            Ok(listener) => {
                listener
                    .set_nonblocking(true)
                    .map_err(|e| format!("Failed to configure control listener: {e}"))?;

                thread::spawn(move || loop {
                    match listener.accept() {
                        Ok((mut stream, _peer)) => {
                            if let Ok(paths) = read_request(&mut stream) {
                                let _ = tx.send(paths);
                            }
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(50));
                        }
                        Err(err) => {
                            eprintln!("Control listener error: {err}");
                            break;
                        }
                    }
                });

                let instance = ControlInstance {
                    instance_id: format!("{}-{}", std::process::id(), now_millis()?),
                    pid: std::process::id(),
                    control_addr: addr,
                };
                instance.touch_active()?;
                return Ok(instance);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(err) => return Err(format!("Failed to bind control listener: {err}")),
        }
    }

    Err("Failed to find a free control port".to_string())
}

pub fn try_forward_paths_to_last_active(paths: &[PathBuf]) -> Result<bool, String> {
    let mut registry = load_registry()?;
    registry
        .windows
        .sort_by(|left, right| right.last_active_ms.cmp(&left.last_active_ms));

    let mut changed = false;
    for record in registry.windows.clone() {
        if record.pid == std::process::id() {
            continue;
        }

        match send_request(record.control_addr.as_str(), paths) {
            Ok(()) => return Ok(true),
            Err(_) => {
                registry.windows.retain(|window| window.instance_id != record.instance_id);
                changed = true;
            }
        }
    }

    if changed {
        save_registry(&registry)?;
    }

    Ok(false)
}

fn read_request(stream: &mut TcpStream) -> Result<Vec<PathBuf>, String> {
    let mut body = String::new();
    stream
        .read_to_string(&mut body)
        .map_err(|e| format!("Failed to read control request: {e}"))?;

    let request: ControlOpenRequest =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse control request: {e}"))?;

    Ok(request.paths.into_iter().map(PathBuf::from).collect())
}

fn send_request(addr: &str, paths: &[PathBuf]) -> Result<(), String> {
    let request = ControlOpenRequest {
        paths: paths.iter().map(|path| path.to_string_lossy().to_string()).collect(),
    };
    let body = serde_json::to_vec(&request).map_err(|e| format!("Failed to serialize control request: {e}"))?;

    let mut stream = TcpStream::connect(addr).map_err(|e| format!("Failed to connect to {addr}: {e}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| format!("Failed to set control stream timeout: {e}"))?;
    stream
        .write_all(&body)
        .map_err(|e| format!("Failed to send control request: {e}"))?;
    stream.flush().map_err(|e| format!("Failed to flush control request: {e}"))
}

fn registry_path() -> PathBuf {
    crate::util::path_ext::app_config_dir().join("window-registry.json")
}

fn load_registry() -> Result<WindowRegistry, String> {
    let path = registry_path();
    if !path.exists() {
        return Ok(WindowRegistry::default());
    }

    let body =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read window registry '{}': {e}", path.display()))?;
    serde_json::from_str(&body).map_err(|e| format!("Failed to parse window registry '{}': {e}", path.display()))
}

fn save_registry(registry: &WindowRegistry) -> Result<(), String> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create control directory '{}': {e}", parent.display()))?;
    }

    let temp_path = path.with_extension("json.tmp");
    let body =
        serde_json::to_string_pretty(registry).map_err(|e| format!("Failed to serialize window registry: {e}"))?;
    fs::write(&temp_path, body)
        .map_err(|e| format!("Failed to write temporary window registry '{}': {e}", temp_path.display()))?;
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("Failed to remove previous window registry '{}': {e}", path.display()))?;
    }
    fs::rename(&temp_path, &path).map_err(|e| {
        format!(
            "Failed to replace window registry '{}' with '{}': {e}",
            temp_path.display(),
            path.display()
        )
    })
}

fn now_millis() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|e| format!("Failed to compute current time: {e}"))
}
