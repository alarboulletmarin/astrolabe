# Security policy

## Supported versions

Only the latest release receives fixes. Please check that you run it before reporting.

## Reporting a vulnerability

Do not open a public issue. Use the [private report form](https://github.com/alarboulletmarin/astrolabe/security/advisories/new) (*Security* tab, *Report a vulnerability*): only the maintainer sees it.

Please include the version, the operating system, and the steps or the file that trigger the problem. **Never paste your Garmin personal API key, a token, or a real activity file in a report**: describe the problem, or send a file made from demonstration data.

You can expect a first answer within a week. A confirmed vulnerability is fixed in a new release, and the advisory is published with it, crediting you unless you prefer otherwise.

## What matters most

Astrolabe handles a personal API key and health data (heart rate, GPS tracks, places you go), so these come first:

- **The Garmin API key**: it must only live in the system keyring, or in the encrypted file (`key.enc`, sealed by `master.key`, both readable by the user alone). Any path that writes it in the clear, logs it, shows it in an error message, or sends it anywhere but Garmin's personal API is a vulnerability.
- **Activity data on disk**: the cache, the downloaded FIT files and the derived tracks are written readable by the user alone, atomically, and are never deleted by the app. A path that makes them readable by others, or that writes outside the app's folders, matters.
- **What leaves the machine**: requests go to Garmin's personal API and to the map tile server only; tile requests carry nothing that identifies you. Anything else that talks to the network is a vulnerability.
- **Files the app parses**: the FIT files downloaded from Garmin (inside zip archives) and the data files of the local store. A crafted file that crashes the app, exhausts memory or writes elsewhere is in scope.

Out of scope: an attacker who already has your user account on the machine can read the files the app can read.
