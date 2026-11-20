// Copies what the film shows from the repository into public/, so nothing is
// committed twice: the screenshots and the Lato font.
//
//   node scripts/copy-assets.mjs

import { cpSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VIDEO = join(dirname(fileURLToPath(import.meta.url)), "..");
const REPO = join(VIDEO, "..", "..");
const PUBLIC = join(VIDEO, "public");

const files = {
  "fonts/Lato-Regular.ttf": "assets/fonts/Lato-Regular.ttf",
  "fonts/Lato-Bold.ttf": "assets/fonts/Lato-Bold.ttf",
};
for (const n of [1, 2, 3, 4, 5]) files[`screenshots/${n}.png`] = `preview/screenshots/${n}.png`;

for (const [to, from] of Object.entries(files)) {
  mkdirSync(dirname(join(PUBLIC, to)), { recursive: true });
  cpSync(join(REPO, from), join(PUBLIC, to));
}
