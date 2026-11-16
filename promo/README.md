# Promo video

`astrolabe-motion.mp4`: 30 seconds, 1920×1080, 30 fps, with its own soundtrack. Everything is made by code, from the app's colours (`src/ui/theme.rs`), its IBM Plex fonts, its icon, the demo routes around Annecy (`scripts/demo-routes.json`) and the screenshots in `docs/`.

| Time | Scene | Sound |
|---|---|---|
| 0–3 s | A star, the astrolabe's rings drawing around it, its scale; "take the height of a star" typed | pad, riser, key clicks, whoosh into the star |
| 3–6 s | The star floods the screen: "read a scale." over a sliding ruler, then "Know where you stand." | half-time kick and bass, snare build |
| 6–10 s | This week, 1h28, against six months of weeks and the median week | the full beat comes in |
| 10–13.5 s | A 66-week streak, a year of days filling in | arpeggio |
| 13.5–19 s | A 15 km trail loop drawn by grade, its profile and figures, then every track on one map | key clicks on the coordinates, whoosh |
| 19–23 s | Eight cuts on eight beats, dark, light and accent: Yours. On your machine. Opens instantly. Talks to Garmin. Nobody else. Figures, never advice. one binary · rust · gpu | a chord stab on each cut |
| 23–27 s | A tilted wall of the app's pages, the features named one per eighth note, then the Today page flattening out | ticks, brighter arpeggio, whoosh |
| 27–30 s | The mark: the rings snap together, the star lands, *Astrolabe — Know where you stand.* | end chime: an A major bell |

## Rebuilding

```bash
python3 prep.py       # timeline.json / timeline.js (shared by picture and sound) and routes.js
python3 audio.py      # soundtrack.wav: drums, bass, pad, arpeggio, clicks, whooshes, chime, all synthesised
node render.js frames # 900 PNG frames from video.html, in headless Chromium (Playwright)
ffmpeg -framerate 30 -i frames/f%04d.png -i soundtrack.wav -vf hqdn3d=3:2:4:4 \
  -c:v libx264 -crf 20 -pix_fmt yuv420p -movflags +faststart -c:a aac -b:a 256k -shortest astrolabe-motion.mp4
```

`video.html` opened in a browser plays the animation live, silent; `video.html?t=12.5` shows one moment.
