import { Audio } from "@remotion/media";
import type { FC } from "react";
import { AbsoluteFill, interpolate, Series, staticFile } from "remotion";
import { Closing } from "./scenes/closing";
import { Feature, type FeatureId } from "./scenes/feature";
import { Formats } from "./scenes/formats";
import { Free } from "./scenes/free";
import { Logo } from "./scenes/logo";
import { Opening } from "./scenes/opening";
import { Speed } from "./scenes/speed";
import { Viewer } from "./scenes/viewer";
import { clamp } from "./theme";
import { type SceneId, scenes, TOTAL_FRAMES } from "./timeline";

const MUSIC_FADE_OUT = 45;

const musicVolume = (f: number) => interpolate(f, [TOTAL_FRAMES - MUSIC_FADE_OUT, TOTAL_FRAMES], [1, 0], clamp);

const feature = (id: FeatureId) => () => <Feature id={id} />;

/** Every scene, in film order (the order of src/timeline.json). */
export const sceneComponents: Record<SceneId, FC> = {
  opening: Opening,
  logo: Logo,
  speed: Speed,
  viewer: Viewer,
  formats: Formats,
  markup: feature("markup"),
  adjust: feature("adjust"),
  crop: feature("crop"),
  info: feature("info"),
  free: Free,
  closing: Closing,
};

/** Straight cuts on the downbeat; the soundtrack is the only sound. */
export const Film = () => (
  <AbsoluteFill style={{ backgroundColor: "#0e0f12" }}>
    <Audio src={staticFile("audio/music.wav")} volume={musicVolume} />
    <Series>
      {(Object.keys(scenes) as SceneId[]).map((id) => {
        const Scene = sceneComponents[id];
        return (
          <Series.Sequence durationInFrames={scenes[id]} key={id} name={id}>
            <Scene />
          </Series.Sequence>
        );
      })}
    </Series>
  </AbsoluteFill>
);
