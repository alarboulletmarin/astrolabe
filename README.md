# Astrolabe

A desktop journal of your sport activities, read from your Garmin account. Your activities, their GPS tracks and their curves are kept on your machine, and the app opens instantly, online or not. Written in Rust with [GPUI](https://www.gpui.rs).

An astrolabe is the old instrument of astronomers and sailors: you take the height of a star, read a scale, and know where you stand. Astrolabe does the same with your activities. It reads them and shows where you stand, without telling you what to do.

> Early days: Astrolabe runs from source on Linux and has no release yet. The interface is in French.

## Features

- **Journal**: every activity, week by week, with the week's totals, a search and a filter by sport (running, trail, cycling, swimming, walking, hiking).
- **Activity sheet**: the map of the track, the pace, heart rate and elevation curves (pointing at one points at the others and on the map), the splits and the laps.
- **Records**: best efforts on the road (1 km, 5 km, 10 km, half marathon, marathon), trail apart, cycling and swimming, for one year or all of them.
- **Statistics**: pick the measure, the grouping (day, week, month, year), the period and the sports, and compare the running totals of several years.
- **Heatmap**: all your tracks on one map, darker where you go most. A click shows the activities that pass there.
- Light and dark, following your system.

## Your data

- **Garmin personal API key**: create it on the Garmin Connect website (*Account settings › Personal API key*) and paste it in the settings. The app checks it with Garmin, then hands it to your system's keyring (GNOME Keyring, KWallet, macOS Keychain, Windows Credential Manager). Where the system has no keyring, the app encrypts it itself (XChaCha20-Poly1305) in a file only you can read, with the key that opens it kept in a separate file in your config folder. It is never written in the clear or logged.
- **Sync**: at launch, the list of your activities, then the file of each one (GPS track, heart rate, laps), downloaded in the background and kept on disk. A limit reached at Garmin pauses the download, which resumes at the next sync.
- **On your machine**: activities and their files live in `~/.local/share/astrolabe`, readable by you only. Nothing goes anywhere else.
- **Maps**: tiles come from OpenStreetMap, are cached in `~/.cache/astrolabe`, and are asked for without anything that identifies you. Another tile server can be set in the settings.

## Build

Astrolabe needs a recent stable Rust ([rustup](https://rustup.rs)) and, on Linux, the libraries GPUI draws with.

Ubuntu, Linux Mint, Debian:

```bash
sudo apt install build-essential pkg-config libfontconfig-dev libfreetype-dev \
  libwayland-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libvulkan1
```

Fedora:

```bash
sudo dnf install gcc pkgconf-pkg-config fontconfig-devel freetype-devel \
  wayland-devel libxcb-devel libxkbcommon-devel libxkbcommon-x11-devel vulkan-loader
```

Then:

```bash
git clone https://github.com/alarboulletmarin/astrolabe.git
cd astrolabe
cargo run --release
```

On Linux the system keyring is a Secret Service (GNOME Keyring, KWallet or KeePassXC); without one, the app falls back to its own encrypted file, with nothing to install.

## Development

```bash
cargo run          # debug build
cargo test         # unit tests, FIT parsing on real files in tests/fixtures
```

Work happens on `develop`; `main` holds what is released.

## License

[MIT](LICENSE). The IBM Plex fonts are under the [SIL Open Font License](assets/fonts/LICENSE-IBM-Plex.txt). Map data © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors.
