use std::env;
use std::fs;
use std::path::PathBuf;

pub fn is_unicode_path(path: &PathBuf) -> bool {
    match path.to_str() {
        Some(s) => s.chars().all(|c| c.is_ascii()),
        None => false,
    }
}

/// Crossplatform safe temporary directory path
/// 1. If std::env::temp_dir() is representable in Unicode, use it
/// 2. Otherwise, fallback to environment variables / hardcoded ASCII paths
pub fn safe_temp_dir() -> PathBuf {
    let tmp = env::temp_dir();

    if is_unicode_path(&tmp) {
        return tmp;
    }

    // 2. fallback candidates: (allow user override)
    //    - RUST_SAFE_TEMP (custom)
    //    - TMPDIR / TMP / TEMP (platform conventions)
    let candidates = [
        env::var_os("RUST_SAFE_TEMP"),
        env::var_os("TMPDIR"),
        env::var_os("TMP"),
        env::var_os("TEMP"),
    ];

    for cand in candidates.iter().flatten() {
        let p = PathBuf::from(cand);
        if is_unicode_path(&p) {
            let _ = fs::create_dir_all(&p);
            return p;
        }
    }

    // 3. Last : completely hardcoded ASCII path
    #[cfg(windows)]
    let p = PathBuf::from(r"C:\Temp");

    #[cfg(not(windows))]
    let p = PathBuf::from("/tmp");

    let _ = fs::create_dir_all(&p);
    p
}

pub fn exe_dir_or_cwd() -> PathBuf {
    if let Ok(exe_path) = env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            return dir.to_path_buf();
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
