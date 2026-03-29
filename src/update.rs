use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::{fs::File, io::Write};

use serde::Deserialize;

use crate::util::path_ext::safe_temp_dir;

pub const RELEASES_PAGE_URL: &str = "https://github.com/edoli/edolview-rs/releases";
const LATEST_RELEASE_API_URL: &str = "https://api.github.com/repos/edoli/edolview-rs/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub html_url: String,
    pub asset_name: String,
    pub asset_download_url: String,
    pub target: UpdateTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateTarget {
    PortableZip,
    WindowsMsi,
    MacDmg,
    LinuxDeb,
    LinuxAppImage,
}

impl UpdateTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::PortableZip => "portable zip",
            Self::WindowsMsi => "MSI installer",
            Self::MacDmg => "DMG installer",
            Self::LinuxDeb => "DEB package",
            Self::LinuxAppImage => "AppImage",
        }
    }
}

#[derive(Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

pub fn check_for_update() -> Result<Option<AvailableUpdate>, String> {
    let release = fetch_latest_release()?;
    if !is_newer_version(release.tag_name.as_str(), CURRENT_VERSION) {
        return Ok(None);
    }

    let target = detect_update_target()?;
    let asset_name = preferred_asset_name(target);
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| format!("No compatible update asset found for {}", target.label()))?;

    Ok(Some(AvailableUpdate {
        version: normalize_version_label(release.tag_name.as_str()),
        html_url: release.html_url,
        asset_name: asset.name,
        asset_download_url: asset.browser_download_url,
        target,
    }))
}

pub fn start_update(update: &AvailableUpdate) -> Result<String, String> {
    let current_exe = env::current_exe().map_err(|e| format!("Failed to locate current executable: {e}"))?;
    let temp_dir = create_update_temp_dir()?;
    let asset_path = temp_dir.join(update.asset_name.as_str());

    download_asset(update.asset_download_url.as_str(), &asset_path)?;
    launch_update_helper(update.target, &current_exe, &asset_path)?;

    Ok(format!(
        "Downloaded {}. Edolview will now close, and a separate updater window will stay visible while the {} is applied.",
        update.version,
        update.target.label()
    ))
}

fn fetch_latest_release() -> Result<ReleaseResponse, String> {
    ureq::get(LATEST_RELEASE_API_URL)
        .header("User-Agent", "edolview-rs")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Failed to query GitHub releases: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read GitHub release response: {e}"))
        .and_then(|body| {
            serde_json::from_str::<ReleaseResponse>(&body)
                .map_err(|e| format!("Failed to parse GitHub release response: {e}"))
        })
}

fn download_asset(url: &str, destination: &Path) -> Result<(), String> {
    let bytes = ureq::get(url)
        .header("User-Agent", "edolview-rs")
        .call()
        .map_err(|e| format!("Failed to download update asset: {e}"))?
        .body_mut()
        .read_to_vec()
        .map_err(|e| format!("Failed to read update asset: {e}"))?;

    fs::write(destination, bytes).map_err(|e| format!("Failed to save update asset: {e}"))
}

fn detect_update_target() -> Result<UpdateTarget, String> {
    let exe = env::current_exe().map_err(|e| format!("Failed to locate current executable: {e}"))?;

    #[cfg(target_os = "windows")]
    {
        let exe_lower = exe.to_string_lossy().to_ascii_lowercase();
        if exe_lower.contains("\\program files\\") {
            return Ok(UpdateTarget::WindowsMsi);
        }
        return Ok(UpdateTarget::PortableZip);
    }

    #[cfg(target_os = "macos")]
    {
        if app_bundle_path(&exe).is_some() {
            return Ok(UpdateTarget::MacDmg);
        }
        return Ok(UpdateTarget::PortableZip);
    }

    #[cfg(target_os = "linux")]
    {
        if exe.extension().and_then(|ext| ext.to_str()) == Some("AppImage") {
            return Ok(UpdateTarget::LinuxAppImage);
        }
        if exe.starts_with(Path::new("/opt/edolview")) {
            return Ok(UpdateTarget::LinuxDeb);
        }
        return Ok(UpdateTarget::PortableZip);
    }

    #[allow(unreachable_code)]
    Err("Automatic updates are not supported on this platform".to_string())
}

