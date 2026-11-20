import { useId } from "react";
import { colors } from "../theme";

// The icon redrawn as vectors on its own 1600-unit grid (assets/icon/rview.png),
// so the stroke can draw itself and the dot can arrive on its own.
const STROKE = "M 510 1085 L 510 735 A 250 250 0 0 1 760 485 L 1085 485";
const STROKE_LENGTH = 350 + (Math.PI * 250) / 2 + 325;
const DOT = { cx: 960, cy: 948, r: 193 } as const;

/** Where the dot sits inside an icon of `size` pixels, from its top-left corner. */
export const dotIn = (size: number) => ({
  cx: (DOT.cx / 1600) * size,
  cy: (DOT.cy / 1600) * size,
  r: (DOT.r / 1600) * size,
});

interface Props {
  readonly size: number;
  /** 0 to 1: the rounded square growing out of the dot. */
  readonly tile?: number;
  /** 0 to 1: the stroke drawing from its foot. */
  readonly stroke?: number;
  /** 0 to 1: the dot. */
  readonly dot?: number;
}

export const LogoMark = ({ size, tile = 1, stroke = 1, dot = 1 }: Props) => {
  const id = useId();

  return (
    <svg height={size} style={{ display: "block", overflow: "visible" }} viewBox="0 0 1600 1600" width={size}>
      <defs>
        <linearGradient id={`${id}-tile`} x1="0" x2="0" y1="0" y2="1">
          <stop offset="0" stopColor="#1e1e1e" />
          <stop offset="1" stopColor="#0d0d0d" />
        </linearGradient>
        <linearGradient id={`${id}-dot`} x1="0.15" x2="0.85" y1="0.1" y2="0.9">
          <stop offset="0" stopColor={colors.lime} />
          <stop offset="1" stopColor={colors.mint} />
        </linearGradient>
      </defs>
      <rect
        fill={`url(#${id}-tile)`}
        height="1440"
        rx="340"
        stroke="#333"
        strokeWidth="14"
        style={{ opacity: Math.min(1, tile * 2), scale: 0.3 + 0.7 * tile, transformOrigin: `${DOT.cx}px ${DOT.cy}px` }}
        width="1440"
        x="80"
        y="80"
      />
      {stroke > 0 && (
        <path
          d={STROKE}
          fill="none"
          stroke={colors.stroke}
          strokeDasharray={STROKE_LENGTH}
          strokeDashoffset={STROKE_LENGTH * (1 - stroke)}
          strokeLinecap="round"
          strokeWidth="168"
        />
      )}
      <circle {...DOT} fill={`url(#${id}-dot)`} style={{ scale: dot, transformOrigin: `${DOT.cx}px ${DOT.cy}px` }} />
    </svg>
  );
};
