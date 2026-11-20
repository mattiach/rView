import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { Brackets, between, grow } from "../components/brackets";
import { FocusText } from "../components/focus-text";
import { Ground } from "../components/ground";
import { copy } from "../copy";
import { clamp, colors, ease, easeIn, font } from "../theme";
import { onBeat, scenes } from "../timeline";

const lit = new Set<string>(copy.formats.lit);
const BUILD = onBeat(12);

const Chip = ({ label }: { readonly label: string }) => (
  <span
    style={{
      backgroundColor: lit.has(label) ? "rgb(94 240 168 / 10%)" : colors.panel,
      border: `2px solid ${lit.has(label) ? "rgb(94 240 168 / 55%)" : colors.line}`,
      borderRadius: 99,
      color: lit.has(label) ? colors.lime : colors.text,
      flexShrink: 0,
      fontFamily: font,
      fontSize: 46,
      fontWeight: 700,
      padding: "20px 44px",
      whiteSpace: "nowrap",
    }}
  >
    {label}
  </span>
);

/** A floor of format names streaming past, the breakdown building into the next drop. */
export const Formats = () => {
  const frame = useCurrentFrame();
  const rush = frame > BUILD ? ((frame - BUILD) / (scenes.formats - BUILD)) ** 2 * 900 : 0;
  const closeIn = interpolate(frame, [BUILD, scenes.formats], [1, 1.25], { ...clamp, easing: easeIn });
  const focus = interpolate(frame, [0, 14], [0, 1], { ...clamp, easing: ease });
  const title = { x: 340, y: 400, w: 1240, h: 280 };

  return (
    <Ground glow={[50, 50]} glowColor="rgb(94 240 168 / 10%)">
      <AbsoluteFill
        style={{
          filter: `blur(${(1 - focus) * 16}px)`,
          justifyContent: "center",
          scale: closeIn,
        }}
      >
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 34,
            opacity: 0.85,
            transform: "perspective(1400px) rotateX(32deg) rotateZ(-9deg) scale(1.45)",
          }}
        >
          {copy.formats.rows.map((row, r) => {
            const dir = r % 2 ? 1 : -1;
            const x = dir * (frame * (2.4 + r * 0.5) + rush) - 600 - r * 170;
            return (
              <div key={row[0]} style={{ display: "flex", gap: 26, translate: `${dir > 0 ? x - 1400 : x}px 0` }}>
                {[...row, ...row, ...row].map((label, i) => (
                  <Chip key={`${label}-${i}`} label={label} />
                ))}
              </div>
            );
          })}
        </div>
      </AbsoluteFill>
      <AbsoluteFill
        style={{
          background: "radial-gradient(40% 32% at 50% 50%, rgb(14 15 18 / 96%) 30%, rgb(14 15 18 / 70%) 65%, transparent 100%)",
        }}
      />
      <AbsoluteFill style={{ alignItems: "center", fontFamily: font, justifyContent: "center", scale: closeIn }}>
        <FocusText
          delay={8}
          style={{ color: colors.text, fontSize: 100, fontWeight: 700, letterSpacing: "-0.02em" }}
          text={copy.formats.title}
        />
        <div
          style={{
            color: colors.muted,
            fontSize: 30,
            marginTop: 26,
            opacity: interpolate(frame, [onBeat(4), onBeat(5)], [0, 1], clamp),
          }}
        >
          {copy.formats.footnote}
        </div>
        <Brackets
          opacity={interpolate(frame, [onBeat(1), onBeat(2)], [0, 1], clamp)}
          rect={between(grow(title, 220), title, frame, onBeat(1), onBeat(2))}
        />
      </AbsoluteFill>
      <AbsoluteFill
        style={{
          backgroundColor: colors.stroke,
          opacity: interpolate(frame, [scenes.formats - 8, scenes.formats], [0, 0.35], { ...clamp, easing: easeIn }),
        }}
      />
    </Ground>
  );
};
