# rView promotional video

A 72 second, 30 fps, 1920×1080 film made with [Remotion](https://remotion.dev): `rview.mp4` plus
`poster.png`. The MP4 is not committed.

From this folder:

```sh
pnpm install
pnpm dev         # Remotion Studio: preview and scrub
pnpm render      # writes rview.mp4 and poster.png
```

`dev` and `render` first run `pnpm assets`: `scripts/copy-assets.mjs` copies the screenshots from
`../screenshots/` and the Lato font from `assets/fonts/` into `public/`, and `scripts/make-music.mjs`
writes the soundtrack, `public/audio/music.wav`. `public/` is not committed.

## Soundtrack

The music is synthesised by `scripts/make-music.mjs` (kick, claps, hats, bass, pad, arpeggio, lead,
risers, echo and reverb, all in plain JavaScript), so it is original and carries no third-party rights.
It is written at 120 BPM against `src/timeline.json`: the drums drop on the logo, break for the formats,
drop again for Markup and wind down over the closing. Change a scene's length there and the music follows.

## Where to change things

| What                                             | Where                               |
| ------------------------------------------------ | ----------------------------------- |
| Every line of on-screen text                     | `src/copy.ts`                       |
| Scene order and lengths in bars                  | `src/timeline.json`                 |
| Which capture a Markup scene shows, its crop and the detail the viewfinder locks on | `features` in `src/scenes/feature.tsx` |
| Colours, font, easing                            | `src/theme.ts`                      |

Each scene is also registered on its own under _Scenes_ in Studio. Screenshot sizes are listed in
`src/components/shot.tsx`; update them if a capture in `../screenshots/` changes size.

## Licensing

This folder's code is MIT-licensed, like the rest of rView. Lato is under the SIL Open Font License 1.1
(`assets/fonts/OFL.txt`).

Remotion is not open source. It is distributed under the [Remotion License](https://www.remotion.dev/license),
which is free for individuals, small companies and non-profits and requires a company licence for larger
for-profit organisations.
