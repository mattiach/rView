# rView

A fast, minimal image viewer for Windows 11 (x64), written in Rust, with **Markup**: crop, rotate, shapes,
text, loupe and redactions that stay editable, saved as `.rvm` projects or exported as images. No C/C++ in the
build.

## Features

- Viewer: GPU rendering (wgpu/DX12), reduced previews of large JPEGs, preloading of the neighbours, folder
  navigation (natural sort, loop, random), slideshow, animations (GIF, APNG, WebP, JPEG XL), colour management
  (ICC, Display P3), multiple windows, fullscreen, drag & drop, Open URL, Copy/Paste (also Base64 images and data URLs: Paste or Open URL opens them, Copy as
  Base64 copies the file on screen), Rename, Delete with undo
  (Recycle Bin), Open With, File Info, Settings, custom shortcuts, English UI (drop-in
  translations in `assets/i18n`).
- Formats: JPEG, PNG/APNG, GIF, WebP, TIFF, BMP, ICO/CUR, ICNS, TGA, PNM, QOI, HDR, EXR, DDS, JPEG XL,
  SVG/SVGZ (re-rasterised when zoomed in), WBMP, XBM, XPM; through the Windows codecs HEIC/HEIF, AVIF, JPEG XR
  and camera RAW; `.rvm` projects.
