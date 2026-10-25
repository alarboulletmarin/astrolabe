# Astrolabe

A desktop journal of your sport activities, read from your Garmin account. Your activities, their GPS tracks and their curves are kept on your machine, and the app opens instantly, online or not. Written in Rust with [GPUI](https://www.gpui.rs).

An astrolabe is the old instrument of astronomers and sailors: you take the height of a star, read a scale, and know where you stand. Astrolabe does the same with your activities. It reads them and shows where you stand, without telling you what to do.

[Website](https://alarboulletmarin.github.io/astrolabe/) · [Install](#install) · [Releases](https://github.com/alarboulletmarin/astrolabe/releases) · [Changelog](CHANGELOG.md)

> Early days: the interface follows your system's language, English or French, and the downloads are not signed (see [macOS](#macos) and [Windows](#windows)).

## Features

- **Today**: the home page. This week against your median week, the run of weeks with an outing, the year so far against the same date a year before; six months week by week (time across sports, stacked by sport; distance for one sport), up to a year day by day, the latest outings and the records of the last two weeks.
- **Journal**: every activity, week by week, with the week's totals (kilometres sport by sport), a search and a filter by sport (running, trail, cycling, swimming, walking, hiking), and the columns you want: distance, time, pace, the gap to your usual pace on like outings, elevation, climb per kilometre, heart rate, calories, rank, the time in each heart rate zone and the altitude profile, which takes what the row has left. Columns that do not fit fold away, the least useful first.
- **Activity sheet**: the map of the track, the pace, heart rate and elevation curves (pointing at one points at the others and on the map), the splits and the laps.
- **Records**: best efforts on the road (1 km, 5 km, 10 km, half marathon, marathon), trail apart, cycling and swimming, for one year or all of them.
- **Statistics**: pick the measure, the grouping (day, week, month, year), the period and the sports, and compare the running totals of several years.
- **Heatmap**: all your tracks on one map, darker where you go most. A click shows the activities that pass there.
- Light and dark, following your system.

## Your data

- **Garmin personal API key**: create it on the Garmin Connect website (*Account settings › Personal API key*) and paste it in the settings. The app checks it with Garmin, then hands it to your system's keyring (GNOME Keyring, KWallet, macOS Keychain, Windows Credential Manager). Where the system has no keyring, the app encrypts it itself (XChaCha20-Poly1305) in a file only you can read, with the key that opens it kept in a separate file in your config folder. It is never written in the clear or logged.
- **Sync**: at launch, the list of your activities, then the file of each one (GPS track, heart rate, laps), downloaded in the background and kept on disk. A limit reached at Garmin pauses the download, which resumes at the next sync.
- **On your machine**: activities and their files live in the app's data folder (`~/.local/share/astrolabe` on Linux), readable by you only. Nothing goes anywhere else.
- **Maps**: tiles come from OpenStreetMap, are cached in `~/.cache/astrolabe`, and are asked for without anything that identifies you. Another tile server can be set in the settings.

## Install

| Your system | Go to |
|---|---|
| Arch Linux | [Arch](#arch-linux) |
| Another Linux distribution | [Other Linux](#other-linux-distributions) |
| macOS | [macOS](#macos) |
| Windows | [Windows](#windows) |

Every release comes with ready-made downloads for Arch, macOS and Windows; elsewhere Astrolabe is compiled on your machine, which takes a few minutes the first time. Compiling needs a recent stable Rust: install it with [rustup](https://rustup.rs).

### Arch Linux

The package is not signed, so download it first: given a URL, pacman looks for a `.sig` file and warns when there is none.

```bash
curl -LO https://github.com/alarboulletmarin/astrolabe/releases/latest/download/astrolabe-x86_64.pkg.tar.zst
sudo pacman -U ./astrolabe-x86_64.pkg.tar.zst
```

Or build the latest tagged release yourself with `makepkg -si` from a checkout.

### Other Linux distributions

You need the development files of fontconfig, freetype, Wayland, libxcb and libxkbcommon (with its X11 part), a Vulkan driver for your GPU, and Rust.

Ubuntu, Linux Mint, Debian:

```bash
sudo apt install build-essential pkg-config libfontconfig-dev libfreetype-dev \
  libwayland-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libvulkan1
```

Fedora (package names not tested):

```bash
sudo dnf install gcc pkgconf-pkg-config fontconfig-devel freetype-devel \
  wayland-devel libxcb-devel libxkbcommon-devel libxkbcommon-x11-devel vulkan-loader
```

Then:

```bash
git clone https://github.com/alarboulletmarin/astrolabe.git
cd astrolabe
make                    # cargo build --release --locked
sudo make install       # PREFIX=/usr/local by default, PREFIX=/usr for a system-wide install
```

`make install` adds the `astrolabe` command, an entry in your application menu and its icon. `sudo make uninstall` removes them. To just try it: `cargo run --release`.

### macOS

Download [`astrolabe-macos.dmg`](https://github.com/alarboulletmarin/astrolabe/releases/latest/download/astrolabe-macos.dmg) (Apple Silicon and Intel, macOS 11 or later), open it and drag Astrolabe to Applications.

The app is not notarized (that needs a paid Apple Developer account), so macOS blocks the first launch. Open **System Settings**, **Privacy & Security**, scroll down and click **Open Anyway**. Or run this once in a terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Astrolabe.app
```

To compile it yourself instead you need Xcode (the full app, for the Metal shader compiler) and Rust, then `cargo install --path . --locked` from a checkout. This installs the `astrolabe` command in `~/.cargo/bin`, without the `.app` bundle.

### Windows

Download [`astrolabe-windows-x86_64-setup.exe`](https://github.com/alarboulletmarin/astrolabe/releases/latest/download/astrolabe-windows-x86_64-setup.exe) and run it (Windows 10 or 11, 64-bit). It installs for your user only, with no administrator rights, and adds a Start menu entry and an uninstaller. If you would rather install nothing, take [`astrolabe-windows-x86_64.zip`](https://github.com/alarboulletmarin/astrolabe/releases/latest/download/astrolabe-windows-x86_64.zip), unzip it anywhere and run `astrolabe.exe`.

The files are not signed (a code-signing certificate costs a few hundred euros a year), so SmartScreen may show "Windows protected your PC": click **More info**, then **Run anyway**.

To compile it yourself instead you need the Visual Studio C++ build tools with the Windows SDK, and Rust (MSVC toolchain), then `cargo install --path . --locked` from a checkout.

### Upgrading

Download the new package, `.dmg` or installer and install it over the old one: your data stay where they are. With `make install`: `git pull`, then `make` and `sudo make install`.

The system keyring is a Secret Service on Linux (GNOME Keyring, KWallet or KeePassXC), the Keychain on macOS and the Credential Manager on Windows. Where there is none, the app falls back to its own encrypted file, with nothing to install.

## Development

```bash
cargo run          # debug build
cargo test --locked  # unit tests, FIT parsing on files in tests/fixtures
```

Work happens on `develop`; `main` holds what is released. See [CONTRIBUTING.md](CONTRIBUTING.md) (which also describes how a release is made) and [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). The IBM Plex fonts are under the [SIL Open Font License](assets/fonts/LICENSE-IBM-Plex.txt). Map data © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors.
