import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { Brackets, between, grow } from "../components/brackets";
import { FocusText } from "../components/focus-text";
import { Ground } from "../components/ground";
import { dotIn, LogoMark } from "../components/logo";
import { copy } from "../copy";
import { clamp, colors, ease, font, green, pop } from "../theme";
import { onBeat, scenes } from "../timeline";

const ICON = { left: 820, top: 150, size: 280 } as const;
const inIcon = dotIn(ICON.size);

/** Where the opening has to leave its dot for the icon to grow around it. */
export const LOGO_DOT = { cx: ICON.left + inIcon.cx, cy: ICON.top + inIcon.cy, r: inIcon.r } as const;

const Chip = ({ label, delay }: { readonly label: string; readonly delay: number }) => {
  const frame = useCurrentFrame();
  const p = interpolate(frame, [delay, delay + 12], [0, 1], { ...clamp, easing: pop });

  return (
    <div
      style={{
        alignItems: "center",
        backgroundColor: colors.panel,
        border: `1.5px solid ${colors.line}`,
        borderRadius: 99,
        color: colors.text,
        display: "flex",
        fontSize: 32,
        fontWeight: 700,
        gap: 14,
        opacity: Math.min(1, p * 2),
        padding: "16px 30px 16px 24px",
        scale: 0.6 + 0.4 * p,
      }}
    >
      <span style={{ background: green, borderRadius: "50%", height: 16, width: 16 }} />
      {label}
    </div>
  );
};

/** The drop: the icon grows around the dot, the stroke draws, the name comes into focus. */
export const Logo = () => {
  const frame = useCurrentFrame();
  const tile = interpolate(frame, [0, 14], [0, 1], { ...clamp, easing: pop });
  const stroke = interpolate(frame, [5, 24], [0, 1], { ...clamp, easing: ease });
  const scale = interpolate(frame, [0, scenes.logo], [1, 1.04], clamp);
  const iconRect = grow({ x: ICON.left, y: ICON.top, w: ICON.size, h: ICON.size }, 30);
  const dotRect = grow({ x: LOGO_DOT.cx, y: LOGO_DOT.cy, w: 0, h: 0 }, LOGO_DOT.r + 26);
  const brackets = between(dotRect, iconRect, frame, 0, 16);
  const fade = interpolate(frame, [onBeat(3), onBeat(4)], [1, 0], clamp);

  return (
    <Ground glow={[50, 30]}>
      <AbsoluteFill style={{ scale }}>
        <div style={{ left: ICON.left, position: "absolute", top: ICON.top }}>
          <LogoMark size={ICON.size} stroke={stroke} tile={tile} />
        </div>
        <Brackets opacity={fade} rect={brackets} />
        <AbsoluteFill style={{ alignItems: "center", fontFamily: font, top: 470 }}>
          <FocusText
            delay={14}
            style={{ color: colors.text, fontSize: 150, fontWeight: 700, letterSpacing: "-0.03em", lineHeight: 1 }}
            text="rView"
          />
          <FocusText
            delay={onBeat(3)}
            stagger={2}
            style={{ color: colors.muted, fontSize: 44, marginTop: 28 }}
            text={copy.logo.tagline}
          />
          <div style={{ display: "flex", gap: 22, marginTop: 64 }}>
            {copy.logo.chips.map((chip, i) => (
              <Chip delay={onBeat(7 + i)} key={chip} label={chip} />
            ))}
          </div>
        </AbsoluteFill>
      </AbsoluteFill>
    </Ground>
  );
};
