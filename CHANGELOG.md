# Changelog

Notable changes, newest first. Versions follow [Semantic Versioning](https://semver.org); until 1.0, a minor version may change how things look or are stored.

## Unreleased

- **A new logo**: the limb of an astrolabe and the circles of its plate around the zenith, with fewer and thicker lines at 16 and 32 px.
- **Loops**: a new page (`Ctrl + L`).
  - Click a start on the map (or type an address, a place or coordinates), ask for an exact distance (±2 %) or a range, streets or trails,
    asphalt or not, and a climb if you like. Up to five different running or trail loops come on
    OpenStreetMap's paths, each with its distance, climb, descent, shares of trail, asphalt and
    out-and-back, and an altitude profile linked to the map.
  - The chosen loop saves as GPX (`Ctrl + S`), and its seed gives it back.
  - Every change of the start, the points, the zones or the line can be undone (`Ctrl + Z`): the
    arrows on the map name the change they take back, and the line under the map says it.
  - Loops run on paths, footways, sidewalks and pedestrian areas (a seafront promenade drawn as
    a polygon too), never on cycle tracks; a road without a sidewalk only links the rest.
  - The map is shaded with the relief, and can show the IGN's aerial photos instead, in 2D as in 3D.
  - The region's extract (Provence-Alpes-Côte d'Azur for now) is downloaded once from Geofabrik,
    and the elevation tiles that cover it from the Amazon open data bucket. The network of paths is
    built and kept on disk, and everything is computed on the machine: no account, no routing
    service, offline afterwards.
  - A damaged network or extract is set aside, never deleted.
- **Storage errors are shown**: an activity cache that can't be read is set aside as `activities.damaged-<number>.json` and reported, instead of opening on an empty journal; a write that fails in the background (tracks, settings) shows in the status line.
- **Windows**: the app no longer opens a console window behind its own.
- **User guide** (`book/`): install, first steps, the pages, synchronising and working offline, shortcuts, your data and backups, troubleshooting. The README is shorter and shows made-up data.

## 0.1.0

First release.

- **Your Garmin data, kept on your machine**: the list of activities and the file of each one (GPS track, heart rate, laps) are downloaded with your Garmin personal API key and stored locally, so the app opens instantly, online or not. The key goes to the system keyring, or to a file the app encrypts itself where there is none. Nothing is destroyed on disk.
- **Today**: this week against your median week, the run of weeks with an outing, the year so far against the same date a year before, six months week by week, the latest outings and the records of the last two weeks.
- **Journal**: every activity week by week, with a search, a filter by sport, columns of your choice (including the gap to your usual pace, the time in each heart rate zone and an altitude profile) and a summary of how each synchronisation ended.
- **Calendar**: by week, month and year, with the activities in the day cells.
- **Activity sheet**: the map of the track, the pace, heart rate and elevation curves linked to the map, the splits and the laps.
- **Statistics**: a measure, a grouping (day, week, month, year), a period and sports, with the running totals of several years compared.
- **Form**: training load, fatigue and freshness.
- **Records**: best efforts for road, trail, cycling and swimming, the ten best of each unfolded, and records of every kind.
- **Heatmap**: all your tracks on one map, with the activities that pass through the point you click.
- Navigation back and forward (Alt + arrows, mouse buttons, Escape), a window moved from its top edge, light and dark themes and English or French, following the system.
- The Astrolabe logo.
- Downloads for Arch Linux, macOS and Windows with every release, and a `make install` for other Linux distributions. The macOS and Windows files are not signed.
