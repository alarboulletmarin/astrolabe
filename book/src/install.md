# Install

| Your system | Go to |
|---|---|
| Arch Linux | [Arch](#arch-linux) |
| Another Linux distribution | [Other Linux](#other-linux-distributions) |
| macOS | [macOS](#macos) |
| Windows | [Windows](#windows) |

Every release comes with ready-made downloads for Arch, macOS and Windows; elsewhere Astrolabe is compiled on your machine, which takes a few minutes the first time. Compiling needs a recent stable Rust: install it with [rustup](https://rustup.rs).

## Arch Linux

The package is not signed, so download it first: given a URL, pacman looks for a `.sig` file and warns when there is none.

```bash
curl -LO https://github.com/alarboulletmarin/astrolabe/releases/latest/download/astrolabe-x86_64.pkg.tar.zst
sudo pacman -U ./astrolabe-x86_64.pkg.tar.zst
```

Or build the latest tagged release yourself with `makepkg -si` from a checkout.

## Other Linux distributions

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

## macOS

Download [`astrolabe-macos.dmg`](https://github.com/alarboulletmarin/astrolabe/releases/latest/download/astrolabe-macos.dmg) (Apple Silicon and Intel, macOS 11 or later), open it and drag Astrolabe to Applications.

The app is not notarized (that needs a paid Apple Developer account), so macOS blocks the first launch. Open **System Settings**, **Privacy & Security**, scroll down and click **Open Anyway**. Or run this once in a terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Astrolabe.app
```

To compile it yourself instead you need Xcode (the full app, for the Metal shader compiler) and Rust, then `cargo install --path . --locked` from a checkout. This installs the `astrolabe` command in `~/.cargo/bin`, without the `.app` bundle.

## Windows

Download [`astrolabe-windows-x86_64-setup.exe`](https://github.com/alarboulletmarin/astrolabe/releases/latest/download/astrolabe-windows-x86_64-setup.exe) and run it (Windows 10 or 11, 64-bit). It installs for your user only, with no administrator rights, and adds a Start menu entry and an uninstaller. If you would rather install nothing, take [`astrolabe-windows-x86_64.zip`](https://github.com/alarboulletmarin/astrolabe/releases/latest/download/astrolabe-windows-x86_64.zip), unzip it anywhere and run `astrolabe.exe`.

The files are not signed (a code-signing certificate costs a few hundred euros a year), so SmartScreen may show "Windows protected your PC": click **More info**, then **Run anyway**.

To compile it yourself instead you need the Visual Studio C++ build tools with the Windows SDK, and Rust (MSVC toolchain), then `cargo install --path . --locked` from a checkout.

## Upgrading

Download the new package, `.dmg` or installer and install it over the old one: your data stay where they are. With `make install`: `git pull`, then `make` and `sudo make install`.

The system keyring is a Secret Service on Linux (GNOME Keyring, KWallet or KeePassXC), the Keychain on macOS and the Credential Manager on Windows. Where there is none, the app falls back to its own encrypted file, with nothing to install.
