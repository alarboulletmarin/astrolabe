# Your data and backups

Astrolabe keeps everything on your machine and sends nothing anywhere but Garmin (to read your
activities) and the map tile server (to draw the maps).

## Where things are

| | Linux | macOS | Windows |
|---|---|---|---|
| activities, files, settings | `~/.local/share/astrolabe` | `~/Library/Application Support/astrolabe` | `%LOCALAPPDATA%\astrolabe` |
| the key that opens `key.enc` | `~/.config/astrolabe` | `~/Library/Application Support/astrolabe` | `%APPDATA%\astrolabe` |
| map tiles | `~/.cache/astrolabe` | `~/Library/Caches/astrolabe` | `%LOCALAPPDATA%\astrolabe` |

On Linux, `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` move them, as for any application.
The settings page shows the data folder of your machine.

In the data folder:

| File | What |
|---|---|
| `activities.json` | the list of your activities, as Garmin gives it |
| `derived.json` | what was read from each file: the simplified track, best efforts, profile, heart rate zones |
| `fit/<id>.zip` | the original file of each activity |
| `settings` | tiles, theme, language, journal columns |
| `key.enc` | your Garmin key, encrypted, only when your system has no keyring |

Health data (activities, files, the key) are readable by you only. Each file is written in full to a
temporary file, then put in place, so a crash or a full disk never leaves half a file.

## Your Garmin key

It is kept in your system's keyring. Where there is none, Astrolabe encrypts it with
XChaCha20-Poly1305 in `key.enc`, in the data folder, and keeps the key that opens it in
`master.key`, in the config folder: one file alone is not enough to read it. The key is never
written in the clear, nor in a log.

**Remove the key** in the settings deletes it from the keyring or the file. To revoke it for good,
delete it on the Garmin Connect website too.

## Backups

Everything can be downloaded again from Garmin, but the files take a while. To keep them, copy the
data folder; the tiles folder can be left out.

To move to another machine, copy the data folder to the same place there. The key comes along
only if it was in `key.enc` and you copy `master.key` too; otherwise paste it again in the settings
(or create a new one on Garmin Connect).

## A damaged file

If `activities.json` can't be read when the app opens, Astrolabe does not throw it away: it renames
it `activities.damaged-<number>.json`, says so in red in the status line, and does not synchronise
on its own. Click the sync button to download the list again. The renamed file stays where it was,
for you to keep or delete.