fn preferred_asset_name(target: UpdateTarget) -> &'static str {
    match target {
        UpdateTarget::PortableZip => {
            #[cfg(target_os = "windows")]
            {
                "edolview-windows.zip"
            }
            #[cfg(target_os = "macos")]
            {
                "edolview-mac.zip"
            }
            #[cfg(target_os = "linux")]
            {
                "edolview-linux.zip"
            }
        }
        UpdateTarget::WindowsMsi => "edolview-installer-msi.msi",
        UpdateTarget::MacDmg => "edolview-installer-dmg.dmg",
        UpdateTarget::LinuxDeb => "edolview-installer-deb.deb",
        UpdateTarget::LinuxAppImage => "edolview-installer-appimage.AppImage",
    }
}

fn create_update_temp_dir() -> Result<PathBuf, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Failed to compute update temp timestamp: {e}"))?
        .as_millis();
    let dir = safe_temp_dir().join(format!("edolview-update-{}-{millis}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create update temp directory: {e}"))?;
    Ok(dir)
}

fn launch_update_helper(target: UpdateTarget, current_exe: &Path, asset_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let script_path = asset_path.parent().unwrap_or_else(|| Path::new(".")).join("apply_update.ps1");
        write_windows_update_script(&script_path, target, current_exe, asset_path)?;
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Sta",
                "-WindowStyle",
                "Hidden",
                "-File",
            ])
            .arg(&script_path)
            .spawn()
            .map_err(|e| format!("Failed to launch update helper: {e}"))?;
        return Ok(());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let script_path = asset_path.parent().unwrap_or_else(|| Path::new(".")).join("apply_update.sh");
        write_unix_update_script(&script_path, target, current_exe, asset_path)?;
        let launch_cmd = format!(
            "nohup sh {} >/dev/null 2>&1 &",
            sh_quote(script_path.to_string_lossy().as_ref())
        );
        Command::new("sh")
            .arg("-c")
            .arg(launch_cmd)
            .spawn()
            .map_err(|e| format!("Failed to launch update helper: {e}"))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("Automatic updates are not supported on this platform".to_string())
}

