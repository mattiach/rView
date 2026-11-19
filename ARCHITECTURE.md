# rView architecture

Two crates, pure Rust (no C/C++ in the build graph: `cargo tree -i cc --workspace -e all` is empty).

| Crate | Path | Role |
| --- | --- | --- |
| `rv-core` | `core/` | Everything that is not UI or GPU: decoding, colour, folder navigation, loader and cache, settings and state, the Markup model, `.rvm` projects, export, metadata, atomic writes. |
| `rview` | `app/` | The Windows app: winit event loop, wgpu (DX12) rendering, egui UI, native menus (muda), Win32 helpers, the E2E driver. |

## Viewer

- **Start**: the GPU is initialised on a thread started at process launch; the window is shown at once with a
  background brush that matches the first frame, and decoding starts before the GPU wait. The egui renderer
  (~45 ms of shader compilation) is built on a worker; the first frames draw only the image.
- **Loader** (`core/src/loader.rs`): priority queue, W = clamp(cpu − 1, 2, 6) workers, one always free for the
  current image, cancellation between phases. Neighbours are preloaded in the direction of travel into a shared
  LRU view cache with a byte budget (never above 25 % of RAM). Obsolete results are dropped, so holding → always
  ends on the last requested file.
- **Large JPEGs** (> 24 MP): a reduced preview through WIC DCT scaling, sized to the window's viewport, then the
  full image. JPEG XL gets a preview only for progressive files.
- **Rendering** (`app/src/render.rs`, `image.wgsl`): one shared DX12 device; image textures with linear-light
  mipmaps; premultiplied linear filtering (no halos from transparent pixels). Same-size images reuse the textures
  on screen. Images wider than the texture limit are tiled.
- **Colour** (`core/src/color.rs`): source ICC → sRGB view space with qcms; Display P3 and gray profiles
  converted; CMYK is converted by the decoder. HDR/EXR are shown as SDR.
- **Decoders** (`core/src/decode`): `image`, `tiff`, `jxl-oxide`, `resvg` (SVG is re-rasterised when zoomed in,
  up to 8192 px), `icns`, own WBMP/XBM/XPM, and WIC for HEIC/HEIF, AVIF, JPEG XR, camera RAW and exotic files.
  Every decoder checks the size from the header first (limit 2 gigapixels).
- **Windows integration** (`app/src/platform.rs`): Recycle Bin with Undo Delete, clipboard (PNG + CF_DIBV5 +
  CF_HDROP in one transaction), Open With, TaskDialog, locale formats, DWM title bar theme. Paths are expanded
  with `GetLongPathNameW` (the Recycle Bin records long paths).

## Markup

- **Model** (`core/src/edit`): `DocState` is an immutable snapshot (shared `Arc`s); `Edit::apply` is pure and
  returns a new state; `History` moves between snapshots (gestures are one entry). Every shape or redaction has a
  stable `ObjectId`. All geometry is in image space; the source pixels are never changed.
- **Rendering**: `render` is the single source of truth for Export, Overwrite, Copy and the golden tests. The
  on-screen preview rasterises the same tiny-skia paths in parallel bands; pan and zoom only transform the layer
  on the GPU, the raster is redone once the view is still (100 ms).
- **Redactions** (`redact.rs`): each becomes a patch (the pixels of its box after the effect), cached by the
  redaction, its read area and the earlier patches it depends on, so preview and export use the same pixels and
  undo/redo hit the cache. Blur is three integer box passes on premultiplied values (exact and deterministic);
  Pixelate is a grid anchored at the area's corner; Solid replaces every pixel the area touches, even partly
  (binary conservative mask, alpha 255, applied after overlapping Blur/Pixelate). Only Solid hides data; Blur
  and Pixelate are visual effects, not a protection.
- **Text** (`text.rs`): cosmic-text shaping and wrapping, skrifa glyph outlines; Lato embedded, installed fonts
  loaded in the background on first use.
