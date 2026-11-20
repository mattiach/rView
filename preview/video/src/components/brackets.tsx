import { interpolate } from "remotion";
import { clamp, colors, ease } from "../theme";

export interface Rect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** A rect on its way from `a` to `b` between two frames, for brackets that hunt for a target. */
export const between = (a: Rect, b: Rect, frame: number, from: number, to: number): Rect => {
  const p = interpolate(frame, [from, to], [0, 1], { ...clamp, easing: ease });
  return { x: a.x + (b.x - a.x) * p, y: a.y + (b.y - a.y) * p, w: a.w + (b.w - a.w) * p, h: a.h + (b.h - a.h) * p };
};

export const grow = (r: Rect, by: number): Rect => ({ x: r.x - by, y: r.y - by, w: r.w + 2 * by, h: r.h + 2 * by });

interface Props {
  readonly rect: Rect;
  readonly color?: string;
  readonly width?: number;
  readonly opacity?: number;
}

/**
 * Four viewfinder corners around a rect. Each one is the stroke of the logo's "r":
 * rounded caps and a rounded bend.
 */
export const Brackets = ({ rect, color = colors.stroke, width = 6, opacity = 1 }: Props) => {
  const { x, y, w, h } = rect;
  const arm = Math.max(18, Math.min(90, Math.min(w, h) * 0.2));
  const bend = arm * 0.45;
  const corner = (cx: number, cy: number, sx: number, sy: number) =>
    `M ${cx} ${cy + sy * arm} L ${cx} ${cy + sy * bend} Q ${cx} ${cy} ${cx + sx * bend} ${cy} L ${cx + sx * arm} ${cy}`;
  const d = [corner(x, y, 1, 1), corner(x + w, y, -1, 1), corner(x, y + h, 1, -1), corner(x + w, y + h, -1, -1)].join(" ");

  return (
    <svg
      aria-hidden="true"
      height="100%"
      style={{ inset: 0, opacity, overflow: "visible", pointerEvents: "none", position: "absolute" }}
      width="100%"
    >
      <path d={d} fill="none" stroke={color} strokeLinecap="round" strokeLinejoin="round" strokeWidth={width} />
    </svg>
  );
};
