# edolview

A simple OpenCV-based image viewer written by rust

## Features
- Load various image formats (anything supported by OpenCV)
- Optionally downscale to fit within the maximum window size
- Optional grayscale conversion
- Exit with ESC or `q`

## Usage
```
# Build (optimized)
cargo build --release

# Run (basic)
cargo run -- path/to/image.jpg
```

## Installing OpenCV on Windows
The `opencv` Rust crate requires a native OpenCV installation on your system.

### 1) Using vcpkg (recommended)
1. Install vcpkg and set environment variables
```
git clone https://github.com/microsoft/vcpkg.git
./vcpkg/bootstrap-vcpkg.bat
```
2. Install OpenCV
```
.\vcpkg.exe install opencv4:x64-windows-static-md
```
3. Set `VCPKG_ROOT` and then `cargo build`.

If static build causes issues, try:
```
.\vcpkg.exe install opencv4:x64-windows
```

### 2) Using the official OpenCV distribution
- Download the Windows package from https://opencv.org/releases
- Set the `OpenCV_DIR` environment variable (e.g. `C:\opencv\build`)
- Add `C:\opencv\build\x64\vc16\bin` to `PATH`

## Roadmap / TODO
- Directory browsing & next/previous image via N / P keys
- Zoom and panning
- Automatic EXIF orientation handling
- Drag & drop support (maybe via winit / egui integration)

## License
MIT
