import type { CSSProperties } from "react";
import { interpolate, useCurrentFrame } from "remotion";
import { clamp, ease, easeIn } from "../theme";

interface Props {
  readonly text: string;
  /** Frame the first word starts arriving. */
  readonly delay?: number;
  /** Frames between consecutive words. */
  readonly stagger?: number;
  /** Frames each word takes to come into focus. */
  readonly duration?: number;
  /** Frame the whole line starts blurring away, if it leaves. */
  readonly out?: number;
  readonly style?: CSSProperties;
}

/** Words come into focus one after another, as if the lens found them. */
export const FocusText = ({ text, delay = 0, stagger = 3, duration = 14, out, style }: Props) => {
  const frame = useCurrentFrame();
  const words = text.split(" ");
  const leave = out === undefined ? 0 : interpolate(frame, [out, out + 10], [0, 1], { ...clamp, easing: easeIn });

  return (
    <span style={{ display: "inline-block", ...style }}>
      {words.map((word, i) => {
        const start = delay + i * stagger;
        const p = interpolate(frame, [start, start + duration], [0, 1], { ...clamp, easing: ease }) * (1 - leave);
        const blur = (1 - p) * 18;
        return (
          <span
            // Words can repeat and never reorder; their position is their identity.
            key={`${i}-${word}`}
            style={{
              display: "inline-block",
              filter: blur > 0.05 ? `blur(${blur}px)` : undefined,
              opacity: p,
              scale: 1.18 - 0.18 * p,
              whiteSpace: "pre",
            }}
          >
            {word}
            {i < words.length - 1 && " "}
          </span>
        );
      })}
    </span>
  );
};
