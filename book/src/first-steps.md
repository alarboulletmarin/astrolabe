# First steps

## 1. Create a Garmin key

Astrolabe reads your activities through Garmin's personal API, with a key that only you hold.

1. Sign in to the Garmin Connect website.
2. Open **Account settings › Personal API key** and create a key.
3. Copy it right away: Garmin shows it only once.

The key gives read access to your data; Astrolabe never writes anything to your Garmin account.

## 2. Paste it in the settings

On the first launch, Astrolabe opens its settings. Paste the key in **Personal API key** and click
**Check and save**. The app asks Garmin whether the key works before keeping it, then hands it to
your system's keyring (GNOME Keyring, KWallet, macOS Keychain, Windows Credential Manager). Where
there is no keyring, the settings say so and the app encrypts the key itself, in a file only you
can read. See [Your data and backups](data.md).

## 3. The first synchronisation

Once the key is saved, the synchronisation starts on its own, in two steps:

1. **Activities**: the list of all your activities, a hundred at a time. The status line, bottom
   right, counts them as they arrive. It takes a few seconds.
2. **Files**: the file of each activity, newest first, one at a time. With years of activities,
   this takes a while; the app is usable all along, and each activity gets its map and curves as
   its file arrives.

Garmin limits how fast its API can be asked. If it asks to slow down, the downloads stop and
resume at the next synchronisation. Nothing is lost. See [Synchronising](sync.md).

## 4. Next launches

Astrolabe opens on what it already has, then synchronises in the background: only the activities
added since the last time are asked for. Without a network, everything already downloaded stays
available.
