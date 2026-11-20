import { loadFont } from "@remotion/fonts";
import { Easing, staticFile } from "remotion";

// rView's own look: the preview pages' ground and blue accent (preview/src/preview.css),
// the logo's off-white stroke and green dot (assets/icon/rview.png).
export const colors = {
  ground: "#0e0f12",
  panel: "#1a1c21",
  line: "rgb(255 255 255 / 9%)",
  text: "#f2f3f5",
  muted: "#9a9fa8",
  blue: "#1a7dff",
  stroke: "#f4f5f0",
  lime: "#d8ff5e",
  mint: "#5ef0a8",
} as const;

export const green = `linear-gradient(135deg, ${colors.lime}, ${colors.mint})`;

export const font = 'Lato, "Segoe UI", sans-serif';

loadFont({ family: "Lato", url: staticFile("fonts/Lato-Regular.ttf"), weight: "400" });
loadFont({ family: "Lato", url: staticFile("fonts/Lato-Bold.ttf"), weight: "700" });

export const ease = Easing.bezier(0.16, 1, 0.3, 1);
export const easeIn = Easing.bezier(0.7, 0, 0.84, 0);
export const pop = Easing.bezier(0.34, 1.56, 0.64, 1);

export const clamp = {
  extrapolateLeft: "clamp",
  extrapolateRight: "clamp",
} as const;
