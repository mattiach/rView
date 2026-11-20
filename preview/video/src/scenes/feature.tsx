import { interpolate, useCurrentFrame } from "remotion";
import { Brackets, between, grow, type Rect } from "../components/brackets";
import { FocusText } from "../components/focus-text";
import { Ground } from "../components/ground";
import { Key } from "../components/key";
import { Lens } from "../components/lens";
import { Shot, type ShotId, sizes } from "../components/shot";
import { copy } from "../copy";
import { clamp, colors, ease, font } from "../theme";
import { onBeat, scenes } from "../timeline";

// The maximised captures have wide empty margins: show the middle.
const MIDDLE: Rect = { x: 470, y: 44, w: 1000, h: 868 };

// Per feature: the capture, its crop, the detail the viewfinder locks on (capture
// pixels), and which side the capture sits on, alternating from cut to cut.
export const features = {
  markup: { n: 2, crop: undefined, focus: { x: 556, y: 428, w: 156, h: 156 }, side: "right" },
  adjust: { n: 3, crop: undefined, focus: { x: 684, y: 110, w: 303, h: 340 }, side: "left" },
  crop: { n: 4, crop: MIDDLE, focus: { x: 528, y: 307, w: 651, h: 341 }, side: "right" },
  info: { n: 5, crop: MIDDLE, focus: { x: 598, y: 307, w: 468, h: 433 }, side: "left" },
} as const satisfies Record<string, { n: ShotId; crop?: Rect; focus: Rect; side: "left" | "right" }>;

export type FeatureId = keyof typeof features;
const ids = Object.keys(features) as FeatureId[];

const BOX = { w: 1150, h: 840 } as const;
const TEXT_W = 560;

/** The preview pages' step bars: one per feature, the current one lit. */
const Steps = ({ index }: { readonly index: number }) => (
  <div style={{ alignItems: "center", color: colors.muted, display: "flex", fontSize: 20, gap: 8, letterSpacing: "0.08em" }}>
    {ids.map((id, i) => (
      <i key={id} style={{ backgroundColor: i === index ? colors.blue : colors.line, borderRadius: 2, height: 4, width: 28 }} />
    ))}
    <span style={{ marginLeft: 10 }}>
      {String(index + 1).padStart(2, "0")} / {String(ids.length).padStart(2, "0")}
    </span>
  </div>
);

export const Feature = ({ id }: { readonly id: FeatureId }) => {
  const frame = useCurrentFrame();
  const { n, crop, focus, side } = features[id];
  const text = copy.features[id];
  const c = crop ?? { x: 0, y: 0, ...sizes[n] };
  const width = Math.min(BOX.w, (BOX.h * c.w) / c.h);
  const s = width / c.w;
  const height = c.h * s;
  const right = side === "right";
  const shotLeft = right ? 1920 - 90 - width : 90;
  const textLeft = right ? 110 : 1920 - 110 - TEXT_W;

  const enter = interpolate(frame, [0, 18], [0, 1], { ...clamp, easing: ease });
  const turn = interpolate(frame, [0, scenes[id]], [16, 6], { ...clamp, easing: ease });
  const target = between(grow(c, -10), grow(focus, 14), frame, 10, 26);

  return (
    <Ground glow={[right ? 68 : 32, 50]}>
      <Lens duration={scenes[id]} push={0.03}>
        <div
          style={{
            left: shotLeft,
            position: "absolute",
            top: (1080 - height) / 2,
            transform: `perspective(2200px) rotateY(${right ? -turn : turn}deg) translateX(${(1 - enter) * (right ? 160 : -160)}px)`,
            transformOrigin: right ? "100% 50%" : "0 50%",
          }}
        >
          <Shot crop={crop} n={n} width={width}>
            <Brackets opacity={interpolate(frame, [8, 14], [0, 1], clamp)} rect={target} width={6 / s} />
          </Shot>
        </div>
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            fontFamily: font,
            height: 1080,
            justifyContent: "center",
            left: textLeft,
            position: "absolute",
            width: TEXT_W,
          }}
        >
          <Steps index={ids.indexOf(id)} />
          <div style={{ marginTop: 44 }}>
            <span
              style={{
                backgroundColor: "rgb(26 125 255 / 12%)",
                border: "1px solid rgb(26 125 255 / 35%)",
                borderRadius: 99,
                color: colors.blue,
                fontSize: 20,
                fontWeight: 700,
                letterSpacing: "0.14em",
                padding: "8px 18px",
                textTransform: "uppercase",
              }}
            >
              {text.tag}
            </span>
          </div>
          <FocusText
            delay={4}
            style={{ color: colors.text, fontSize: 100, fontWeight: 700, letterSpacing: "-0.03em", lineHeight: 1.05, marginTop: 30 }}
            text={text.title}
          />
          <FocusText
            delay={onBeat(1.5)}
            duration={12}
            stagger={1}
            style={{ color: colors.muted, fontSize: 32, lineHeight: 1.45, marginTop: 26 }}
            text={text.body}
          />
          <div style={{ display: "flex", gap: 10, marginTop: 40 }}>
            {text.keys.map((label, k) => (
              <Key delay={onBeat(3) + k * 3} key={label} label={label} pressAt={[onBeat(5)]} />
            ))}
          </div>
        </div>
      </Lens>
    </Ground>
  );
};
