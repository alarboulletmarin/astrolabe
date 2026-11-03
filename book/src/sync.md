# Synchronising and working offline

## What a synchronisation does

It starts on its own at launch when a key is saved, and from the round button at the bottom of the
rail or **Synchronise now** in the settings.

1. **Activities**: Astrolabe asks Garmin for your activities, newest first, and stops at the first
   one it already knows. The new list is written to disk before it replaces the one shown: if the
   write fails, nothing changes.
2. **Files**: the file of every activity not read yet is downloaded, kept on disk, and read for its
   track, curves, best efforts and heart rate zones. One at a time, with a short pause in between.

The button at the bottom of the rail shows how it goes: a progress ring while it runs, a check for
a few seconds when it went through, a red mark when it failed or stopped short. Its tooltip says
why; a click starts it again.

## When it stops

| What happens | What Astrolabe does |
|---|---|
| Garmin asks to slow down while listing | the sync fails, says *Too many requests*, and changes nothing; try again later |
| Garmin asks to slow down while downloading files | the downloads stop; the files already read are kept; the next sync resumes with the rest |
| a file can't be read | it is counted as failed and named in the settings; the others go on; it is tried again next time |
| five files fail in a row | the downloads stop, with the last error; the next sync resumes |
| you click **Stop** in the settings | the downloads stop after the file in progress |
| the network is down | the sync fails with *Network* and the reason; everything already downloaded stays |

## Offline

Everything Astrolabe has downloaded works without a network: the journal, the activities, their
curves, the records, the statistics, the form. Map tiles come from the network the first time and
are kept on disk; a place never shown before stays blank until the network is back.

Only the activities added on Garmin since the last synchronisation are missing. When the network
comes back, a click on the sync button fetches them.

## What isn't fetched again

A synchronisation stops at the first activity it already knows, so a change made later on Garmin to
an older activity (its name, its type) is not seen.
