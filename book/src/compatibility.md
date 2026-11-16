# Compatibility

| System | Status |
|---|---|
| Linux, GNOME on Wayland | first-class: this is what Astrolabe is built and used on (Arch Linux) |
| Linux, other desktops, Wayland or X11 | should work. Not tested. |
| macOS 11 or later, Windows 10 or 11 | built and tested by CI on every commit, and the downloads are built there too. Not used day to day by the author. |

## Linux

- Astrolabe draws with Vulkan: it needs a working driver (Mesa or the vendor one). See [Troubleshooting](troubleshooting.md) if the window does not open.
- It draws its own title bar, shadow and resize edges. See [Navigation](navigation.md#the-window).
- A system keyring (GNOME Keyring, KWallet, KeePassXC) holds the key; without one the app encrypts it in a file. See [Your data and backups](data.md).

## macOS and Windows

The system title bar is used. The key goes to the Keychain or the Credential Manager. The downloads are not signed: see [Install](install.md).

## Fonts

IBM Plex Sans and Plex Mono are built into the app.

## Garmin

Astrolabe reads Garmin's personal API with a key only you hold. It needs an account with activities on Garmin Connect, and never writes to it.

## Accessibility

- A screen reader cannot read the window yet: the toolkit Astrolabe is drawn with exposes no accessibility tree.
- Maps and charts answer to the pointer; not every control can be reached from the keyboard.
