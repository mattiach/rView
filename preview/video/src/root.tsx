import { Composition, Folder } from "remotion";
import { Film, sceneComponents } from "./film";
import { FPS, type SceneId, scenes, TOTAL_FRAMES } from "./timeline";

const size = { fps: FPS, height: 1080, width: 1920 } as const;

export const RemotionRoot = () => (
  <>
    <Composition component={Film} durationInFrames={TOTAL_FRAMES} id="RView" {...size} />
    {/* Each scene on its own, for working on one without scrubbing the film. */}
    <Folder name="Scenes">
      {(Object.keys(scenes) as SceneId[]).map((id) => (
        <Composition component={sceneComponents[id]} durationInFrames={scenes[id]} id={id} key={id} {...size} />
      ))}
    </Folder>
  </>
);
