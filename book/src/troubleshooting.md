# Troubleshooting

Errors appear in red in the status line, bottom right, and in the tooltip of the sync button.
Problems with the files of activities are listed in the settings, under the synchronisation.

## The key

| Message | What to do |
|---|---|
| *Garmin refused the key: expired or revoked?* | create a new key on Garmin Connect and paste it in the settings |
| *This text does not look like a Garmin API key.* | copy the whole key again; it has no spaces |
| *The system key store is unavailable* | nothing: the key is encrypted by the app instead, in a file only you can read |
| *The key saved in the app is unreadable: remove it and add it again.* | `key.enc` and `master.key` no longer match (one was copied without the other): **Remove the key**, then paste it again |

## Synchronising

| Message | What to do |
|---|---|
| *Too many requests: Garmin asks to wait a little.* | wait a few minutes, then click the sync button |
| *Garmin asks to slow down: the rest will come at the next synchronisation.* | nothing: the files already read are kept |
| *Network: …* | check the connection; everything already downloaded stays available |
| *Garmin answered 500* (or another number) | Garmin's side; try again later |
| *Unexpected answer from Garmin* | try again; if it lasts, [open an issue](https://github.com/alarboulletmarin/astrolabe/issues) with the message, never your key |
| *Stopped after 5 failures in a row* | read the last failure; the next sync resumes |

## Files on disk

| Message | What to do |
|---|---|
| *Activity cache unreadable (…), kept as activities.damaged-….json* | click the sync button to download the list again; see [A damaged file](data.md#a-damaged-file) |
| *Activity cache unreadable : …* (without *kept as*) | the data folder itself can't be read: check that it exists and belongs to you |
| *Cache not saved*, *Tracks not saved*, *Track not saved*, *Settings not saved* | the disk is full or the folder can't be written: free some space or check its permissions, then synchronise again |

## The window

- **On Linux, the window doesn't open**: Astrolabe draws with Vulkan; check that a Vulkan driver
  for your graphics card is installed (see [Install](install.md#other-linux-distributions)).
- **macOS refuses to open it**: see [macOS](install.md#macos) in the install guide.
- **Windows shows "Windows protected your PC"**: see [Windows](install.md#windows).

## Reporting a problem

[Open an issue](https://github.com/alarboulletmarin/astrolabe/issues) with the version, the system and the steps that lead to the problem. Never include your key, a token, or
real activity data (tracks, places, health figures). For a security problem, follow the
[security policy](https://github.com/alarboulletmarin/astrolabe/blob/develop/SECURITY.md).
