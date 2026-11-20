import { interpolate, useCurrentFrame } from "remotion";
import { clamp, colors, font, pop } from "../theme";

interface Props {
  readonly label: string;
  /** Frame it pops in. */
  readonly delay: number;
  /** Frames it is pressed down on, if any. */
  readonly pressAt?: readonly number[];
  readonly size?: number;
}

/** A keycap as the preview pages draw them, popping in and pressed on the beat. */
export const Key = ({ label, delay, pressAt = [], size = 26 }: Props) => {
  const frame = useCurrentFrame();
  const enter = interpolate(frame, [delay, delay + 12], [0, 1], { ...clamp, easing: pop });
  const pressed = pressAt.some((f) => frame >= f && frame < f + 5);

  return (
    <span
      style={{
        backgroundColor: pressed ? colors.blue : colors.panel,
        border: "1.5px solid rgb(255 255 255 / 16%)",
        borderBottomWidth: pressed ? 1.5 : 5,
        borderRadius: size * 0.32,
        color: colors.text,
        display: "inline-block",
        fontFamily: font,
        fontSize: size,
        fontWeight: 700,
        lineHeight: 1,
        minWidth: size * 2,
        opacity: Math.min(1, enter * 2),
        padding: `${size * 0.4}px ${size * 0.5}px`,
        scale: 0.5 + 0.5 * enter,
        textAlign: "center",
        translate: `0 ${pressed ? 3.5 : 0}px`,
      }}
    >
      {label}
    </span>
  );
};
