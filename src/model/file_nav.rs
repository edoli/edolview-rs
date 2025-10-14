use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use color_eyre::eyre::Result;
use notify::{event::*, recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Default)]
pub struct FileNav {
    pub dir_path: Option<PathBuf>,
    pub files_in_dir: Vec<PathBuf>,
    pub current_file_index: Option<usize>,

    // Filesystem watching
    dir_watcher: Option<RecommendedWatcher>,
    dir_event_rx: Option<mpsc::Receiver<Result<notify::Event, notify::Error>>>,

    // Debounce for watcher events
    pub event_debounce: Duration,
    pending_changed: bool,
    last_change_instant: Option<Instant>,
    staged_set: Option<HashSet<PathBuf>>,
}

impl FileNav {
    pub fn new() -> Self {
        Self {
            dir_path: None,
            files_in_dir: Vec::new(),
            current_file_index: None,
            dir_watcher: None,
            dir_event_rx: None,
            event_debounce: Duration::from_millis(120),
            pending_changed: false,
            last_change_instant: None,
            staged_set: None,
        }
    }

    #[inline]
    pub fn is_supported_image(path: &PathBuf) -> bool {
        let exts = [
            "png", "jpeg", "jpg", "jpe", "jp2", "bmp", "dib", "exr", "tif", "tiff", "hdr", "pic", "webp", "raw", "pfm",
            "pgm", "ppm", "pbm", "pxm", "pnm", "sr", "flo",
        ];
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        exts.contains(&ext.as_str())
    }

    #[inline]
    pub fn sort_paths_case_insensitive(files: &mut Vec<PathBuf>) {
        files.sort_by(|a, b| {
            let an = a
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            let bn = b
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            an.cmp(&bn)
        });
    }

