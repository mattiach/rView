import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { FocusText } from "../components/focus-text";
import { Ground } from "../components/ground";
import { Lens } from "../components/lens";
import { copy } from "../copy";
import { clamp, colors, ease, font, green } from "../theme";
import { BEAT, onBeat, scenes } from "../timeline";

const LEFT = 330;
const TOP = 200;
const LINE = 180;
const lineAt = (i: number) => onBeat(4 * i);

/** Three words, one a bar. The logo's dot marks the line that just landed. */
export const Free = () => {
  const frame = useCurrentFrame();
  const { lines, footnote } = copy.free;
  let y = TOP + LINE / 2;
  lines.forEach((_, i) => {
    const p = interpolate(frame, [lineAt(i), lineAt(i) + 10], [0, 1], { ...clamp, easing: ease });
    y += i > 0 ? LINE * p : 0;
  });
  const appear = interpolate(frame, [0, 10], [0, 1], { ...clamp, easing: ease });
  const pulse = 1 + 0.18 * Math.exp(-(frame % BEAT) / 4);
  const r = 24 * appear * pulse;

  return (
    <Ground glow={[25, 45]} glowColor="rgb(94 240 168 / 13%)">
      <Lens duration={scenes.free} push={0.025}>
        <div
          style={{
            background: green,
            borderRadius: "50%",
            boxShadow: "0 0 50px rgb(94 240 168 / 45%)",
            height: r * 2,
            left: LEFT - 90 - r,
            position: "absolute",
            top: y - r,
            width: r * 2,
          }}
        />
        <AbsoluteFill style={{ fontFamily: font, left: LEFT, top: TOP }}>
          {lines.map((line, i) => (
            <FocusText
              delay={lineAt(i)}
              key={line}
              style={{
                color: i === lines.length - 1 ? colors.lime : colors.text,
                fontSize: 160,
                fontWeight: 700,
                height: LINE,
                letterSpacing: "-0.03em",
                lineHeight: `${LINE}px`,
              }}
              text={line}
            />
          ))}
          <div style={{ alignItems: "center", color: colors.muted, display: "flex", fontSize: 32, gap: 22, marginTop: 70 }}>
            {footnote.map((item, i) => {
              const p = interpolate(frame, [onBeat(12 + i), onBeat(12 + i) + 10], [0, 1], { ...clamp, easing: ease });
              return (
                <span key={item} style={{ alignItems: "center", display: "flex", gap: 22, opacity: p, translate: `0 ${(1 - p) * 16}px` }}>
                  {i > 0 && <span style={{ backgroundColor: colors.muted, borderRadius: "50%", height: 6, opacity: 0.6, width: 6 }} />}
                  {item}
                </span>
              );
            })}
          </div>
        </AbsoluteFill>
      </Lens>
    </Ground>
  );
};
