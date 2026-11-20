import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { Brackets, between, type Rect } from "../components/brackets";
import { FocusText } from "../components/focus-text";
import { Ground } from "../components/ground";
import { Key } from "../components/key";
import { Lens } from "../components/lens";
import { Shot, sizes } from "../components/shot";
import { copy } from "../copy";
import { clamp, colors, ease, font } from "../theme";
import { onBeat, scenes } from "../timeline";

const WIDTH = 1000;
const S = WIDTH / sizes[1].w;
const TOP = 50;

// In the capture's pixels: the photo, then the right-click menu.
const PHOTO: Rect = { x: 256, y: 46, w: 456, h: 680 };
const MENU: Rect = { x: 598, y: 264, w: 302, h: 390 };
const MENU_CENTRE = { x: (MENU.x + MENU.w / 2) * S, y: (MENU.y + MENU.h / 2) * S };

const SWITCH = onBeat(8);

/** The viewer itself: arrow keys on the beat, then the viewfinder finds the menu. */
export const Viewer = () => {
  const frame = useCurrentFrame();
  const tilt = interpolate(frame, [0, 22], [1, 0], { ...clamp, easing: ease });
  const zoom = interpolate(frame, [SWITCH, SWITCH + 18], [1, 1.2], { ...clamp, easing: ease });
  const target = between(PHOTO, MENU, frame, SWITCH, SWITCH + 14);
  const keysOut = interpolate(frame, [SWITCH - 8, SWITCH], [1, 0], clamp);
  const press = (beats: number[]) => beats.map(onBeat);

  return (
    <Ground glow={[50, 40]}>
      <Lens duration={scenes.viewer} push={0.02}>
        <div
          style={{
            left: (1920 - WIDTH) / 2,
            position: "absolute",
            top: TOP,
            transform: `perspective(1800px) rotateX(${tilt * 22}deg) translateY(${tilt * 90}px)`,
            transformOrigin: "50% 100%",
          }}
        >
          <div style={{ scale: zoom, transformOrigin: `${MENU_CENTRE.x}px ${MENU_CENTRE.y}px` }}>
            <Shot n={1} width={WIDTH}>
              <Brackets
                opacity={interpolate(frame, [14, 24], [0, 1], clamp)}
                rect={target}
                width={6 / S}
              />
            </Shot>
          </div>
        </div>
        <AbsoluteFill style={{ alignItems: "center", fontFamily: font, top: 860 }}>
          <div style={{ height: 70, position: "relative", width: "100%" }}>
            <AbsoluteFill style={{ alignItems: "center" }}>
              <FocusText
                delay={10}
                out={SWITCH - 10}
                style={{ color: colors.text, fontSize: 56, fontWeight: 700 }}
                text={copy.viewer.first}
              />
            </AbsoluteFill>
            <AbsoluteFill style={{ alignItems: "center" }}>
              <FocusText
                delay={SWITCH + 4}
                style={{ color: colors.text, fontSize: 56, fontWeight: 700 }}
                text={copy.viewer.second}
              />
            </AbsoluteFill>
          </div>
          <div style={{ display: "flex", gap: 56, marginTop: 26, opacity: keysOut }}>
            {copy.viewer.keys.map((group, g) => (
              <div key={group.label} style={{ alignItems: "center", display: "flex", gap: 10 }}>
                {group.keys.map((label, k) => (
                  <Key
                    delay={onBeat(1.5 + g) + k * 3}
                    key={label}
                    label={label}
                    pressAt={
                      label === "→" ? press([3, 4, 5]) : label === "←" ? press([6]) : label === "R" ? press([7]) : []
                    }
                  />
                ))}
                <span style={{ color: colors.muted, fontSize: 28, marginLeft: 8 }}>{group.label}</span>
              </div>
            ))}
          </div>
        </AbsoluteFill>
      </Lens>
    </Ground>
  );
};
