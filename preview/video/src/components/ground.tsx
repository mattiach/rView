import type { ReactNode } from "react";
import { AbsoluteFill, useCurrentFrame } from "remotion";
import { colors } from "../theme";

interface Props {
  readonly children?: ReactNode;
  /** Where the glow sits, in percent of the frame. */
  readonly glow?: readonly [x: number, y: number];
  readonly glowColor?: string;
}

/** The preview pages' ground: near black, a dot grid drifting slowly, one soft glow. */
export const Ground = ({ children, glow = [50, 50], glowColor = "rgb(26 125 255 / 22%)" }: Props) => {
  const frame = useCurrentFrame();

  return (
    <AbsoluteFill style={{ backgroundColor: colors.ground, overflow: "hidden" }}>
      <AbsoluteFill
        style={{
          background: `radial-gradient(45% 60% at ${glow[0]}% ${glow[1]}%, ${glowColor}, transparent 70%)`,
        }}
      />
      <AbsoluteFill
        style={{
          backgroundImage: "radial-gradient(circle, rgb(255 255 255 / 7%) 1.2px, transparent 1.8px)",
          backgroundPosition: `${-frame * 0.4}px ${-frame * 0.25}px`,
          backgroundSize: "28px 28px",
          maskImage: "radial-gradient(ellipse at 50% 50%, black 30%, transparent 85%)",
        }}
      />
      {children}
      <AbsoluteFill
        style={{
          background: "radial-gradient(ellipse at 50% 50%, transparent 55%, rgb(0 0 0 / 55%) 100%)",
          pointerEvents: "none",
        }}
      />
    </AbsoluteFill>
  );
};
