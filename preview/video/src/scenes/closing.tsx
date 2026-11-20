import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { Brackets, between, grow } from "../components/brackets";
import { FocusText } from "../components/focus-text";
import { Ground } from "../components/ground";
import { LogoMark } from "../components/logo";
import { copy } from "../copy";
import { clamp, colors, ease, easeIn, font, green, pop } from "../theme";
import { onBeat, scenes } from "../timeline";

const ICON = 220;
const LOCKUP = { x: 520, y: 170, w: 880, h: 720 };
const FADE = 40;

/** The lockup, the address, and the viewfinder framing it one last time. */
export const Closing = () => {
  const frame = useCurrentFrame();
  const tile = interpolate(frame, [0, 14], [0, 1], { ...clamp, easing: pop });
  const stroke = interpolate(frame, [4, 22], [0, 1], { ...clamp, easing: ease });
  const dot = interpolate(frame, [10, 24], [0, 1], { ...clamp, easing: pop });
  const url = interpolate(frame, [onBeat(4), onBeat(5)], [0, 1], { ...clamp, easing: ease });
  const out = interpolate(frame, [scenes.closing - FADE, scenes.closing], [1, 0], { ...clamp, easing: easeIn });
  const scale = interpolate(frame, [0, scenes.closing], [1.03, 1], clamp);

  return (
    <Ground glow={[50, 40]}>
      <AbsoluteFill style={{ alignItems: "center", fontFamily: font, opacity: out, paddingTop: 210, scale }}>
        <LogoMark dot={dot} size={ICON} stroke={stroke} tile={tile} />
        <FocusText
          delay={10}
          style={{ color: colors.text, fontSize: 130, fontWeight: 700, letterSpacing: "-0.03em", lineHeight: 1, marginTop: 40 }}
          text="rView"
        />
        <FocusText
          delay={onBeat(2)}
          stagger={2}
          style={{ color: colors.muted, fontSize: 40, marginTop: 24 }}
          text={copy.closing.tagline}
        />
        <div
          style={{
            alignItems: "center",
            backgroundColor: colors.panel,
            border: `1.5px solid ${colors.line}`,
            borderRadius: 99,
            color: colors.text,
            display: "flex",
            fontSize: 36,
            fontWeight: 700,
            gap: 16,
            marginTop: 50,
            opacity: url,
            padding: "18px 36px 18px 28px",
            translate: `0 ${(1 - url) * 20}px`,
          }}
        >
          <span style={{ background: green, borderRadius: "50%", height: 18, width: 18 }} />
          {copy.closing.url}
        </div>
        <Brackets
          opacity={interpolate(frame, [onBeat(6), onBeat(7)], [0, 1], clamp)}
          rect={between(grow(LOCKUP, 240), LOCKUP, frame, onBeat(6), onBeat(7.5))}
        />
      </AbsoluteFill>
    </Ground>
  );
};
