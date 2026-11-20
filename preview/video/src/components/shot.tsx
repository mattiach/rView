import type { CSSProperties, ReactNode } from "react";
import { Img, staticFile } from "remotion";
import { colors } from "../theme";
import type { Rect } from "./brackets";

/** Natural size of every capture in preview/screenshots. */
export const sizes = {
  1: { w: 969, h: 723 },
  2: { w: 969, h: 723 },
  3: { w: 1083, h: 681 },
  4: { w: 1707, h: 912 },
  5: { w: 1707, h: 912 },
} as const;

export type ShotId = keyof typeof sizes;

interface Props {
  readonly n: ShotId;
  /** Part of the capture to show, in its pixels; the whole capture if left out. */
  readonly crop?: Rect;
  /** Width on screen; the height follows the crop. */
  readonly width: number;
  /** Drawn on top in the capture's pixel space, so overlays line up with the UI. */
  readonly children?: ReactNode;
  readonly style?: CSSProperties;
}

/** A capture in the preview pages' frame: thin edge, faint outer ring, deep shadow. */
export const Shot = ({ n, crop, width, children, style }: Props) => {
  const c = crop ?? { x: 0, y: 0, w: sizes[n].w, h: sizes[n].h };
  const s = width / c.w;

  return (
    <div
      style={{
        backgroundColor: colors.panel,
        border: `1px solid ${colors.line}`,
        borderRadius: 14,
        boxShadow: "0 0 0 10px rgb(255 255 255 / 3%), 0 50px 100px -30px rgb(0 0 0 / 80%)",
        height: c.h * s,
        overflow: "hidden",
        position: "relative",
        width,
        ...style,
      }}
    >
      <div
        style={{
          height: sizes[n].h,
          left: -c.x * s,
          position: "absolute",
          scale: s,
          top: -c.y * s,
          transformOrigin: "0 0",
          width: sizes[n].w,
        }}
      >
        <Img src={staticFile(`screenshots/${n}.png`)} style={{ display: "block" }} />
        {children}
      </div>
    </div>
  );
};
