# Performance

Figures measured on 3 November 2026, on a laptop with an Intel Core i9-13900H and 64 GB of memory,
under Linux (GNOME, Wayland). They are repeatable: run the same commands to measure your machine.

## Opening

| What | Time |
|---|---|
| the window appears (release build) | 110 ms |
| the same, when the machine has an NVIDIA driver installed but not running | 1.95 s |
| reading 5,000 activities and their tracks (1 MB + 44 MB of JSON) | 120 ms, after the window shows |

The window shows first; the activities are read in the background and drawn when they are in.

On a laptop whose NVIDIA graphics are switched off, the graphics library asks every installed
Vulkan driver for its devices, and NVIDIA's takes almost two seconds to say it has none. Pointing
Vulkan at the active driver avoids it: `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json astrolabe`.

Memory: about 320 MB resident after five seconds, and as much with an empty data folder as with
ten years of activities (2,152): the activities are not what takes it.

## A large history

5,000 activities over twenty years, each with a track of 400 points (2 million points in all), in
the window on GPUI's test platform (layout and painting, without the GPU):

| What | Time |
|---|---|
| first draw, on Today | 360 ms |
| open the Journal | 240 ms |
| open the Calendar | 70 ms |
| open Statistics | 18 ms |
| open Form | 17 ms |
| open Records | 50 ms |
| open the Heatmap (its raster is drawn off the UI thread) | 290 ms |
| the heat raster alone, 2400 × 1600 pixels | 410 ms, off the UI thread |
| one `↓` in the Journal | 66 ms |

One file:

| What | Time |
|---|---|
| derive an 8-hour track at one sample a second (28,800 samples): map line, best efforts, profile, heart rate zones, splits | 5 ms |

## Known slow spots

- **Moving the Journal's selection** grows with the history: 10 ms a step with 500 activities,
  23 ms with 1,500, 66 ms with 5,000, which a held key feels.
- **The first draw of Today** grows the same way: 50 ms with 500 activities, 120 ms with 1,500,
  360 ms with 5,000.

## Measuring

```bash
# The large history (an ignored test: optimised, with the debug selectors the clicks need).
# MEASURE_N sets the number of activities (5,000 by default).
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo test -- --ignored --nocapture measure

# The real window, on made-up data.
cargo build --release
scripts/demo-data.py /tmp/demo --days 3650
XDG_DATA_HOME=/tmp/demo target/release/astrolabe
```
