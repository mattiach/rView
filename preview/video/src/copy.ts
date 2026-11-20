// Every line of on-screen text. Claims follow the root README; the timings are
// its own measurements, so the film promises nothing the app does not do.

export const copy = {
  opening: ["Thousands of photos.", "Dozens of formats.", "One viewer."],
  logo: {
    tagline: "The image viewer for Windows.",
    chips: ["Free", "Open source", "Lightweight"],
  },
  speed: {
    title: "Fast where it matters.",
    stats: [
      { value: 24, decimals: 0, unit: "ms", label: "from launch to window" },
      { value: 5.2, decimals: 1, unit: "ms", label: "to the next image" },
      { value: 44, decimals: 0, unit: "ms", label: "to sort 10 000 files" },
    ],
    footnote: "Release build · Ryzen 7 7800X3D · Radeon RX 7900 XT · NVMe SSD",
  },
  viewer: {
    first: "Arrow through the folder.",
    second: "Everything else is one right-click away.",
    keys: [
      { keys: ["←", "→"], label: "Previous / next" },
      { keys: ["R"], label: "Random file" },
      { keys: ["F11"], label: "Fullscreen" },
    ],
  },
  formats: {
    title: "Opens what you throw at it.",
    footnote: "HEIC, AVIF, JPEG XR and RAW through the Windows codecs.",
    rows: [
      ["JPEG", "PNG", "WebP", "GIF", "JPEG XL", "TIFF", "BMP", "APNG"],
      ["HEIC", "AVIF", "SVG", "QOI", "EXR", "HDR", "DDS", "TGA"],
      ["DNG", "CR3", "NEF", "ARW", "RAF", "ORF", "RW2", "PEF"],
      ["ICO", "ICNS", "PNM", "SVGZ", "JPEG XR", "HEIF", "XPM", "WBMP"],
      ["CR2", "SRW", "NRW", "PAM", "XBM", "CUR", "JXL", "PBM"],
    ],
    lit: ["JPEG XL", "HEIC", "AVIF", "SVG", "DNG", "CR3", "EXR"],
  },
  features: {
    markup: {
      tag: "Markup",
      title: "Mark it up.",
      body: "Arrows, shapes, text, a loupe and redactions. Every one stays editable.",
      keys: ["Ctrl", "Shift", "A"],
    },
    adjust: {
      tag: "Adjust",
      title: "Fine-tune.",
      body: "Exposure, contrast, temperature and more, with a before / after divider.",
      keys: ["\\"],
    },
    crop: {
      tag: "Crop",
      title: "Frame it.",
      body: "Free, fixed ratios, or a preset for every social network.",
      keys: [],
    },
    info: {
      tag: "File Info",
      title: "Know your file.",
      body: "Size, camera, metadata. One click copies any line.",
      keys: ["I"],
    },
  },
  free: {
    lines: ["Free.", "Open source.", "Lightweight."],
    footnote: ["MIT licence", "No ads", "No telemetry", "Installer or portable"],
  },
  closing: {
    tagline: "The image viewer for Windows.",
    url: "github.com/mattiach/rview",
  },
} as const;
