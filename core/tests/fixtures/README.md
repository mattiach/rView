# Real fixtures

Files that cannot be produced by the project's own (pure-Rust) encoders. Each one is checked by
`core/tests/fixtures.rs` against a reference decoded by an **independent decoder** (libjxl reference
images, libavif, libjpeg-turbo, libheif), so the tests compare two decoders on the same bitstream.
Missing files are reported as `SKIPPED: fixtures/<name> not present`.

**The files are not in the repository** (`.gitignore` keeps only this README and the two
scripts). Without them the tests print `SKIPPED` (CI does not run the tests). To get them
back, from this folder:

    python fetch_fixtures.py                      # downloads below, SHA-256 checked (standard library only)
    pip install --target <dir> pillow pillow-heif imagecodecs
    python gen_fixtures.py <dir>                  # generated files below

Total ≈ 4.7 MB (largest: photo.dng 2.1 MB, oriented.jxl.ref.png 0.7 MB). The external encoders below were used
only to create test data; they are not dependencies of rView.

## Downloaded

| File | Source | Licence |
| --- | --- | --- |
| `still.jxl`, `still.jxl.ref.png` (`fetch_fixtures.py`) | libjxl/conformance `testcases/grayscale/{input.jxl, ref.png}`, commit `b1d0f990b03e57bf6d137c365cd5dc8b470b9191` | BSD-3-Clause (the JPEG XL Project Authors), text in `LICENSE-libjxl-conformance.txt` |
| `oriented.jxl`, `oriented.jxl.ref.png` | same repository, `testcases/bench_oriented_brg` (orientation in the codestream) | BSD-3-Clause |
| `anim.jxl`, `anim.jxl.ref.gif`, `anim.jxl.test.json` | same repository, `testcases/animation_newtons_cradle` (36 frames, durations in `test.json`) | BSD-3-Clause |
| `progressive.jxl` | same repository, `testcases/progressive/input.jxl` (2 passes + LF frame) | BSD-3-Clause |
| `photo.dng` | raw.pixls.us, "Sigma - fp - 8bit (16:9).DNG", sha256 `80ae0b8fdce3f58286fc194513c6ec0080ab07caca753408b00ca72c90e5ad0c` (verified) | CC0 1.0 |

## Generated with independent encoders (`gen_fixtures.py`)

| File | Encoder | Reference |
| --- | --- | --- |
| `photo.avif` (BT.601 nclx, libavif default) | Pillow 13 / libavif + aom | `photo.avif.ref.png`, Pillow / libavif + dav1d |
| `anim.webp` (3 lossless frames, 40/60/80 ms) | Pillow 13 / libwebp | values in the test (libwebp decodes them exactly) |
| `cmyk.jpg` (Adobe APP14, inverted CMYK) | Pillow 13 / libjpeg-turbo | `cmyk.jpg.ref.png`, Pillow / libjpeg-turbo |
| `quadrants.svgz` | Python gzip of an SVG | values in the test |
| `orient1.heic` … `orient8.heic` (pixels stored unrotated + `irot`/`imir`, as iPhones do) | pillow-heif 1.8.1 / libheif 1.23.6 + x265 | `orientN.heic.ref.png`, libheif (applies the transformations) |
| `heic_bt601.heic`, `heic_bt709.heic` (same image, two colour matrices) | same | `*.heic.ref.png`, libheif |
| `large.jxl` (6000×4200, single pass) | imagecodecs / libjxl | full decode by jxl-oxide (preview path) |

Lossy encoders are not bit-reproducible across versions: regenerated files can differ from earlier ones, but
the tests compare each one with its own reference, so they stay valid.

## Known issues covered by `#[ignore]` tests

- Windows HEIF and AVIF decoders convert BT.601 (nclx matrix 6) with BT.709.
- image-webp 0.2.4 composites some animation frames off by 1 (upstream).
