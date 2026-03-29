# Distribution automation

This document tracks the release packaging and store distribution automation for `edolview-rs`.

## Current status

| Target | Status | Notes |
| --- | --- | --- |
| GitHub Release zip artifacts | Done | Existing pipeline keeps publishing `edolview-windows.zip`, `edolview-mac.zip`, and `edolview-linux.zip`. |
| GitHub Release MSI | Done | Windows tag builds now produce an `.msi` and upload it to the GitHub Release. |
| GitHub Release DMG | Done | macOS tag builds now create an `.app` bundle, wrap it in a `.dmg`, and upload it to the GitHub Release. |
| GitHub Release DEB | Done | Linux tag builds now create a `.deb` package and upload it to the GitHub Release. |
| GitHub Release AppImage | Done | Linux tag builds now create an AppImage and upload it to the GitHub Release. |
| Microsoft Store | Planned | Requires MSIX packaging, signing, and Store submission credentials. |
| Mac App Store | Planned | Requires App Sandbox, entitlements, signing, notarization/App Store Connect packaging, and upload automation. |
| Flatpak / Flathub | Planned | Initial Flathub submission is manual; later updates can be automated after onboarding. |
| Snap | Planned | Can be automated after Snap metadata and store credentials are added. |

## Phase 1: GitHub Release installers

Phase 1 keeps the current zip artifacts and adds installer/package artifacts to the same GitHub Release:

- Windows: `edolview-installer-msi.msi`
- macOS: `edolview-installer-dmg.dmg`
- Linux: `edolview-installer-deb.deb`
- Linux: `edolview-installer-appimage.AppImage`

Installer artifacts are uploaded separately so AppImage and DEB do not get bundled into a single Linux installer zip.

### Packaging layout decisions

- The app loads `colormap` relative to the executable directory.
- Every installer therefore ships `colormap` beside the executable, not in a shared data path.
- The DEB package installs the app into `/opt/edolview` and exposes `/usr/bin/edolview` as a launcher script.
- The AppImage packages `colormap` inside the AppDir next to the shipped binary.
- The macOS `.app` bundle stores `colormap` inside `Contents/MacOS` so the current runtime lookup continues to work.
- The MSI installs the main executable and `colormap` together under the application install directory.

### File association status

- Windows MSI currently registers supported extensions as file associations and `Open With` candidates.
- Linux desktop packaging currently declares supported MIME types through the generated `.desktop` file.
- These changes help the app appear as an available image viewer, but they do **not** force the OS default app to switch automatically.
- Automatic default-app selection still needs platform-specific follow-up work and, on modern Windows, may require sending the user to the system default-app settings UI.

## Phase 2: Store automation roadmap

### Microsoft Store

Planned work:

- Add MSIX packaging.
- Add certificate/signing flow.
- Add Store submission automation with repository secrets.

### Mac App Store

Planned work:

- Audit sandbox compatibility.
- Add entitlements and app bundle metadata required for App Store distribution.
- Add signing and App Store Connect upload automation.

### Flatpak / Flathub

Planned work:

- Add Flatpak manifest and local build job.
- Submit the app to Flathub manually once.
- After onboarding, automate manifest updates/releases.

### Snap

Planned work:

- Add `snapcraft.yaml`.
- Add Snap build and publish jobs.
- Publish automatically with Snap Store credentials.

## Required secrets for later phases

These are **not** required for phase 1:

- Microsoft Store submission credentials
- Apple signing certificates / App Store Connect credentials
- Flathub automation credentials or bot workflow setup
- Snap Store token

## Notes

- Phase 1 is implemented in `.github/workflows/ci.yml`.
- `packaging/` is treated as generated output and is recreated by `cargo run -p xtask -- icons`.
- Store uploads are intentionally left for a later phase because each store has extra policy, signing, and onboarding requirements.
