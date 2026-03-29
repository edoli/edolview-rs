use anyhow::{bail, Context, Result};
use ico::IconDirEntry;
use oxipng::{self, Interlacing, Options, StripChunks};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[allow(dead_code)]
#[path = "../../src/supported_image.rs"]
mod supported_image;

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

/// Rasterize `icon.svg` → `icons/icon.png` (512x512) and then run existing pipelines.
fn generate_all_icons() -> Result<()> {
    let svg_src = Path::new("icon.svg");
    if !svg_src.exists() {
        bail!("missing icon.svg");
    }

    fs::create_dir_all("icons")?;

    let base_png = Path::new("icons/icon.png");
    rasterize_svg_to_png(svg_src, base_png, (512, 512)).expect("SVG → PNG rasterization failed");

    // from here on, reuse the existing PNG-based pipeline with our freshly rendered icon.png
    generate_windows_ico(base_png, Path::new("icons/app.ico")).context("Windows .ico generation failed")?;
    generate_linux_pngs(base_png, Path::new("icons/hicolor")).context("Linux hicolor png generation failed")?;

    // macOS icns (only if iconutil present + on macOS host)
    if which::which("iconutil").is_ok() {
        generate_macos_icns(base_png, Path::new(format!("icons/{APP_NAME}.icns").as_str()))?;
    } else {
        eprintln!("[warn] 'iconutil' not found; skipping ICNS. Run this on macOS to create icons/{APP_NAME}.icns");
    }

    write_linux_app_run("packaging/linux/AppRun")?;
    write_linux_wrapper("packaging/linux/edolview-wrapper.sh")?;
    write_linux_desktop(format!("packaging/{APP_NAME_LC}.desktop").as_str())?;
    write_macos_info_plist_template("packaging/macos/Info.plist.in")?;
    write_windows_license_rtf("packaging/windows/License.rtf")?;
    write_windows_wxs("packaging/windows/edolview.wxs")?;

    println!("icons generated in ./icons");
    Ok(())
}

pub fn rasterize_svg_to_png(svg_path: &Path, png_out: &Path, size: (u32, u32)) -> Result<(), String> {
    use resvg::{
        tiny_skia::Pixmap,
        usvg::{Transform, Tree},
    };

    let data = fs::read(svg_path).map_err(|e| format!("failed to read SVG file: {e}"))?;
    let options = resvg::usvg::Options::default();
    let rtree = Tree::from_data(&data, &options).map_err(|err| err.to_string())?;

    let source_size = (rtree.size().width(), rtree.size().height());

    let mut pixmap = Pixmap::new(size.0, size.1)
        .ok_or_else(|| format!("Failed to create SVG Pixmap of size {}x{}", size.0, size.1))?;

    resvg::render(
        &rtree,
        Transform::from_scale(size.0 as f32 / source_size.0, size.1 as f32 / source_size.1),
        &mut pixmap.as_mut(),
    );

    let png = pixmap.encode_png().map_err(|e| format!("failed to encode PNG: {e}"))?; // tiny-skia가 PNG 인코딩 제공

    let mut opts = Options::from_preset(6); // 6 is max compression, slowest
    opts.strip = StripChunks::Safe;
    opts.interlace = Some(Interlacing::None);

    let optimized = oxipng::optimize_from_memory(&png, &opts).map_err(|e| format!("oxipng optimize failed: {e}"))?;

    fs::write(png_out, optimized).map_err(|e| format!("failed to save optimized PNG: {e}"))?;

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

    // Apple recommended sizes (keep square). Note: true iconset typically includes @2x variants;
    // we keep the simple variant consistent with previous code.
    let sizes = [16u32, 32, 64, 128, 256, 512, 1024];
    for &sz in &sizes {
        let resized = img.resize_exact(sz, sz, FilterType::Lanczos3);
        let rgba8 = match resized {
            DynamicImage::ImageRgba8(buf) => buf,
            _ => resized.to_rgba8(),
        };
        let filename = format!("icon_{sz}x{sz}.png");
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
    let mime_types = supported_image::supported_image_mime_types().join(";");
    let mime_line = if mime_types.is_empty() {
        String::new()
    } else {
        format!("MimeType={mime_types};\n")
    };
    let desktop = format!(
        r#"[Desktop Entry]
Type=Application
Name={APP_NAME}
Exec={APP_NAME_LC}
Icon={APP_NAME_LC}
Terminal=false
Categories=Utility;
{mime_line}"#
    );
    fs::write(path, desktop)?;
    Ok(())
}

fn write_linux_app_run(path: &str) -> Result<()> {
    fs::create_dir_all(Path::new(path).parent().unwrap())?;
    fs::write(
        path,
        r#"#!/bin/sh
SELF_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
exec "$SELF_DIR/usr/bin/edolview" "$@"
"#,
    )?;
    Ok(())
}

fn write_linux_wrapper(path: &str) -> Result<()> {
    fs::create_dir_all(Path::new(path).parent().unwrap())?;
    fs::write(
        path,
        r#"#!/bin/sh
exec /opt/edolview/edolview "$@"
"#,
    )?;
    Ok(())
}

fn write_macos_info_plist_template(path: &str) -> Result<()> {
    fs::create_dir_all(Path::new(path).parent().unwrap())?;
    let extensions = supported_image::supported_image_extensions()
        .into_iter()
        .map(|ext| format!("            <string>{ext}</string>"))
        .collect::<Vec<_>>()
        .join("\n");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>English</string>
    <key>CFBundleDisplayName</key>
    <string>Edolview</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeExtensions</key>
            <array>
{extensions}
            </array>
            <key>CFBundleTypeName</key>
            <string>Supported image files</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
        </dict>
    </array>
    <key>CFBundleExecutable</key>
    <string>edolview</string>
    <key>CFBundleIconFile</key>
    <string>edolview.icns</string>
    <key>CFBundleIdentifier</key>
    <string>kr.edoli.edolview</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Edolview</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>__SHORT_VERSION__</string>
    <key>CFBundleVersion</key>
    <string>__VERSION__</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#
    );
    fs::write(path, plist)?;
    Ok(())
}

