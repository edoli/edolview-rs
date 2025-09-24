use anyhow::{bail, Context, Result};
use ico::IconDirEntry;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const APP_NAME: &str = "Edolview";
const APP_NAME_LC: &str = "edolview";

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let cmd = args.get(0).map(String::as_str).unwrap_or("");
    match cmd {
        "icons" => generate_all_icons(),
        "install-linux" => install_linux_assets(),
        _ => {
            eprintln!("Usage:\n cargo run -p xtask -- icons\n cargo run -p xtask -- install-linux");
            Ok(())
        }
    }
}

fn generate_all_icons() -> Result<()> {
    let src = Path::new("icon.png");
    if !src.exists() {
        bail!("missing icon.png");
    }

    fs::create_dir_all("icons")?;

    generate_windows_ico(src, Path::new("icons/app.ico")).context("Windows .ico generation failed")?;

    generate_linux_pngs(src, Path::new("icons/hicolor")).context("Linux hicolor png generation failed")?;

    // macOS icns (only if iconutil present + on macOS host)
    if which::which("iconutil").is_ok() {
        generate_macos_icns(src, Path::new(format!("icons/{APP_NAME}.icns").as_str()))?;
    } else {
        eprintln!("[warn] 'iconutil' not found; skipping ICNS. Run this on macOS to create icons/{APP_NAME}.icns");
    }

    // Generate Linux .desktop template
    write_linux_desktop(format!("packaging/{APP_NAME_LC}.desktop").as_str())?;

    println!("icons generated in ./icons");
    Ok(())
}

fn generate_windows_ico(src_png: &Path, out_ico: &Path) -> Result<()> {
    use ico::{IconDir, IconImage};
    use image::{imageops::FilterType, DynamicImage};

    let bytes = fs::read(src_png)?;
    let img = image::load_from_memory(&bytes)?;

    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let mut dir = IconDir::new(ico::ResourceType::Icon);

    for &sz in &sizes {
        let resized = img.resize_exact(sz, sz, FilterType::Lanczos3);
        let rgba = match resized {
            DynamicImage::ImageRgba8(buf) => buf,
            _ => resized.to_rgba8(),
        };
        let icon = IconImage::from_rgba_data(sz, sz, rgba.into_raw());
        dir.add_entry(IconDirEntry::encode(&icon)?);
    }

    let mut f = fs::File::create(out_ico)?;
    dir.write(&mut f)?;
    Ok(())
}

fn generate_linux_pngs(src_png: &Path, base: &Path) -> Result<()> {
    use image::{imageops::FilterType, DynamicImage};

    let bytes = fs::read(src_png)?;
    let img = image::load_from_memory(&bytes)?;

    let sizes = [16u32, 24, 32, 48, 64, 128, 256, 512];
    for &sz in &sizes {
        let out = base.join(format!("{sz}x{sz}/apps"));
        fs::create_dir_all(&out)?;
        let resized = img.resize_exact(sz, sz, FilterType::Lanczos3);
        let rgba8 = match resized {
            DynamicImage::ImageRgba8(buf) => buf,
            _ => resized.to_rgba8(),
        };
        let dst = out.join(format!("{APP_NAME_LC}.png"));
        image::save_buffer(&dst, &rgba8, sz, sz, image::ColorType::Rgba8)?;
    }
    Ok(())
}

fn generate_macos_icns(src_png: &Path, out_icns: &Path) -> Result<()> {
    // Build a temporary .iconset directory of required sizes, then call `iconutil`.
    use image::{imageops::FilterType, DynamicImage};

    let tmp = PathBuf::from("icons/tmp.iconset");
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;

    let bytes = fs::read(src_png)?;
    let img = image::load_from_memory(&bytes)?;

    // Apple recommended sizes
    let sizes = [16u32, 32, 64, 128, 256, 512, 1024];
    for &sz in &sizes {
        let resized = img.resize_exact(sz, sz, FilterType::Lanczos3);
        let rgba8 = match resized {
            DynamicImage::ImageRgba8(buf) => buf,
            _ => resized.to_rgba8(),
        };
        let filename = if sz == 1024 {
            format!("icon_{sz}x{sz}.png")
        } else {
            format!("icon_{sz}x{sz}.png")
        };
        let dst = tmp.join(filename);
        image::save_buffer(&dst, &rgba8, sz, sz, image::ColorType::Rgba8)?;
    }

    // Run iconutil (macOS only)
    let status = Command::new("iconutil")
        .args(["-c", "icns", tmp.to_str().unwrap(), "-o", out_icns.to_str().unwrap()])
        .status()
        .context("failed to run iconutil")?;

    if !status.success() {
        bail!("iconutil failed with status: {:?}", status);
    }

    // Cleanup
    let _ = fs::remove_dir_all(&tmp);
    Ok(())
}

fn write_linux_desktop(path: &str) -> Result<()> {
    fs::create_dir_all(Path::new(path).parent().unwrap())?;
    let desktop = format!(r#"[Desktop Entry]
Type=Application
Name={APP_NAME}
Exec={APP_NAME_LC}
Icon={APP_NAME_LC}
Terminal=false
Categories=Utility;
"#);
    fs::write(path, desktop)?;
    Ok(())
}

fn install_linux_assets() -> Result<()> {
    let home = dirs::home_dir().context("cannot resolve home dir")?;
    let icons = Path::new("icons/hicolor");
    if !icons.exists() {
        bail!("Run: cargo run -p xtask -- icons");
    }

    // Copy icons
    for entry in walkdir::WalkDir::new(&icons) {
        let entry = entry?;
        let p = entry.path();
        if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("png") {
            let rel = p.strip_prefix(icons)?;
            let dest = home.join(".local/share/icons").join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(p, &dest)?;
        }
    }

    // Install .desktop
    let desktop_path = format!("packaging/{APP_NAME_LC}.desktop");
    let desktop_src = Path::new(desktop_path.as_str());
    if desktop_src.exists() {
        let dest = home.join(format!(".local/share/applications/{APP_NAME_LC}.desktop").as_str());
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(desktop_src, dest)?;
    } else {
        eprintln!("[warn] packaging/{APP_NAME_LC}.desktop missing; run icons task first");
    }

    println!("Installed Linux icon assets and desktop entry to ~/.local/share");
    Ok(())
}
