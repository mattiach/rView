# Contributing to rView

Issues and pull requests are welcome. How the code is organised: [ARCHITECTURE.md](ARCHITECTURE.md).

## Requirements

- Windows 11 x64 and a DirectX 12 adapter (WARP works for the tests, slower).
- Rust 1.97.1, pinned in `rust-toolchain.toml`: `rustup` installs it on the first build.
- Optional: [Inno Setup 6](https://jrsoftware.org/isinfo.php) for the installer, Python 3 for the test
  fixtures and `THIRD-PARTY-NOTICES.txt`.

## Build and run

| Command | What it does |
| --- | --- |
| `cargo run -p rview -- <file or folder>` | Debug build, optionally opening a file or folder |
| `cargo build --release -p rview` | `target\release\rview.exe` (portable, ~23 MB) |
| `iscc dist\win\rview.iss` | Installer in `target\installer`, after the release build |

The logo is `assets/icon/rview.png`. The build turns it into a 16–256 px icon, embeds it in `rview.exe` and
copies it next to the exe as `rview.ico` for the installer. To change the logo, replace the PNG with a square
image on a transparent background.

## Checks before a pull request

Nothing runs these automatically, so run them yourself:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -q
```

After changing dependencies, run `python scripts/third_party_notices.py` and commit
`THIRD-PARTY-NOTICES.txt`. `python scripts/third_party_notices.py --check` tells you whether it is out of date.

## Rules for changes

- Pure Rust only: no dependency that compiles C or C++. `cargo tree -i cc --workspace -e all` must stay empty.
- Every `unsafe` block has a `// SAFETY:` comment.
- External input (files, projects, clipboard, network) returns errors, never panics.
- Tests write only to temporary folders and generate their data at run time. Don't add binary files to the
  repository.

## Tests

The end-to-end tests run the real app with its windows cloaked: nothing appears on screen or in the
taskbar, but the windows exist, so leave mouse and keyboard alone until the run ends. Each E2E test prints
one line: `e2e_markup: OK`, or `FAILED` with the reason.

```
cargo test --workspace -q                                   # everything
cargo test -p rv-core -q                                    # core only, no windows
cargo test -p rview -q                                      # app: unit, Windows integration, E2E
cargo test -p rview --test e2e_markup                       # one E2E test
cargo test -p rv-core --test edit -- curved_arrow           # tests whose name contains "curved_arrow"
cargo test --release -p rview --test e2e_nav_perf           # navigation timings, thresholds checked in release
cargo test -p rv-core --test capabilities -- --nocapture --test-threads 1   # platform capability report
```

- Real-file fixtures are not in the repository. `python core/tests/fixtures/fetch_fixtures.py` downloads
  them (SHA-256 checked); without them those tests print `SKIPPED`. See
  [core/tests/fixtures/README.md](core/tests/fixtures/README.md).
- `windows_integration`, `e2e_lifecycle` and `e2e_base64` replace the clipboard content.
  `windows_integration` also sends a file to the Recycle Bin and restores it.
- `uia_native` drives the native UI through UI Automation (menu bar, context menu, Delete confirmation, Open
  dialog, Settings). Its windows are **not** cloaked: they appear on screen for about half a minute. Without
  an interactive desktop it prints `SKIPPED`.
- Tests never touch `%APPDATA%\rView`.
- `RVIEW_GOLDEN_DIR=<folder>` writes the Markup golden images as PNG so you can look at them.

## Benchmarks and fuzzing

| Command | What it measures |
| --- | --- |
| `cargo bench -p rv-core --bench decode` | Folder scan, decode, previews |
| `cargo bench -p rv-core --bench markup` | Redaction patches, Markup overlay, export |
| `cargo bench -p rview --bench upload_mip` | GPU upload and mipmaps |
| `cargo run --profile fuzz -p rv-core --example fuzz_project -- 600` | Fuzzes the `.rvm` reader for 600 s |

Results and method: [docs/performance.md](docs/performance.md).

## New version

`powershell -ExecutionPolicy Bypass -File scripts\release.ps1 1.2.0` sets the version in `Cargo.toml` and
`dist\win\rview.iss`, builds `rview.exe`, regenerates `THIRD-PARTY-NOTICES.txt` and, if Inno Setup 6 is
installed, builds `target\installer\rview-1.2.0-setup-x64.exe`. Everything stays on your machine: the script
publishes nothing.

Code signing needs your own certificate: see the comments at the top of `dist\win\rview.iss`.
