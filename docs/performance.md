# Performance

Measured on Windows 11 Pro 25H2, Ryzen 7 7800X3D, Radeon RX 7900 XT (plus the processor's integrated GPU),
NVMe SSD, 2560×1440, with the release profile of the workspace (`lto = "thin"`,
`codegen-units = 1`, `panic = "abort"`, `opt-level = 3`). Numbers on other hardware will differ.

## Reproducing

    cargo bench -p rv-core --bench decode
    cargo bench -p rv-core --bench markup
    cargo bench -p rview --bench upload_mip
    cargo test --release -p rview --test e2e_nav_perf     # asserts the navigation thresholds in release
    cargo test --release -p rv-core --test redact -- patch_timing --nocapture   # asserts < 30 ms

The benchmarks are plain timing programs (criterion is not used because it pulls C code through `alloca`);
each prints `BENCH <name>: median …, best …`. `e2e_nav_perf` and `patch_timing` assert their thresholds only
in release builds; debug builds print the numbers without asserting.

## Results

| Measure                                                     | Result                                                                                     | Source                                                          |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------ | --------------------------------------------------------------- |
| Start → window visible (no file)                            | 23–26 ms                                                                                   | `rview --measure-startup` (4 runs)                              |
| Start with a 12 MP JPEG → image visible                     | 370–386 ms (decode and view preparation: 55 ms; the rest is GPU initialisation, see below) | `rview --measure-startup <12 MP JPEG>` (4 runs); `decode` bench |
| 50 MP JPEG → first image (reduced preview)                  | 94 ms                                                                                      | `e2e_nav_perf` (release)                                        |
| 50 MP JPEG → full resolution                                | 260 ms                                                                                     | `e2e_nav_perf` (release)                                        |
| Next/Previous, preloaded                                    | 5.2 ms                                                                                     | `e2e_nav_perf` (release)                                        |
| Next/Previous, not preloaded, 12 MP JPEG                    | 65 ms                                                                                      | `e2e_nav_perf` (release)                                        |
| Holding →                                                   | last image on screen 70 ms after the last key; no obsolete result is shown                 | `e2e_hold_perf`, `e2e_navigate`                                 |
| Zoom/pan                                                    | transform only, no CPU work per frame                                                      | `render.rs`                                                     |
| Folder of 10 000 files: scan + sort                         | 44 ms, on a worker thread                                                                  | `decode` bench                                                  |
| Redaction patch 1000×1000 (12 MP image)                     | Blur 28–29 ms, Pixelate 15 ms, Solid 13 ms                                                 | `markup` bench, `patch_timing`                                  |
| Markup overlay, 200 shapes, 2560×1440                       | 4.4 ms (CPU raster of the whole layer)                                                     | `markup` bench                                                  |
| Export PNG 24 MP with 50 shapes                             | 211 ms (render + encode)                                                                   | `markup` bench                                                  |
| GPU upload of a 24 MP image                                 | 7.6 ms                                                                                     | `upload_mip` bench                                              |
| 12 MP decode + view preparation: JPEG / PNG / WebP lossless | 55 / 59 / 51 ms                                                                            | `decode` bench                                                  |
| JPEG XL 25 MP single pass, decode + view preparation        | 573 ms                                                                                     | `decode` bench                                                  |
| 50 MP JPEG: reduced preview (≥ 2560 px) / full decode       | 127 / 235 ms (cold codec)                                                                  | `decode` bench                                                  |
| Memory with a 12 MP image open: private / total working set | 151–152 / 192–193 MB (without a file: 105 / 141 MB)                                        | release exe, 6 s after start, median of 3 runs                  |

Memory method: release `rview.exe` started with a 12 MP JPEG and default settings (preloading Adjacent, so
the view cache holds the image), read 6 s after start. "Private working set" is Task Manager's Memory column
(`Win32_PerfRawData_PerfProc_Process.WorkingSetPrivate`); the total working set also counts pages shared
with other processes (driver and system DLL code, ~40 MB here). `--measure-startup` times from the creation
of the app object, a few milliseconds after the process starts.

## Limitations

- **Start-up on PCs with several GPUs.** The graphics library (wgpu) creates a Direct3D 12 device on every
  adapter to read its capabilities before choosing one. On the machine above, which has a discrete and an
  integrated GPU with separate drivers, this probing takes ~170 ms, so most of the start-up time with an
  image is spent before rView can draw. PCs with a single GPU do less of this work.
- **GPU memory.** wgpu's default allocator keeps a 64 MB block of CPU-visible GPU memory resident; it is
  part of the private working set above.
- **Blur on weaker CPUs.** A 1000×1000 Blur patch takes close to 30 ms on the machine above and more on
  PCs with fewer cores. It runs off the UI thread and is cached, so a slower CPU shows that area's preview
  later; the UI does not freeze.
- **JPEG XL previews.** Only progressive files get a fast preview; single-pass files are shown when the full
  decode finishes.