- **`.rvm` projects** (`core/src/project.rs`): a ZIP with `manifest.json` (`"format": "rview-markup"`),
  `document.json` and the original file's exact bytes. Read entirely in memory as untrusted input: sizes and
  ratios checked while reading, local headers compared with the central directory, every value validated; any
  violation is an error, never a partial load or a panic. Fuzzed by `core/examples/fuzz_project.rs`.
- **Writes** (`core/src/atomic.rs`): encode in memory, write a hidden temp file in the destination folder,
  verify it, then rename. Overwrite Original uses `ReplaceFileW`; the backup goes to the Recycle Bin and can be
  restored. Exports never copy the source's metadata segments: EXIF is rebuilt per preset (keep, strip location,
  strip all), previews are always stripped.

## Tests

- Unit and integration tests in both crates; golden images are stored as hashes (no binary assets in the
  repository). Third-party fixtures are downloaded on demand (`core/tests/fixtures/README.md`); without them the
  fixture tests print `SKIPPED`.
- **E2E** (`app/tests/e2e_*.rs`): the real app runs a script of steps (`app/src/driver.rs`) inside its own event
  loop and checks observations and offscreen snapshots. Windows are cloaked (invisible, not in the taskbar) while
  a driver runs. Each E2E test is its own process (one event loop per process).
- **Native E2E** (`app/tests/uia_native.rs`): a UI Automation client in a child process reads and operates the
  menu bar, the context menu, the Delete TaskDialog, the Open dialog and the Settings window (AccessKit) while
  the script waits (`Step::Until`). Windows stay visible (`Config::visible`: UI Automation ignores cloaked
  ones). Patterns only, no injected input; `SKIPPED` without an interactive desktop.
- **Benchmarks** (`core/benches`, `app/benches`): plain timing programs (criterion pulls C code), printed as
  `BENCH <name>: median …`; results in `docs/performance.md`.

## Known limits

- **Start-up and memory** (`docs/performance.md`): wgpu creates a D3D12 device on every adapter before
  choosing one (~170 ms on a PC with an integrated + a discrete GPU; wgpu offers no way to probe only one),
  so most of the 12 MP start-up (~375 ms there) is GPU initialisation. Its default allocator keeps a 64 MB
  block of CPU-visible memory; with a 12 MP image the private working set is ~151 MB.
- **Advanced Color / HDR / wide gamut**: the view space is sRGB and the swapchain SDR; wide-gamut colours are
  clipped to sRGB (no FP16 scRGB swapchain; the unverified assumptions AC-1…AC-4 are in
  `docs/capabilities.md`).
- **Windows HEIF/AVIF decoders** apply BT.709 to files tagged BT.601 (colours slightly off, as in every app
  using WIC); WIC gives no planar access to correct it. Tests `heic_bt601_*` / `avif_colours_*` are `#[ignore]`.
- **image-webp 0.2.4** composites some animated WebP frames off by 1 (upstream).
- **WARP** (no GPU, e.g. virtual machines): rendering is slow, so the animation cadence check prints `SKIPPED`.
- **Open URL**: at most 1 GiB, held in memory; proxy authentication is not supported.
- **Recycle Bin and long paths**: a file whose full path has 260 characters or more is not moved to the bin
  (Windows 11 25H2 would delete it for good while reporting success); Delete explains it, Delete
  Permanently still works, and an Overwrite Original backup stays next to the file.
- **Delete Permanently** overwrites the file 3 times (zeros, ones, random, each flushed) before deleting it.
  SSD wear levelling, compressed or copy-on-write volumes and shadow copies can still keep old blocks; a file
  with other hard links is only unlinked (its data belongs to them).
- **Open With** shows no icon for Store apps; **TaskDialog** is always light (Windows draws it without dark mode).
- **Very large images**: 180 MP opens tiled with a peak working set of ~1.6 GB.
- **XPM**: named colours beyond a short list show as black; **WBMP**: type 0 only.
