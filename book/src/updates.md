# Updates

Astrolabe does not check for updates yet, and never updates itself: a new version is something you install. (Checking and updating from the app is planned: issue [#41](https://github.com/alarboulletmarin/astrolabe/issues/41).)

To know a version is out, watch the [releases](https://github.com/alarboulletmarin/astrolabe/releases) of the repository.

## Updating

Install over the old version. Your data stay where they are: they live outside the program. See [Your data and backups](data.md).

| Installed with | How to update |
|---|---|
| Arch Linux package | download the new `astrolabe-x86_64.pkg.tar.zst` and `sudo pacman -U` it |
| `make install` | `git pull`, then `make` and `sudo make install` |
| macOS `.dmg` | open the new `astrolabe-macos.dmg` and drag Astrolabe over the old one in Applications |
| Windows installer | run the new `astrolabe-windows-x86_64-setup.exe`: it replaces the old program |
| Windows zip | unzip the new one over the old folder |
| `cargo install` | `cargo install --path . --locked --force` from an updated checkout |

The links are in [Install](install.md).

## Privacy

Since nothing is checked, Astrolabe contacts nobody about versions. What it does send, and to whom, is in [Your data and backups](data.md).