    pub fn refresh_dir_listing_for(&mut self, dir: PathBuf) {
        let dir_abs = canonicalize_friendly(&dir).unwrap_or(dir.clone());
        self.dir_path = Some(dir_abs.clone());
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir_abs) {
            for ent in entries.flatten() {
                let p = ent.path();
                if p.is_file() && Self::is_supported_image(&p) {
                    files.push(p);
                }
            }
        }
        Self::sort_paths_case_insensitive(&mut files);
        self.files_in_dir = files;
        self.pending_changed = false;
        self.last_change_instant = None;
        self.staged_set = None;
    }

    pub fn select_index_for_path(&mut self, path: &PathBuf) {
        let idx = self.files_in_dir.iter().position(|p| p == path).or_else(|| {
            let fname = path.file_name();
            self.files_in_dir.iter().position(|p| p.file_name() == fname)
        });
        self.current_file_index = idx;
    }

    pub fn navigate_next(&mut self) -> Option<PathBuf> {
        if let (Some(_), Some(cur_idx)) = (self.dir_path.clone(), self.current_file_index) {
            if self.files_in_dir.is_empty() {
                return None;
            }
            let next = (cur_idx + 1) % self.files_in_dir.len();
            let path = self.files_in_dir[next].clone();
            return Some(path);
        }
        None
    }

    pub fn navigate_prev(&mut self) -> Option<PathBuf> {
        if let (Some(_), Some(cur_idx)) = (self.dir_path.clone(), self.current_file_index) {
            if self.files_in_dir.is_empty() {
                return None;
            }
            let prev = if cur_idx == 0 {
                self.files_in_dir.len() - 1
            } else {
                cur_idx - 1
            };
            let path = self.files_in_dir[prev].clone();
            return Some(path);
        }
        None
    }

    pub fn start_dir_watcher(&mut self, dir: PathBuf) -> Result<()> {
        self.stop_dir_watcher();
        self.pending_changed = false;
        self.last_change_instant = None;
        self.staged_set = None;
        let dir_abs = canonicalize_friendly(&dir).unwrap_or(dir.clone());
        let (tx, rx) = mpsc::channel::<Result<notify::Event, notify::Error>>();
        let mut watcher = recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;
        watcher.watch(&dir_abs, RecursiveMode::NonRecursive)?;
        self.dir_path = Some(dir_abs);
        self.dir_event_rx = Some(rx);
        self.dir_watcher = Some(watcher);
        Ok(())
    }

    pub fn stop_dir_watcher(&mut self) {
        self.dir_watcher = None;
        self.dir_event_rx = None;
    }

    pub fn process_watcher_events(&mut self) {
        let Some(rx) = &self.dir_event_rx else { return };
        let Some(dir) = &self.dir_path else { return };
        let dir = dir.clone();

        if self.staged_set.is_none() {
            self.staged_set = Some(self.files_in_dir.iter().cloned().collect());
        }
        let set = self.staged_set.as_mut().unwrap();
        let mut changed = false;

        for res in rx.try_iter() {
            let Ok(event) = res else { continue };
            let mut handled = false;
            match event.kind {
                EventKind::Create(CreateKind::File) | EventKind::Create(CreateKind::Any) => {
                    for p in event.paths.iter() {
                        if p.parent() == Some(dir.as_path()) && Self::is_supported_image(p) {
                            if set.insert(p.clone()) {
                                changed = true;
                            }
                        }
                    }
                    handled = true;
                }
                EventKind::Remove(RemoveKind::File) | EventKind::Remove(RemoveKind::Any) => {
                    for p in event.paths.iter() {
                        if p.parent() == Some(dir.as_path()) {
                            if set.remove(p) {
                                changed = true;
                            }
                        }
                    }
                    handled = true;
                }
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                    if event.paths.len() == 2 {
                        let old = event.paths[0].clone();
                        let newp = event.paths[1].clone();
                        if old.parent() == Some(dir.as_path()) {
                            if set.remove(&old) {
                                changed = true;
                            }
                        }
                        if newp.parent() == Some(dir.as_path()) && Self::is_supported_image(&newp) {
                            if set.insert(newp) {
                                changed = true;
                            }
                        }
                        handled = true;
                    }
                }
                _ => {}
            }

            if !handled {
                if let EventKind::Modify(ModifyKind::Name(_)) = event.kind {
                    if event.paths.len() == 1 {
                        let p = event.paths[0].clone();
                        if p.parent() == Some(dir.as_path()) {
                            if Self::is_supported_image(&p) {
                                if set.insert(p) {
                                    changed = true;
                                }
                            } else {
                                if set.remove(&p) {
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        if changed {
            self.pending_changed = true;
            self.last_change_instant = Some(Instant::now());
        }

        if self.pending_changed {
            let ready = self
                .last_change_instant
                .map(|t| t.elapsed() >= self.event_debounce)
                .unwrap_or(false);
            if ready {
                if let Some(mut set) = self.staged_set.take() {
                    let mut new_list: Vec<PathBuf> = set.drain().collect();
                    Self::sort_paths_case_insensitive(&mut new_list);
                    self.files_in_dir = new_list;
                }
                self.pending_changed = false;
                self.last_change_instant = None;
            }
        }
    }

    pub fn clear(&mut self) {
        self.stop_dir_watcher();
        self.dir_path = None;
        self.files_in_dir.clear();
        self.current_file_index = None;
    }
}

/// Canonicalize a path but strip Windows verbatim prefixes ("\\\\?\\" or "\\\\?\\UNC\\")
/// so that UI display is cleaner. Falls back to standard canonicalize if dunce fails
/// (e.g., on non-existent path) and finally to the original input.
fn canonicalize_friendly(p: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let can = dunce::canonicalize(p).ok().or_else(|| std::fs::canonicalize(p).ok());
        can.or_else(|| Some(p.to_path_buf()))
    }
    #[cfg(not(windows))]
    {
        std::fs::canonicalize(p).ok().or_else(|| Some(p.to_path_buf()))
    }
}