#[cfg(target_os = "windows")]
fn write_windows_update_script(
    path: &Path,
    target: UpdateTarget,
    current_exe: &Path,
    asset_path: &Path,
) -> Result<(), String> {
    let current_dir = current_exe
        .parent()
        .ok_or_else(|| "Current executable has no parent directory".to_string())?;
    let pid = std::process::id();

    let script = match target {
        UpdateTarget::PortableZip => format!(
            r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$form = New-Object System.Windows.Forms.Form
$form.Text = 'Updating Edolview'
$form.Width = 420
$form.Height = 150
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.ControlBox = $false
$form.TopMost = $true
$label = New-Object System.Windows.Forms.Label
$label.Left = 20
$label.Top = 16
$label.Width = 360
$label.Height = 44
$label.TextAlign = 'MiddleLeft'
$progress = New-Object System.Windows.Forms.ProgressBar
$progress.Left = 20
$progress.Top = 74
$progress.Width = 360
$progress.Height = 18
$progress.Style = 'Marquee'
$progress.MarqueeAnimationSpeed = 30
$form.Controls.Add($label)
$form.Controls.Add($progress)
$form.Show()
function Set-Status([string] $text) {{
    $label.Text = $text
    [System.Windows.Forms.Application]::DoEvents()
}}
Set-Status({status_waiting})
$pidToWait = {pid}
while (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue) {{
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 300
}}
Set-Status({status_installing})
$assetPath = {asset_path}
$targetDir = {target_dir}
$extractDir = Join-Path (Split-Path $assetPath -Parent) 'portable'
if (Test-Path $extractDir) {{
    Remove-Item $extractDir -Recurse -Force
}}
Expand-Archive -Path $assetPath -DestinationPath $extractDir -Force
Copy-Item (Join-Path $extractDir 'edolview.exe') (Join-Path $targetDir 'edolview.exe') -Force
if (Test-Path (Join-Path $targetDir 'colormap')) {{
    Remove-Item (Join-Path $targetDir 'colormap') -Recurse -Force
}}
Copy-Item (Join-Path $extractDir 'colormap') (Join-Path $targetDir 'colormap') -Recurse -Force
Set-Status({status_restarting})
Start-Process -FilePath (Join-Path $targetDir 'edolview.exe')
$form.Close()
"#,
            status_waiting = ps_quote_string("Waiting for Edolview to close so the update can start..."),
            status_installing = ps_quote_string("Installing the new portable build..."),
            status_restarting = ps_quote_string("Update installed. Restarting Edolview..."),
            asset_path = ps_quote_path(asset_path),
            target_dir = ps_quote_path(current_dir),
        ),
        UpdateTarget::WindowsMsi => format!(
            r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$form = New-Object System.Windows.Forms.Form
$form.Text = 'Updating Edolview'
$form.Width = 420
$form.Height = 150
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.ControlBox = $false
$form.TopMost = $true
$label = New-Object System.Windows.Forms.Label
$label.Left = 20
$label.Top = 16
$label.Width = 360
$label.Height = 44
$label.TextAlign = 'MiddleLeft'
$progress = New-Object System.Windows.Forms.ProgressBar
$progress.Left = 20
$progress.Top = 74
$progress.Width = 360
$progress.Height = 18
$progress.Style = 'Marquee'
$progress.MarqueeAnimationSpeed = 30
$form.Controls.Add($label)
$form.Controls.Add($progress)
$form.Show()
function Set-Status([string] $text) {{
    $label.Text = $text
    [System.Windows.Forms.Application]::DoEvents()
}}
Set-Status({status_waiting})
$pidToWait = {pid}
while (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue) {{
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 300
}}
Set-Status({status_installing})
$assetPath = {asset_path}
Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', $assetPath, '/passive', '/norestart')
$form.Close()
"#,
            status_waiting = ps_quote_string("Waiting for Edolview to close so the installer can start..."),
            status_installing =
                ps_quote_string("Launching the MSI installer. Follow any prompts to finish the update."),
            asset_path = ps_quote_path(asset_path),
        ),
        _ => return Err("Unsupported Windows update target".to_string()),
    };

    fs::write(path, script).map_err(|e| format!("Failed to write update helper script: {e}"))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_unix_update_script(
    path: &Path,
    target: UpdateTarget,
    current_exe: &Path,
    asset_path: &Path,
) -> Result<(), String> {
    let pid = std::process::id();
    let status_message = sh_quote(status_message(target));
    let script = match target {
        UpdateTarget::PortableZip => {
            let current_dir = current_exe
                .parent()
                .ok_or_else(|| "Current executable has no parent directory".to_string())?;
            format!(
                r#"#!/bin/sh
set -eu
STATUS_PID=""
STATUS_MESSAGE={status_message}
start_status() {{
    if command -v zenity >/dev/null 2>&1; then
        zenity --info --title="Updating Edolview" --text="$STATUS_MESSAGE" --no-wrap --timeout=600 >/dev/null 2>&1 &
        STATUS_PID=$!
    elif command -v osascript >/dev/null 2>&1; then
        osascript -e 'on run argv' -e 'display dialog (item 1 of argv) with title "Updating Edolview" buttons {{}} giving up after 600' -e 'end run' "$STATUS_MESSAGE" >/dev/null 2>&1 &
        STATUS_PID=$!
    elif command -v notify-send >/dev/null 2>&1; then
        notify-send "Updating Edolview" "$STATUS_MESSAGE"
    fi
}}
stop_status() {{
    if [ -n "${{STATUS_PID:-}}" ]; then
        kill "$STATUS_PID" 2>/dev/null || true
    fi
}}
trap 'stop_status' EXIT
start_status
PID={pid}
while kill -0 "$PID" 2>/dev/null; do
    sleep 0.3
done
ASSET={asset}
TARGET_DIR={target_dir}
EXTRACT_DIR="$(dirname "$ASSET")/portable"
PYTHON_BIN="$(command -v python3 || command -v python || true)"
if [ -z "$PYTHON_BIN" ]; then
    exit 1
fi
rm -rf "$EXTRACT_DIR"
mkdir -p "$EXTRACT_DIR"
"$PYTHON_BIN" -c "import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$ASSET" "$EXTRACT_DIR"
rm -f "$TARGET_DIR/edolview"
rm -rf "$TARGET_DIR/colormap"
cp "$EXTRACT_DIR/edolview" "$TARGET_DIR/edolview"
chmod +x "$TARGET_DIR/edolview"
cp -R "$EXTRACT_DIR/colormap" "$TARGET_DIR/colormap"
nohup "$TARGET_DIR/edolview" >/dev/null 2>&1 &
"#,
                status_message = status_message,
                asset = sh_quote(asset_path.to_string_lossy().as_ref()),
                target_dir = sh_quote(current_dir.to_string_lossy().as_ref()),
            )
        }
        UpdateTarget::MacDmg => {
            let app_bundle =
                app_bundle_path(current_exe).ok_or_else(|| "Failed to locate current app bundle".to_string())?;
            format!(
                r#"#!/bin/sh
set -eu
STATUS_PID=""
STATUS_MESSAGE={status_message}
start_status() {{
    if command -v osascript >/dev/null 2>&1; then
        osascript -e 'on run argv' -e 'display dialog (item 1 of argv) with title "Updating Edolview" buttons {{}} giving up after 600' -e 'end run' "$STATUS_MESSAGE" >/dev/null 2>&1 &
        STATUS_PID=$!
    elif command -v zenity >/dev/null 2>&1; then
        zenity --info --title="Updating Edolview" --text="$STATUS_MESSAGE" --no-wrap --timeout=600 >/dev/null 2>&1 &
        STATUS_PID=$!
    elif command -v notify-send >/dev/null 2>&1; then
        notify-send "Updating Edolview" "$STATUS_MESSAGE"
    fi
}}
stop_status() {{
    if [ -n "${{STATUS_PID:-}}" ]; then
        kill "$STATUS_PID" 2>/dev/null || true
    fi
}}
trap 'stop_status' EXIT
start_status
PID={pid}
while kill -0 "$PID" 2>/dev/null; do
    sleep 0.3
done
ASSET={asset}
APP_BUNDLE={app_bundle}
MOUNT_DIR="$(dirname "$ASSET")/mount"
rm -rf "$MOUNT_DIR"
mkdir -p "$MOUNT_DIR"
hdiutil attach -nobrowse -quiet -mountpoint "$MOUNT_DIR" "$ASSET"
SOURCE_APP="$(find "$MOUNT_DIR" -maxdepth 1 -name '*.app' -print -quit)"
if [ -z "$SOURCE_APP" ]; then
    hdiutil detach "$MOUNT_DIR" -quiet || true
    exit 1
fi
rm -rf "$APP_BUNDLE"
ditto "$SOURCE_APP" "$APP_BUNDLE"
hdiutil detach "$MOUNT_DIR" -quiet || hdiutil detach "$MOUNT_DIR" -force -quiet || true
open "$APP_BUNDLE"
"#,
                status_message = status_message,
                asset = sh_quote(asset_path.to_string_lossy().as_ref()),
                app_bundle = sh_quote(app_bundle.to_string_lossy().as_ref()),
            )
        }
        UpdateTarget::LinuxDeb => format!(
            r#"#!/bin/sh
set -eu
STATUS_PID=""
STATUS_MESSAGE={status_message}
start_status() {{
    if command -v zenity >/dev/null 2>&1; then
        zenity --info --title="Updating Edolview" --text="$STATUS_MESSAGE" --no-wrap --timeout=600 >/dev/null 2>&1 &
        STATUS_PID=$!
    elif command -v notify-send >/dev/null 2>&1; then
        notify-send "Updating Edolview" "$STATUS_MESSAGE"
    fi
}}
stop_status() {{
    if [ -n "${{STATUS_PID:-}}" ]; then
        kill "$STATUS_PID" 2>/dev/null || true
    fi
}}
trap 'stop_status' EXIT
start_status
PID={pid}
while kill -0 "$PID" 2>/dev/null; do
    sleep 0.3
done
ASSET={asset}
if command -v pkexec >/dev/null 2>&1; then
    nohup pkexec /usr/bin/dpkg -i "$ASSET" >/dev/null 2>&1 &
elif command -v xdg-open >/dev/null 2>&1; then
    nohup xdg-open "$ASSET" >/dev/null 2>&1 &
else
    exit 1
fi
"#,
            status_message = status_message,
            asset = sh_quote(asset_path.to_string_lossy().as_ref()),
        ),
        UpdateTarget::LinuxAppImage => format!(
            r#"#!/bin/sh
set -eu
STATUS_PID=""
STATUS_MESSAGE={status_message}
start_status() {{
    if command -v zenity >/dev/null 2>&1; then
        zenity --info --title="Updating Edolview" --text="$STATUS_MESSAGE" --no-wrap --timeout=600 >/dev/null 2>&1 &
        STATUS_PID=$!
    elif command -v notify-send >/dev/null 2>&1; then
        notify-send "Updating Edolview" "$STATUS_MESSAGE"
    fi
}}
stop_status() {{
    if [ -n "${{STATUS_PID:-}}" ]; then
        kill "$STATUS_PID" 2>/dev/null || true
    fi
}}
trap 'stop_status' EXIT
start_status
PID={pid}
while kill -0 "$PID" 2>/dev/null; do
    sleep 0.3
done
ASSET={asset}
CURRENT_EXE={current_exe}
install -m 755 "$ASSET" "$CURRENT_EXE"
nohup "$CURRENT_EXE" >/dev/null 2>&1 &
"#,
            status_message = status_message,
            asset = sh_quote(asset_path.to_string_lossy().as_ref()),
            current_exe = sh_quote(current_exe.to_string_lossy().as_ref()),
        ),
        _ => return Err("Unsupported Unix update target".to_string()),
    };

    let mut file = File::create(path).map_err(|e| format!("Failed to create update helper script: {e}"))?;
    file.write_all(script.as_bytes())
        .map_err(|e| format!("Failed to write update helper script: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = file
            .metadata()
            .map_err(|e| format!("Failed to read update helper metadata: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).map_err(|e| format!("Failed to mark update helper executable: {e}"))?;
    }

    Ok(())
}

fn normalize_version_label(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with(['v', 'V']) {
        trimmed.to_string()
    } else {
        format!("v{trimmed}")
    }
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate_parts), Some(current_parts)) => compare_versions(&candidate_parts, &current_parts).is_gt(),
        _ => normalize_version_label(candidate) != normalize_version_label(current),
    }
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let core = version.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;

    let mut parts = Vec::new();
    for part in core.split('.') {
        if part.is_empty() {
            return None;
        }
        parts.push(part.parse::<u64>().ok()?);
    }
    Some(parts)
}

fn compare_versions(left: &[u64], right: &[u64]) -> std::cmp::Ordering {
    let max_len = left.len().max(right.len());
    for idx in 0..max_len {
        let lhs = *left.get(idx).unwrap_or(&0);
        let rhs = *right.get(idx).unwrap_or(&0);
        match lhs.cmp(&rhs) {
            std::cmp::Ordering::Equal => {}
            non_eq => return non_eq,
        }
    }
    std::cmp::Ordering::Equal
}

#[cfg(target_os = "windows")]
fn ps_quote_path(path: &Path) -> String {
    let escaped = path.to_string_lossy().replace('\'', "''");
    format!("'{escaped}'")
}

#[cfg(target_os = "windows")]
fn ps_quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn status_message(target: UpdateTarget) -> &'static str {
    match target {
        UpdateTarget::PortableZip => "Edolview is installing the downloaded update and will reopen automatically.",
        UpdateTarget::WindowsMsi | UpdateTarget::MacDmg | UpdateTarget::LinuxDeb => {
            "Edolview is handing the update over to the installer. Follow any installer prompts to finish updating."
        }
        UpdateTarget::LinuxAppImage => "Edolview is replacing the AppImage and will reopen automatically.",
    }
}

#[cfg(target_os = "macos")]
fn app_bundle_path(path: &Path) -> Option<PathBuf> {
    path.ancestors().find_map(|ancestor| {
        let ext = ancestor.extension()?.to_str()?;
        if ext.eq_ignore_ascii_case("app") {
            Some(ancestor.to_path_buf())
        } else {
            None
        }
    })
}

#[cfg(not(target_os = "macos"))]
fn app_bundle_path(_path: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::is_newer_version;

    #[test]
    fn detects_newer_semver_versions() {
        assert!(is_newer_version("v0.4.20", "0.4.19"));
        assert!(is_newer_version("0.5.0", "0.4.20"));
        assert!(is_newer_version("0.4.20.1", "0.4.20"));
    }

    #[test]
    fn ignores_same_or_older_versions() {
        assert!(!is_newer_version("v0.4.20", "0.4.20"));
        assert!(!is_newer_version("0.4.19", "0.4.20"));
        assert!(!is_newer_version("0.4", "0.4.0"));
    }
}
