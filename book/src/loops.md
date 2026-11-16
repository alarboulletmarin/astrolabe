# Planning loops

The **Loops** page (`Ctrl + L`, `Cmd + L` on macOS) proposes running and trail loops from a start you
choose, on the real paths of OpenStreetMap. You get the distance, the climb and descent, the altitude
profile, and a GPX file for your watch or another app. Everything is computed on your machine: there
is no account, no credits, no routing service and no tracking.

## The region, once

The first time, the page offers to **Download the region**: Provence-Alpes-Côte d'Azur for now.
Astrolabe then does the following:

1. It downloads the region's OpenStreetMap extract from Geofabrik, a few hundred MB, kept in the data
   folder.
2. It builds the network of paths one can run on from it: four passes over the extract. Paths,
   tracks, footways, bridleways, steps, pedestrian and living streets, residential, unclassified,
   service and tertiary roads are kept, and so are pedestrian areas drawn as a polygon (a seafront
   promenade, a square): one runs round their edge. Secondary and primary roads are kept too, to
   tie the rest together. Ways that are private, forbidden, or closed to walkers are left out, and
   so are motorways, trunk roads and cycle tracks (unless walkers may share them).
   - A road whose sidewalks are drawn as ways of their own is left out: one runs on its
     sidewalks.
   - A road said to have a sidewalk counts as a footway.
   - A road said to have none costs three times as much: it is taken only to link the rest.
3. It fetches the elevation tiles that cover the paths, kept in the cache folder, and reads the climb
   and descent of every stretch from them.

The status line shows each step, and **Stop** halts it. The window stays usable meanwhile: the work
runs on all cores but one. Once built, the network is kept on disk and read back in a moment at the
next launch.

## Asking for loops

1. Click on the map where you want to start. The start goes to the nearest path, within 500 m.
2. Choose the distance:
   - **Exact**: the length you type, within ±2 %;
   - **Range**: any length between the two you type.
3. Choose the ground:
   - **Streets**, **Either** or **Trails**: what the loops favour;
   - **Avoid asphalt**: makes paved ways cost more.
   - **Climb wished**: optional, in metres. It ranks first the loops whose climb is nearest to it; it
     doesn't force it.
4. Click **Propose loops**, or press `Enter` in a field. A click on the map plans again from there.

Up to five loops come, as different as possible from each other: two loops sharing more than 30 % of
the way are not both shown. Each loop says:

- its distance, its climb and descent;
- its share of trail, of asphalt, and of way run twice (an out-and-back);
- its seed (`#…`).

`↑` and `↓` pick a loop. The chosen one is drawn thicker on the map, with its altitude profile
below the list. Pointing at the profile points at the map.

Under the list, **Others** (`Ctrl + N`) proposes other loops from the same start, **Reverse** runs
the chosen loop the other way round, and **GPX** (`Ctrl + S`) writes it where you say. The file
carries the OpenStreetMap attribution its licence asks for.

## Changing a loop

- A click on the map, the start set, asks the loops to pass by that point. The panel then shows
  **Pass by** with the number of points; its cross takes them all away.
- Drag the start, a point, or the chosen loop's line to move them; a right click takes a point away.
- The cross on the **Start** line clears the start, the points and the loops, to begin elsewhere.

Every change can be undone. The two arrows at the top left of the map undo and redo
(`Ctrl + Z`, `Ctrl + Shift + Z`), and their tooltip names the change: "Undo: point to pass by
added". After each change, and each undo, the line under the map says what happened for three
seconds.

## How loops are found

A loop starts as a polygon of two to four points around the start. It is drawn at random from the
seed and sized for the distance asked. The paths between those points are the cheapest by A*, with
these costs:

- trails or streets cost more depending on what you chose;
- asphalt costs more if you avoid it;
- secondary and primary roads cost twice as much or more: they are taken to link paths, not to
  run along;
- difficult or faint mountain paths cost more, and alpine scrambles are left out;
- a stretch already taken costs four times more.

The polygon is then resized until the length fits. In exact mode, single points are moved to a
nearby crossing until it falls within 2 %. Out-and-backs are never added to make up the length.

The same seed from the same start, with the same settings and the same downloaded network, gives the
same loop: the loop drawn from a seed always comes first.

## Relief

The map is shaded from the elevation tiles, lit from the north-west, so valleys and ridges show.
**Flat map** turns it off. The altitudes come from Mapzen's Terrain Tiles (SRTM, Copernicus EU-DEM
and other sources), about 30 m apart in France. Climb is read every 10 m along each path and smooths
away rises under 2 m, so it reads a little lower than a barometric watch.

## What it doesn't do

- Only one region for now.
- The network is the one downloaded. A newer one comes with **Download the region** again: it
  replaces the extract once complete.
- It doesn't know whether a path is open today, a private yard crossed, or a ford deep. Look at the
  ground.
