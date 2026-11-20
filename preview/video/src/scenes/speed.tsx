import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { Brackets, between, grow, type Rect } from "../components/brackets";
import { FocusText } from "../components/focus-text";
import { Ground } from "../components/ground";
import { Lens } from "../components/lens";
import { copy } from "../copy";
import { clamp, colors, ease, font, green } from "../theme";
import { onBeat, scenes } from "../timeline";

const CARD = { w: 470, h: 330, gap: 50, top: 360 } as const;
const cardLeft = (i: number) => (1920 - 3 * CARD.w - 2 * CARD.gap) / 2 + i * (CARD.w + CARD.gap);
const cardRect = (i: number): Rect => grow({ x: cardLeft(i), y: CARD.top, w: CARD.w, h: CARD.h }, 16);
const cardAt = (i: number) => onBeat(2 + 3 * i);

const Stat = ({ i }: { readonly i: number }) => {
  const frame = useCurrentFrame();
  const { value, decimals, unit, label } = copy.speed.stats[i];
  const start = cardAt(i);
  const enter = interpolate(frame, [start, start + 14], [0, 1], { ...clamp, easing: ease });
  const count = interpolate(frame, [start, start + 22], [0, 1], { ...clamp, easing: ease });
  const load = interpolate(frame, [start + 4, start + 14], [0, 1], { ...clamp, easing: ease });

  return (
    <div
      style={{
        backgroundColor: "rgb(26 28 33 / 70%)",
        border: `1.5px solid ${colors.line}`,
        borderRadius: 24,
        display: "flex",
        filter: enter < 1 ? `blur(${(1 - enter) * 14}px)` : undefined,
        flexDirection: "column",
        height: CARD.h,
        justifyContent: "center",
        left: cardLeft(i),
        opacity: enter,
        padding: "0 44px",
        position: "absolute",
        top: CARD.top,
        translate: `0 ${(1 - enter) * 50}px`,
        width: CARD.w,
      }}
    >
      <div style={{ alignItems: "baseline", display: "flex", gap: 14 }}>
        <span style={{ color: colors.text, fontSize: 150, fontVariantNumeric: "tabular-nums", fontWeight: 700, letterSpacing: "-0.04em", lineHeight: 1 }}>
          {(value * count).toFixed(decimals)}
        </span>
        <span style={{ backgroundClip: "text", backgroundImage: green, color: "transparent", fontSize: 64, fontWeight: 700 }}>{unit}</span>
      </div>
      <div style={{ color: colors.muted, fontSize: 30, marginTop: 18 }}>{label}</div>
      <div style={{ backgroundColor: colors.line, borderRadius: 3, height: 6, marginTop: 30, overflow: "hidden" }}>
        <div style={{ background: green, height: "100%", width: `${load * 100}%` }} />
      </div>
    </div>
  );
};

/** README's measurements, one per card, the viewfinder hopping to each as it lands. */
export const Speed = () => {
  const frame = useCurrentFrame();
  const hops = copy.speed.stats.map((_, i) => cardAt(i));
  let rect = grow(cardRect(0), 200);
  hops.forEach((at, i) => {
    rect = between(rect, cardRect(i), frame, at, at + 12);
  });
  const all = between(rect, grow({ x: cardLeft(0), y: CARD.top, w: 3 * CARD.w + 2 * CARD.gap, h: CARD.h }, 30), frame, onBeat(11), onBeat(12));
  const show = interpolate(frame, [cardAt(0), cardAt(0) + 8], [0, 1], clamp);

  return (
    <Ground glow={[50, 55]}>
      <Lens duration={scenes.speed}>
        <AbsoluteFill style={{ alignItems: "center", fontFamily: font }}>
          <FocusText
            delay={4}
            style={{ color: colors.text, fontSize: 92, fontWeight: 700, letterSpacing: "-0.02em", marginTop: 150 }}
            text={copy.speed.title}
          />
          {copy.speed.stats.map((stat, i) => (
            <Stat i={i} key={stat.label} />
          ))}
          <div
            style={{
              bottom: 170,
              color: colors.muted,
              fontSize: 24,
              letterSpacing: "0.04em",
              opacity: interpolate(frame, [onBeat(11), onBeat(12)], [0, 0.85], clamp),
              position: "absolute",
            }}
          >
            {copy.speed.footnote}
          </div>
        </AbsoluteFill>
        <Brackets opacity={show} rect={all} />
      </Lens>
    </Ground>
  );
};
