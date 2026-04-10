# Agent Instructions

## Validation

- After making code changes, run `cargo fmt`.
- After Rust code changes, run `cargo build`.
- If `cargo build` is blocked because a running `edolview` process is holding the executable open (for example from a VS Code debug session), terminate the `edolview` process and retry the build.

## Generated assets and packaging

- If you touch `icon.svg`, `icons\`, `build.rs`, or startup icon loading, run `cargo run -p xtask -- icons` before validating.
- Keep generated icon files such as `icons\icon.png` and `icons\app.ico` in sync with code and packaging references.
- Treat files under `packaging\` as generated outputs unless you have confirmed otherwise.
- After changing packaging generation logic, regenerate assets with `cargo run -p xtask -- icons` so committed packaging outputs stay in sync with the source definitions.

## Release and versioning

- Treat `Cargo.toml` as the single source of truth for the app version.
- When preparing a release, use `cargo run -p xtask -- release-version <x.y.z>` instead of editing the version and git tag separately by hand.
- If updating `Cargo.toml`, include the `Cargo.lock` change in the release commit.
- After running that command, push both `main` and the new `v<x.y.z>` tag.
- Do not create or push a release tag whose version does not match `Cargo.toml`.

## Unsafe, OpenGL, and image-core changes

- Treat `src\ui\gl\`, `src\ui\image_viewer.rs`, `src\model\image.rs`, and `src\model\image_io.rs` as safety-sensitive code.
- Keep `unsafe` blocks minimal and document invariants when changing them.

## Performance and concurrency rules

- This app is real-time. Prioritize low-latency interaction and immediate UI feedback.
- Keep heavy or latency-sensitive work off the egui UI thread.
- Preserve existing background-thread, channel, and `request_repaint` patterns.
- Avoid unnecessary full-image copies.
- Reuse existing shared ownership and caching patterns such as `Arc`, `Mutex`, `OnceLock`, and shared asset storage.

## Format support changes

- If you add a new image format, update the loader, the supported extension list in `src\model\file_nav.rs`, and the README supported-format list together.

## Socket protocol compatibility

- If you change the socket protocol, consider compatibility with the Python package and VS Code extension mentioned in the README.
- Document any breaking protocol change explicitly.

## Error handling

- Prefer existing `color_eyre` error propagation patterns.
- For user-visible failures, surface them through the current toast and `eprintln!` flow instead of failing silently.
