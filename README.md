# EdolView - Image Viewer for Visualization and Analysis

A lightning-fast, versatile image viewer 🚀
Designed for researchers, engineers, and graphics professionals who need **speed, flexibility, and powerful visualization**.


* ⚡ **Blazing Fast Startup & I/O**: Instant startup and rapid image loading without delays
* 📂 **Wide Format Support**: Open and explore all major research and [graphics formats](#supported-file-formats)
* 🎨 **Colormap Visualization**: Go beyond raw images with rich colormap-based data visualization
* 🔧 **Flexible Display Controls**: Fine-tune normalization, exposure, gamma, and offset with ease

![Screenshot](images/screenshot.jpg)

## Installation

- __Windows__:
  - [edolview-windows.zip](https://github.com/edoli/edolview-rs/releases/latest/download/edolview-windows.zip)
  - [edolview-installer-msi.msi](https://github.com/edoli/edolview-rs/releases/latest/download/edolview-installer-msi.msi)
- __macOS__:
  - [edolview-mac.zip](https://github.com/edoli/edolview-rs/releases/latest/download/edolview-mac.zip)
  - [edolview-installer-dmg.dmg](https://github.com/edoli/edolview-rs/releases/latest/download/edolview-installer-dmg.dmg)
- __Linux__:
  - [edolview-linux.zip](https://github.com/edoli/edolview-rs/releases/latest/download/edolview-linux.zip)
  - [edolview-installer-appimage.AppImage](https://github.com/edoli/edolview-rs/releases/latest/download/edolview-installer-appimage.AppImage)
  - [edolview-installer-deb.deb](https://github.com/edoli/edolview-rs/releases/latest/download/edolview-installer-deb.deb)

## Usage

### Shortcuts

* **Navigation**
  * `←` / `→` : navigate image files in current directory
  * `page up` / `page down` : navigate loaded assets
  * `+` / `-`, `scroll` : zoom in / out
  * `r` : reset view
  * `f11` : fullscreen mode

* **Selection**
  * `shift` + `drag` : select rectangle region
  * `ctrl` / `cmd` + `a` : select full image
  * `esc` : deselect region
  * `ctrl` / `cmd` + `c` : copy image or selected region
  * `ctrl` / `cmd` + `s` : save image or selected region

* **Bookmarks**
  * `b` : toggle bookmarks window
  * `ctrl` / `cmd` + `b` : add or remove current selection bookmark
  * `[` / `]` : jump to previous / next bookmark

* **View Presets**
  * `ctrl` / `cmd` + `1` .. `9` : save current view preset to slot 1 .. 9
  * `alt` + `1` .. `9` : apply view preset from slot 1 .. 9

### Features

* Color maps: **Edolview** supports various color maps. To get more information of color maps, see the wiki page: https://github.com/edoli/edolview-rs/wiki/Color-Map
 
* **Remote Viewer**: **Edolview** can also be controlled remotely over the network. The current protocol is implemented in the [Python package](https://pypi.org/project/edolview/) and the [VS Code extension](https://marketplace.visualstudio.com/items?itemName=edolview-vscode.edolview). See [Remote-Viewer](https://github.com/edoli/edolview-rs/wiki/Remote%E2%80%90Viewer).

* **Copy-Paste Selected Region**

https://github.com/user-attachments/assets/fd76159c-da77-4f5d-946c-667cda5c1482

* **Plot Selected Region**

https://github.com/user-attachments/assets/4a219f8b-39f3-48a8-a5ea-b9d610bb3f40


## How to Build

The project is Rust-based and uses **OpenCV 4.12.0** for image decoding and processing.
Optional HEIF/HEIC support can be enabled with `--features heif` after preparing `libheif`.

### 0) Prerequisites (all platforms)
* Rust
* CMake, LLVM/Clang
* curl, unzip
* pkg-config (Linux/macOS)
* Git
* Ninja (Windows)


### 1) Install Rust

**Linux / macOS**

```bash
# installs rustup + stable toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# then reload shell or:
source "$HOME/.cargo/env"

# Check installation
rustc -V && cargo -V
```

**Windows (PowerShell)**

```powershell
# Option A: winget
winget install Rustlang.Rustup

# Option B: official installer
# https://win.rustup.rs/x86_64 (downloads rustup-init.exe)

# Check installation
rustc -V; cargo -V
```

### 2) Prepare OpenCV

**Linux (Ubuntu)**

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential cmake git curl unzip pkg-config \
  clang libclang-dev llvm-dev nasm

git clone -b release https://github.com/edoli/opencv-edolview.git opencv
cd opencv
sh cmake_script.sh

export PKG_CONFIG_PATH="$(pwd)/../install/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export OPENCV_PKGCONFIG=1 OPENCV_LINK_STATIC=1 PKG_CONFIG_ALL_STATIC=1
```

**macOS**

```bash
brew update
brew install cmake llvm git pkg-config nasm

LLVM_PREFIX="$(brew --prefix llvm)"
export LIBCLANG_PATH="$LLVM_PREFIX/lib"
export DYLD_LIBRARY_PATH="$LLVM_PREFIX/lib:${DYLD_LIBRARY_PATH:-}"
export PATH="$LLVM_PREFIX/bin:$PATH"

git clone -b release https://github.com/edoli/opencv-edolview.git opencv
cd opencv
sh cmake_script.sh

export PKG_CONFIG_PATH="$(pwd)/../install/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export OPENCV_PKGCONFIG=1 OPENCV_LINK_STATIC=1 PKG_CONFIG_ALL_STATIC=1
```

**Windows (PowerShell)**

```powershell
choco install -y --no-progress llvm cmake ninja git curl unzip nasm

$llvmBin = "C:\Program Files\LLVM\bin"
if (Test-Path $llvmBin) {
  $env:PATH = "$llvmBin;$env:PATH"
  $env:LIBCLANG_PATH = $llvmBin
}

git clone -b release https://github.com/edoli/opencv-edolview.git opencv
Push-Location opencv
pwsh .\cmake_script.ps1

# Optional helpers for linking
$installDir = Join-Path $pwd "..\install"
$libPath = "$installDir\x64\vc17\staticlib"
$libs = Get-ChildItem -Path $libPath -Filter *.lib | ForEach-Object { $_.BaseName }
$libsJoined = $libs -join ","

$env:OPENCV_LINK_LIBS=$libsJoined
$env:OPENCV_LINK_PATHS = $libPath
$env:OPENCV_INCLUDE_PATHS = "$installDir\include"
$env:OPENCV_LINK_STATIC = "1"
Pop-Location
```

### 3) Optional HEIF/HEIC support

Install and build the vcpkg dependencies before using `--features heif`:

```bash
cargo install cargo-vcpkg
cargo vcpkg -v build
```

On Linux and macOS, add the vcpkg pkg-config directory before building with `--features heif`:

```bash
# Linux x86_64
export PKG_CONFIG_PATH="$(pwd)/target/vcpkg/installed/x64-linux-release/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

# macOS arm64
export PKG_CONFIG_PATH="$(pwd)/target/vcpkg/installed/arm64-osx-release/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
```

Windows uses the vcpkg triplet from `Cargo.toml` and does not need an extra `PKG_CONFIG_PATH` setting.

### 4) Build the viewer

```bash
# generate resources for the app
cargo run -p xtask -- icons
# from project root
cargo build --release
# enable optional HEIF/HEIC support when libheif is available
# cargo run -p xtask --features heif -- icons
# cargo build --release --features heif
# or run directly
cargo run
```

**Built executable files path**

* Linux/macOS: `target/release/edolview`
* Windows: `target/release/edolview.exe`

---

### Troubleshooting

* `pkg-config: opencv4 not found` → set `PKG_CONFIG_PATH` to your OpenCV’s `lib/pkgconfig`.
* Windows link errors → ensure `OPENCV_LINK_PATHS`/`OPENCV_INCLUDE_PATHS` point to your static build.
* macOS libclang issues → ensure Homebrew LLVM is on `PATH`, with `LIBCLANG_PATH`/`DYLD_LIBRARY_PATH` set.

## Supported file formats
EdolView accepts the following file extensions in the current build:

* Common image formats: `png`, `jpg`, `jpeg`, `jpe`, `jp2`, `bmp`, `dib`, `tif`, `tiff`, `webp`
* HDR and analysis formats: `exr`, `hdr`, `pic`, `raw`, `pfm`, `flo`
* Netpbm family: `pbm`, `pgm`, `ppm`, `pnm`, `pxm`
* Other raster formats: `sr`
* Optional HEIF support: `heic`, `heif`

## License
EdolView is available under the MIT license.
