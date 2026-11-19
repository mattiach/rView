# Platform capabilities

rView depends on some behaviours of its libraries and of Windows that are checked by tests rather than
assumed: `core/tests/capabilities.rs` (synthetic files) and `core/tests/fixtures.rs` (real third-party
files, compared with an independent decoder). Results below are from Windows 11 Pro 25H2 (build
26200.9711), Radeon RX 7900 XT, one SDR display. Reproduce with

    cargo test -p rv-core --test capabilities -- --nocapture --test-threads 1
    python core/tests/fixtures/fetch_fixtures.py     # third-party fixtures (see its README)
    cargo test -p rv-core --test fixtures -- --nocapture

Each check prints a `CAPABILITY` or `SKIPPED` line.

## Libraries and codecs

| Capability | Result | Fallback / note |
| --- | --- | --- |
| ICC read by `image` from PNG / JPEG / WebP | OK | — |
| ICC read by `image` from TIFF (tag 34675) | **not supported** by `image` 0.25.10 | the tag is read with the `tiff` crate; the test asserts that rView gets the profile |
| `DynamicImage::apply_orientation` (PNG eXIf, WebP EXIF) | OK | — |
| GIF loop count | OK | — |
| `kamadak-exif` `experimental::Writer` | OK | used by the three metadata presets |
| `img-parts` ICC + EXIF insertion (JPEG, PNG, WebP) | OK | — |
| `jxl-oxide` EXIF box | OK on a real codestream in a container with an `Exif` box: read as metadata, its Orientation is **not** applied (the codestream's is) | no JXL encoder for a synthetic file: `SKIPPED` in the capability test |
| WIC DCT-scaled JPEG decode | OK (2048×1024 → 256×128 in 2.5 ms) | — |
| WIC HEIF decode | OK | — |
| WIC JPEG XR decode | OK | — |
| WIC AVIF decode | OK on a libavif file; colours: see below | Windows has no AVIF encoder: `SKIPPED` in the capability test |
| CUR through `image`'s ICO decoder | OK | — |
| JPEG CMYK (Adobe APP14, inverted) | OK vs libjpeg-turbo: max difference 1, mean 0.005 | no CMYK encoder for a synthetic file: `SKIPPED` in the capability test |
| HEIF `irot`/`imir` (all 8 orientations) | OK: all 8 match libheif | — |
| JPEG XL orientation in the codestream | OK, applied once (vs libjxl reference: max difference 1) | — |
| `serde` feature `rc` (`Arc<T>`, `Arc<[T]>`) | OK | — |
| `zip` reader checks CRC-32 | OK | — |
| `ReplaceFileW` keeps attributes and creation time | OK | — |
| Backup to the Recycle Bin and restore | OK | paths of 260+ characters are refused: Windows 11 25H2 deletes such files permanently instead of recycling them (also through 8.3 names) |

Known decoder issues (tests marked `#[ignore]` with the reason, listed in the test output):

- The Windows HEIF and AVIF decoders convert BT.601 files (nclx matrix 6) with BT.709 (an AVIF test file
  vs libavif: max difference 23, mean 3.0); BT.709 files match libheif exactly. WIC offers no planar access
  to correct it.
- `image-webp` 0.2.4 composites some animated WebP frames off by 1 (upstream issue).

## Advanced Color

The view space is sRGB and the swapchain is SDR. Using the monitor's profile or the Advanced Color state
would rely on the assumptions below, which are not verified, so rView reads neither: Auto colour conversion
means sRGB (unit test in `core/src/color.rs`), and wide-gamut colours are clipped to sRGB.

| ID | Assumption | Status |
| --- | --- | --- |
| AC-1 | DisplayConfig / DXGI tell SDR, Advanced Color (ACM) and HDR apart | **not verified**: the API answers (Advanced Color supported, off, SDR), but only that one configuration was tested |
| AC-2 | DWM treats a `Bgra8Unorm` swapchain as sRGB with Advanced Color on | **not verified** (needs ACM/HDR on and a colorimeter or a capture) |
| AC-3 | `GetICMProfileW` returns the profile in use and changes are signalled | **not verified** |
| AC-4 | wgpu-hal DX12 sets scRGB for an `Rgba16Float` swapchain | **not tested**: rView has no FP16 swapchain |

Probe output (`advanced_color_probe`) on the display above:

    INFO rc=0 supported=1 enabled=0 wideEnforced=1 forceDisabled=0 bpc=8;
    INFO_2 rc=0 value=0x45 activeColorMode=0 (0 SDR, 1 WCG, 2 HDR)

Without a display the probe prints `SKIPPED`.

## Checks that print SKIPPED by design

- `folder.rs` / `windows_integration`: 8.3-name cases when the volume has no 8.3 names.
- `windows_integration show_in_explorer_call`: opens an Explorer window, runs only with `RVIEW_TEST_SHELL_UI=1`
  or on GitHub Actions runners.
- `e2e_anim`: the 50 fps cadence check on the WARP software adapter (no GPU).
- `uia_native`: without an interactive desktop session.
- Fixture tests when the files are not present (see `core/tests/fixtures/README.md`).
