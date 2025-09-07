# edolview

간단한 OpenCV 기반 이미지 뷰어 (Rust)

## 기능
- 다양한 이미지 포맷 로드 (OpenCV 지원 포맷)
- 최대 창 크기에 맞춰 축소 (옵션)
- 그레이스케일 변환 (옵션)
- ESC 또는 q 로 종료

## 사용법
```
# Cargo 빌드
cargo build --release

# 실행 (기본)
cargo run -- path/to/image.jpg

# 창 제목 지정
cargo run -- path/to/image.jpg --title "My Photo"

# 최대 크기 1280x720 으로 맞춰서(너무 크면 축소)
cargo run -- path/to/large.png --max-size 1280x720

# 그레이스케일 보기
cargo run -- path/to/image.jpg --grayscale
```

## Windows 환경에서 OpenCV 설치
opencv-rust 크레이트는 시스템에 OpenCV 라이브러리를 요구합니다.

### 1) vcpkg 사용 (권장)
1. vcpkg 설치 후 환경 변수 설정
```
git clone https://github.com/microsoft/vcpkg.git
./vcpkg/bootstrap-vcpkg.bat
```
2. OpenCV 설치
```
./vcpkg/vcpkg install opencv[contrib]:x64-windows-static-md
```
3. `VCPKG_ROOT` 환경 변수 설정 및 `cargo build`

### 2) OpenCV 공식 배포본 사용
- https://opencv.org/releases 에서 Windows 패키지 다운로드
- `OpenCV_DIR` 환경 변수 (예: `C:\opencv\build`) 설정
- `PATH` 에 `C:\opencv\build\x64\vc16\bin` 추가

## TODO (향후 개선)
- 폴더 탐색 및 N/P 키로 다음/이전 이미지
- 확대/축소 (zoom) 및 패닝
- EXIF 회전 자동 적용
- 드래그앤드롭 지원 (winit/egui 연계 고려)

## 라이선스
MIT
