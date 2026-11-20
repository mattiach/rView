import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { Brackets, between, grow, type Rect } from "../components/brackets";
import { FocusText } from "../components/focus-text";
import { Ground } from "../components/ground";
import { copy } from "../copy";
import { clamp, colors, ease, font, green, pop } from "../theme";
import { BEAT, onBeat } from "../timeline";
import { LOGO_DOT } from "./logo";

const WIDE: Rect = { x: 400, y: 370, w: 1120, h: 260 };
const NARROW: Rect = { x: 620, y: 370, w: 680, h: 260 };
const AT_DOT: Rect = grow({ x: LOGO_DOT.cx, y: LOGO_DOT.cy, w: 0, h: 0 }, LOGO_DOT.r + 26);

// One phrase a bar; the last bar hands over to the logo.
const phraseAt = (i: number) => onBeat(2 + 4 * i);
const HANDOVER = onBeat(13);

const DOT_START = { cx: 960, cy: 720, r: 12 };

/**
 * Three phrases come into focus inside the viewfinder. Then the viewfinder closes
 * on the green dot, which flies to where the logo's dot will be: the next scene
 * builds the icon around it.
 */
export const Opening = () => {
  const frame = useCurrentFrame();

  const arrive = interpolate(frame, [onBeat(1), onBeat(2)], [0, 1], { ...clamp, easing: ease });
  const kick = Math.max(...copy.opening.map((_, i) => (frame >= phraseAt(i) ? Math.exp(-(frame - phraseAt(i)) / 6) : 0)));
  const base = between(WIDE, NARROW, frame, phraseAt(2) - 6, phraseAt(2) + 8);
  const framed = between(grow(base, 18 * kick), AT_DOT, frame, HANDOVER, onBeat(15.5));
  const rect = between(grow(WIDE, 260), framed, frame, onBeat(1), onBeat(2));

  const fly = interpolate(frame, [HANDOVER, onBeat(15.8)], [0, 1], { ...clamp, easing: ease });
  const appear = interpolate(frame, [8, 22], [0, 1], { ...clamp, easing: pop });
  const pulse = fly > 0 ? 1 : 1 + 0.22 * Math.exp(-(frame % BEAT) / 4);
  const dot = {
    cx: DOT_START.cx + (LOGO_DOT.cx - DOT_START.cx) * fly,
    cy: DOT_START.cy + (LOGO_DOT.cy - DOT_START.cy) * fly,
    r: (DOT_START.r + (LOGO_DOT.r - DOT_START.r) * fly) * appear * pulse,
  };

  return (
    <Ground glow={[50, 50]} glowColor="rgb(26 125 255 / 14%)">
      <AbsoluteFill style={{ alignItems: "center", justifyContent: "center" }}>
        {copy.opening.map((phrase, i) => (
          <FocusText
            delay={phraseAt(i)}
            key={phrase}
            out={i < copy.opening.length - 1 ? phraseAt(i + 1) - 10 : HANDOVER - 4}
            stagger={4}
            style={{
              color: colors.text,
              fontFamily: font,
              fontSize: 110,
              fontWeight: 700,
              letterSpacing: "-0.02em",
              position: "absolute",
              top: 440,
            }}
            text={phrase}
          />
        ))}
      </AbsoluteFill>
      <Brackets opacity={arrive} rect={rect} />
      <div
        style={{
          background: green,
          borderRadius: "50%",
          boxShadow: `0 0 ${40 + 40 * fly}px rgb(94 240 168 / 45%)`,
          height: dot.r * 2,
          left: dot.cx - dot.r,
          position: "absolute",
          top: dot.cy - dot.r,
          width: dot.r * 2,
        }}
      />
    </Ground>
  );
};
