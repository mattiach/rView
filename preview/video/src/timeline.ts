import timeline from "./timeline.json";

// Scenes are measured in bars of the soundtrack (scripts/make-music.mjs reads the
// same file), so every cut lands on a downbeat. 120 BPM at 30 fps: 15 frames a beat.
export const FPS = 30;
export const BEAT = (FPS * 60) / timeline.bpm;
export const BAR = 4 * BEAT;

export type SceneId = keyof typeof timeline.bars;

export const scenes = Object.fromEntries(
  Object.entries(timeline.bars).map(([id, bars]) => [id, bars * BAR])
) as Record<SceneId, number>;

export const TOTAL_FRAMES = Object.values(scenes).reduce((a, b) => a + b, 0);

/** Frame within a scene of its nth beat; beat 0 is the cut into it. */
export const onBeat = (n: number) => Math.round(n * BEAT);
