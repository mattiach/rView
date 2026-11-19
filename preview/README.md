# Preview

Promotional images of rView, rebuilt from screenshots whenever the interface changes noticeably.

- `screenshots/`: captures of the app window, taken by hand.
- `src/`: one HTML page per feature, all styled by `preview.css`. The same page gives both layouts: wide
  (screenshot left, text and shortcuts right) and square (screenshot on top, text below).
- `renders/wide/` (1920×1080) and `renders/square/` (1200×1200): the generated PNGs. Never edit them by hand.

## Updating

1. Save new screenshots over the files in `screenshots/`:

   | File    | Screen                                                        |
   | ------- | ------------------------------------------------------------- |
   | `1.png` | Viewer, right-click menu open                                 |
   | `2.png` | Markup, a loupe selected                                      |
   | `3.png` | Markup, Adjust with the before / after divider                |
   | `4.png` | Markup, Crop with the preset list open (maximised window)     |
   | `5.png` | Markup, File Info open (maximised window)                     |

2. From the repository root, run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File preview\render.ps1
   ```

   It uses headless Microsoft Edge; set `EDGE_PATH` if it is not in the default location.

3. Check the PNGs in `renders/`. A screenshot is shown whole unless its page crops it: on the `.shot` element,
   `--ratio` sets the shape of the visible area, `--focus` which part of the capture it shows and `--trim`
   the fraction of the height cut off the top (the title and menu bars). Pages 4 and 5 use them because a
   maximised window has wide empty margins.

Text is in each page; headings and shortcuts must match the app (`app/src/actions.rs`).
