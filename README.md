<p align="center">
  <img src="assets/icon/rview.png" width="128" height="128" alt="rView logo">
</p>

<h1 align="center">rView</h1>

<p align="center">
  An image viewer for Windows, written in Rust, with a built-in editor for annotations and redactions.
</p>

rView opens an image, lets you move through its folder with the arrow keys and stays out of the way. Markup
(Ctrl+Shift+A) adds crop, adjustments, shapes, text and redactions without touching the original file: the
edits stay editable, and you save them as an `.rvm` project or export a new image.

## Install

- **Installer** (`rview-<version>-setup-x64.exe`): installs for the current user, no administrator rights
  needed (all users is an option). Adds rView to "Open with" for every supported type and to Settings > Apps >
  Default apps without changing your current defaults, registers `.rvm` projects, and comes with an
  uninstaller.
- **Portable**: copy `rview.exe` anywhere and run it.

Either way, settings and state are stored in `%APPDATA%\rView` (`settings.toml`, `state.toml`).

Built and tested on Windows 11 x64. The exe declares support for Windows 10 and the installer accepts
Windows 10 1809 or later, but those versions are not tested.

## Viewer

- GPU rendering (wgpu on DirectX 12). Zoom and pan only change a transform, so they cost no CPU work per
  frame.
- Neighbouring files are preloaded in the direction you are moving. JPEGs above 24 MP show a reduced preview
  first, then the full image.
- Folder navigation: six sort modes, loop at the ends, random file (R), slideshow.
- Animations: GIF, APNG, WebP and JPEG XL, with Save Frame As.
- Colour management: embedded ICC profiles and Display P3 are converted to sRGB.
- Multiple windows, fullscreen, drag & drop, custom shortcuts.
- File operations: Rename, Delete to the Recycle Bin with Undo, Open With, File Info.
- Clipboard: copy and paste images. Paste and Open URL also accept Base64 images and `data:` URLs, and Copy
  as Base64 copies the file on screen.
- Open URL downloads an image from the web. It is the only time rView uses the network.
- Convert (Tools menu): change format, JPEG quality, transparency, DPI and size.

The interface is in English. Translations are Fluent files in [`assets/i18n`](assets/i18n): adding a
`<language>.ftl` file is enough, no code change needed.

## Formats

| Decoded by rView                                                                                                                       | Through the Windows codecs                                                                   |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| JPEG, PNG/APNG, GIF, WebP, TIFF, BMP, ICO/CUR, ICNS, TGA, PNM (PBM/PGM/PPM/PAM), QOI, HDR, EXR, DDS, JPEG XL, SVG/SVGZ, WBMP, XBM, XPM | HEIC/HEIF, AVIF, JPEG XR, camera RAW (DNG, CR2, CR3, NEF, NRW, ARW, ORF, RW2, RAF, PEF, SRW) |

The formats in the right column need the matching codec installed in Windows (for example _HEIF Image
Extensions_ from the Microsoft Store). SVG is re-rasterised when you zoom in, so it stays sharp. HDR and EXR
are shown as SDR.

## Markup

- **Crop**: free, original, 1:1, 4:3, 3:2, 16:9, social media presets (Instagram, Facebook, X, YouTube,
  LinkedIn) or a custom ratio. Also rotate and flip.
- **Adjust**: exposure, brightness, contrast, saturation, temperature, hue, sharpness.
- **Draw**: rectangle, ellipse, line, arrow (straight or curved), pen, highlighter, text (embedded Lato or any
  installed font), loupe.
- **Watermark**: place an image, then move, resize and rotate it and set its opacity. The last two images
  used are remembered.
- **Redact**: Solid, Blur or Pixelate.
- **Compare** (`\`): a draggable divider between the original and the edited image.
- Every object can be selected and changed later. Undo and redo cover every step.

When you are done:

- **Save Project** writes an `.rvm` file you can open again and keep editing.
- **Export** writes PNG, JPEG, WebP, TIFF or BMP, with the same size and DPI options as Convert. You can keep
  the metadata, remove the location or remove all of it. Embedded thumbnails are never copied.
- **Overwrite Original** replaces the file and puts a backup of the original in the Recycle Bin.
- **Copy** puts the result on the clipboard.

> [!IMPORTANT]
> Only **Solid** redaction hides data. Blur and Pixelate are visual effects and may be reversible.
> An `.rvm` project contains the whole original image, including what is under its redactions: keep projects
> private and share an export instead.

## Performance

Release build on a Ryzen 7 7800X3D, Radeon RX 7900 XT, NVMe SSD, 2560×1440:

| Measure                                            | Time           |
| -------------------------------------------------- | -------------- |
| Start, no file → window visible                    | 24 ms          |
| Start with a 12 MP JPEG → image on screen          | ~375 ms        |
| Next image, preloaded / not preloaded (12 MP JPEG) | 5.2 ms / 65 ms |
| 50 MP JPEG: reduced preview / full resolution      | 94 ms / 260 ms |
| Folder of 10 000 files: scan and sort              | 44 ms          |
| Memory with a 12 MP image open (Task Manager)      | 151 MB         |

On PCs with more than one GPU, such as a graphics card plus the processor's integrated GPU, start-up is
slower: the graphics library initialises every adapter before choosing one. Method, more measurements and
known limitations: [docs/performance.md](docs/performance.md).

## Documentation

- [CONTRIBUTING.md](CONTRIBUTING.md): building, tests, benchmarks, releases, rules for changes.
- [ARCHITECTURE.md](ARCHITECTURE.md): how the viewer and Markup work inside.
- [docs/capabilities.md](docs/capabilities.md): library and Windows behaviours that the tests check.
- [SECURITY.md](SECURITY.md): how to report a vulnerability.

## Licence

rView is released under the MIT licence ([LICENSE.md](LICENSE.md)). The embedded Lato font is under the SIL
Open Font License 1.1 ([assets/fonts/OFL.txt](assets/fonts/OFL.txt)). The licences of every crate and font
compiled into `rview.exe` are in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt), which ships with the
installer.