fn write_windows_wxs(path: &str) -> Result<()> {
    fs::create_dir_all(Path::new(path).parent().unwrap())?;

    let formats = supported_image::supported_image_formats();

    let components = formats
        .iter()
        .map(|format| {
            let ext_id = format.ext.to_ascii_uppercase();
            let prog_id = format!("Edolview.{}", ext_id);
            let component_id = format!("cmpAssoc{}", ext_id);
            let open_with_component_id = format!("cmpOpenWith{}", ext_id);
            format!(
                r#"          <Component Id="{component_id}" Guid="*">
            <RegistryKey Root="HKLM" Key="Software\Classes\{prog_id}">
              <RegistryValue Type="string" Value="Edolview {ext_id} file" KeyPath="yes" />
              <RegistryKey Key="DefaultIcon">
                <RegistryValue Type="string" Value="[INSTALLFOLDER]edolview.exe,0" />
              </RegistryKey>
              <RegistryKey Key="shell\open\command">
                <RegistryValue Type="string" Value="&quot;[INSTALLFOLDER]edolview.exe&quot; &quot;%1&quot;" />
              </RegistryKey>
            </RegistryKey>
          </Component>
          <Component Id="{open_with_component_id}" Guid="*">
            <RegistryKey Root="HKLM" Key="Software\Classes\.{ext}\OpenWithProgids">
              <RegistryValue Name="{prog_id}" Type="string" Value="" KeyPath="yes" />
            </RegistryKey>
            <RegistryKey Root="HKLM" Key="Software\Classes\Applications\edolview.exe\SupportedTypes">
              <RegistryValue Name=".{ext}" Type="string" Value="" />
            </RegistryKey>
          </Component>"#,
                ext = format.ext,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let component_refs = formats
        .iter()
        .map(|format| {
            let ext_id = format.ext.to_ascii_uppercase();
            format!("      <ComponentRef Id=\"cmpAssoc{ext_id}\" />\n      <ComponentRef Id=\"cmpOpenWith{ext_id}\" />")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let wxs = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Product
    Id="*"
    Name="Edolview"
    Language="1033"
    Version="$(var.ProductVersion)"
    Manufacturer="Daniel Jeon"
    UpgradeCode="9B95884C-31AA-4A5C-8209-D7CE6CA9C2D8">
    <Package InstallerVersion="500" Compressed="yes" InstallScope="perMachine" Platform="x64" />
    <MajorUpgrade DowngradeErrorMessage="A newer version of [ProductName] is already installed." />
    <MediaTemplate EmbedCab="yes" CompressionLevel="high" />

    <Icon Id="ProductIcon" SourceFile="$(var.ProjectDir)\icons\app.ico" />
    <Property Id="ARPPRODUCTICON" Value="ProductIcon" />
    <Property Id="WIXUI_INSTALLDIR" Value="INSTALLFOLDER" />
    <UIRef Id="WixUI_Mondo" />
    <WixVariable Id="WixUILicenseRtf" Value="$(var.ProjectDir)\packaging\windows\License.rtf" />

    <Directory Id="TARGETDIR" Name="SourceDir">
      <Directory Id="ProgramFiles64Folder">
        <Directory Id="INSTALLFOLDER" Name="Edolview">
          <Component Id="cmpMainExe" Guid="D7781E26-5FAE-4F3F-8B92-BD40530A4628">
            <File Id="filMainExe" Source="$(var.ReleaseBinDir)\edolview.exe" KeyPath="yes" Checksum="yes" />
          </Component>
        </Directory>
      </Directory>
      <Directory Id="ProgramMenuFolder">
        <Directory Id="ApplicationProgramsFolder" Name="Edolview" />
      </Directory>
      <Directory Id="DesktopFolder" Name="Desktop" />
    </Directory>

    <Feature
      Id="MainFeature"
      Title="Edolview"
      Description="Image viewer for visualization and analysis"
      Level="1"
      Display="expand"
      ConfigurableDirectory="INSTALLFOLDER"
      AllowAdvertise="no"
      Absent="disallow">
      <ComponentRef Id="cmpMainExe" />
      <ComponentGroupRef Id="ColormapFiles" />
      <ComponentGroupRef Id="FileAssociations" />
      <Feature
        Id="StartMenuShortcutsFeature"
        Title="Start Menu shortcuts"
        Description="Create Start Menu shortcuts, including an uninstall entry."
        Level="1"
        AllowAdvertise="no"
        Absent="allow">
        <ComponentRef Id="cmpStartMenuShortcut" />
      </Feature>
      <Feature
        Id="DesktopShortcutFeature"
        Title="Desktop shortcut"
        Description="Create a shortcut on the desktop."
        Level="1"
        AllowAdvertise="no"
        Absent="allow">
        <ComponentRef Id="cmpDesktopShortcut" />
      </Feature>
    </Feature>
  </Product>

  <Fragment>
    <ComponentGroup Id="FileAssociations">
{component_refs}
    </ComponentGroup>
  </Fragment>

  <Fragment>
    <DirectoryRef Id="INSTALLFOLDER">
{components}
    </DirectoryRef>
  </Fragment>

  <Fragment>
    <DirectoryRef Id="ApplicationProgramsFolder">
      <Component Id="cmpStartMenuShortcut" Guid="*">
        <Shortcut
          Id="StartMenuShortcut"
          Name="Edolview"
          Description="Image viewer for visualization and analysis"
          Target="[INSTALLFOLDER]edolview.exe"
          WorkingDirectory="INSTALLFOLDER"
          Icon="ProductIcon"
          IconIndex="0" />
        <Shortcut
          Id="UninstallShortcut"
          Name="Uninstall Edolview"
          Description="Remove Edolview from this computer"
          Target="[SystemFolder]msiexec.exe"
          Arguments="/x [ProductCode]" />
        <RemoveFolder Id="RemoveApplicationProgramsFolder" On="uninstall" />
        <RegistryValue Root="HKCU" Key="Software\Edolview" Name="StartMenuShortcut" Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>
  </Fragment>

  <Fragment>
    <DirectoryRef Id="DesktopFolder">
      <Component Id="cmpDesktopShortcut" Guid="*">
        <Shortcut
          Id="DesktopShortcut"
          Name="Edolview"
          Description="Image viewer for visualization and analysis"
          Target="[INSTALLFOLDER]edolview.exe"
          WorkingDirectory="INSTALLFOLDER"
          Icon="ProductIcon"
          IconIndex="0" />
        <RegistryValue Root="HKCU" Key="Software\Edolview" Name="DesktopShortcut" Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>
  </Fragment>
</Wix>
"#
    );
    fs::write(path, wxs)?;
    Ok(())
}

fn write_windows_license_rtf(path: &str) -> Result<()> {
    fs::create_dir_all(Path::new(path).parent().unwrap())?;

    let license_text = fs::read_to_string("LICENSE").context("failed to read LICENSE")?;
    let escaped = license_text
        .replace('\\', r"\\")
        .replace('{', r"\{")
        .replace('}', r"\}")
        .replace('\r', "")
        .replace(
            '\n', r"\par
",
        );

    let rtf = format!(
        "{{\\rtf1\\ansi\\deff0{{\\fonttbl{{\\f0\\fnil Consolas;}}}}\\viewkind4\\uc1\\pard\\f0\\fs20 {escaped}\\par\n}}"
    );
    fs::write(path, rtf)?;
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