- Markup (Ctrl+Shift+A): non-destructive crop (standard, social media and custom aspect ratios), rotate/flip,
  image adjustments (exposure, brightness, contrast, saturation, temperature, hue, sharpness), watermarks
  (an image placed, moved, resized, rotated, with opacity; the last two remembered), before/after comparison
  (a draggable divider, `\`), rectangle, ellipse, line, arrow (straight or
  curved), pen, highlighter, text (embedded Lato or installed fonts), loupe, redactions (Solid: an opaque
  cover, the only reliable way to hide data; Blur and Pixelate: visual effects, not a protection), selection
  with handles, undo/redo. Save Project (`.rvm`, safe to open from anyone), Export (PNG/JPEG/WebP/TIFF/BMP, metadata presets,
  previews always stripped), Overwrite Original (backup in the Recycle Bin), Copy. A `.rvm` project contains
  the whole original image, including what is under its redactions: keep it private and share an export.
- Convert (Tools menu), Export and Save Frame As: output format and JPEG quality, transparency kept or replaced
  with a background colour, resolution (72/96/150/300 or custom DPI, written to the file without resampling),
  resize (pixels with or without proportions, percentage, or print size in cm/in at the chosen DPI), metadata
  presets.

How it works inside: [ARCHITECTURE.md](ARCHITECTURE.md).
Measurements: [docs/performance.md](docs/performance.md). Library and platform behaviours checked by tests:
[docs/capabilities.md](docs/capabilities.md).

## Requirements

- Windows 11 x64.
- Rust 1.97 (pinned in `rust-toolchain.toml`; `rustup` installs it on the first build).
- Optional: [Inno Setup 6](https://jrsoftware.org/isinfo.php) for the installer, Python 3 for the test fixtures and
  `THIRD-PARTY-NOTICES.txt`.

## Commands

| Command                                                              | What it does                                                                                                                                                        |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo run -p rview -- <file>`                                       | Run a debug build, optionally opening a file or folder                                                                                                              |
| `cargo build --release -p rview`                                     | Release build: `target\release\rview.exe` (portable, ~23 MB)                                                                                                        |
| `powershell -ExecutionPolicy Bypass -File scripts\release.ps1 1.2.0` | Prepare a release: sets the version, builds `rview.exe`, regenerates `THIRD-PARTY-NOTICES.txt` and, with Inno Setup 6, `target\installer\rview-1.2.0-setup-x64.exe` |
| `iscc dist\win\rview.iss`                                            | Installer only, after the release build                                                                                                                             |
| `cargo test --workspace -q`                                          | Every test (unit, integration, end-to-end) with a compact output                                                                                                    |
| `cargo fmt --all --check`                                            | Formatting check                                                                                                                                                    |
| `cargo clippy --workspace --all-targets -- -D warnings`              | Lints, warnings as errors                                                                                                                                           |
| `cargo bench -p rv-core --bench decode`                              | Folder scan, decode and preview benchmarks                                                                                                                          |
| `cargo bench -p rv-core --bench markup`                              | Redaction patch, Markup overlay and export benchmarks                                                                                                               |
| `cargo bench -p rview --bench upload_mip`                            | GPU upload + mipmap benchmark                                                                                                                                       |
| `cargo run --profile fuzz -p rv-core --example fuzz_project -- 600`  | Fuzz the `.rvm` reader for 600 s                                                                                                                                    |

## Releases and CI

`.github/workflows/build.yml` runs on every push and pull request: it builds `rview.exe`, checks that
`THIRD-PARTY-NOTICES.txt` is up to date (`python scripts/third_party_notices.py --check`) and runs
`cargo audit` against the RustSec advisory database. It does not run the formatting check, Clippy or the
tests: run them locally (see Contributing).

To release, run `release.ps1 <version>`, commit, push, then push the tag `v<version>`. CI checks that the
tag matches `Cargo.toml` and, only for the repository owner's tags on `mattiach/rview`, publishes the
GitHub release with `rview-<version>-windows-x64.exe` and `THIRD-PARTY-NOTICES.txt` (the installer is
built locally). The update check compares the latest release tag (`v1.2.0` or `1.2.0`) with the version
of the running build.

## Tests

The end-to-end tests run the real app with its windows cloaked: nothing appears on screen or in the taskbar,
but the windows still exist, so let the run finish without using the mouse or keyboard on them. Every E2E test
prints one line (`e2e_markup: OK`, or `FAILED` with the reason). They need a DirectX 12 adapter (WARP works,
slower).

```
cargo test --workspace -q                                   # everything, compact
cargo test -p rv-core -q                                    # core only (no windows)
cargo test -p rview -q                                      # app: unit, Windows integration, E2E
cargo test -p rview --test e2e_markup                       # one E2E test
cargo test -p rv-core --test edit -- curved_arrow           # tests whose name contains "curved_arrow"
cargo test --release -p rview --test e2e_nav_perf           # navigation timings, thresholds checked in release
cargo test -p rv-core --test capabilities -- --nocapture --test-threads 1   # platform capability report
```

- Real-file fixtures are not in the repository: `python core/tests/fixtures/fetch_fixtures.py` downloads them
  (SHA-256 checked); without them those tests print `SKIPPED`. See `core/tests/fixtures/README.md`.
- `windows_integration`, `e2e_lifecycle` and `e2e_base64` replace the clipboard content; `windows_integration` passes a file
  through the Recycle Bin (restored).
- `uia_native` drives the native UI through UI Automation (menu bar, context menu, Delete confirmation, Open
  dialog, Settings): its windows and dialogs are **not** cloaked, they appear on screen for about half a
  minute; leave mouse and keyboard alone meanwhile. Without an interactive desktop it prints `SKIPPED`.
- Tests write only to temporary folders and never use `%APPDATA%\rView` or the real GitHub API.
- `RVIEW_GOLDEN_DIR=<folder>` writes the Markup golden images as PNG to look at.

## Install

- Portable: copy `rview.exe` anywhere (settings in `%APPDATA%\rView`).
- Installer: per-user (no administrator rights), Start menu, "Open with" for every supported type, Default
  apps registration, `.rvm` file type, uninstaller. Code signing: see the comments at the top of
  `dist\win\rview.iss` (needs your certificate).

## Icon

`assets/icon/rview.ico` (16–256 px) is embedded in `rview.exe` (Explorer, shortcuts, taskbar, title bar) and
used by the installer. To change the logo edit `assets/icon/make_icon.py` and run it (needs Pillow).

## Performance

Windows 11 Pro, Ryzen 7 7800X3D, Radeon RX 7900 XT, NVMe SSD, 2560×1440, release build:

| Measure                                                           | Time                                 |
| ----------------------------------------------------------------- | ------------------------------------ |
| Start → window visible (no file)                                  | 24 ms                                |
| Start → 12 MP JPEG on screen                                      | ~375 ms (mostly GPU device creation) |
| 50 MP JPEG: first image (reduced preview) / full resolution       | 94 ms / 260 ms                       |
| Next image, preloaded / not preloaded (12 MP JPEG)                | 5.2 ms / 65 ms                       |
| Holding → : last image after the last key                         | 70 ms                                |
| Folder of 10 000 files: scan + sort                               | 44 ms                                |
| Export PNG 24 MP with 50 shapes (render + encode)                 | 211 ms                               |
| Redaction 1000×1000 (Blur / Solid)                                | 28 / 13 ms                           |
| Memory with a 12 MP image open (Task Manager / total working set) | 151 / 192 MB                         |

Method and more measurements: [docs/performance.md](docs/performance.md). Known limitation: on PCs with
more than one GPU (e.g. a discrete GPU plus the processor's integrated one) start-up is slower, because the
graphics library initialises every adapter before choosing one.

## Contributing

Issues and pull requests are welcome. CI only builds and audits dependencies, so before opening a pull
request run the checks locally: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace -q`. After changing dependencies, run
`python scripts/third_party_notices.py` and commit `THIRD-PARTY-NOTICES.txt`. Keep the build pure Rust (no dependency that compiles C/C++:
`cargo tree -i cc --workspace -e all` must stay empty), give every `unsafe` block a `// SAFETY:` comment,
return errors instead of panicking on external input (files, projects, clipboard, network), and make tests
write only to temporary folders, generating their data at run time rather than adding binary files.

## Licence

MIT (`LICENSE.md`). Lato: SIL Open Font License 1.1 (`assets/fonts/OFL.txt`). The licences of the crates
and fonts compiled into `rview.exe` are in `THIRD-PARTY-NOTICES.txt` (shipped with the installer and every
release). Security issues: [SECURITY.md](SECURITY.md).
