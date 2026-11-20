import type { ReactNode } from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { clamp, ease } from "../theme";

interface Props {
  readonly children: ReactNode;
  /** Scene length, for the slow push-in that keeps every frame moving. */
  readonly duration: number;
  readonly push?: number;
}

/** Every scene arrives like a lens pulling focus: soft and a little close, then sharp. */
export const Lens = ({ children, duration, push = 0.035 }: Props) => {
  const frame = useCurrentFrame();
  const focus = interpolate(frame, [0, 12], [0, 1], { ...clamp, easing: ease });
  const drift = interpolate(frame, [0, duration], [0, push], clamp);

  return (
    <AbsoluteFill
      style={{
        filter: focus < 1 ? `blur(${(1 - focus) * 16}px)` : undefined,
        scale: 1.06 - 0.06 * focus + drift,
      }}
    >
      {children}
    </AbsoluteFill>
  );
};
