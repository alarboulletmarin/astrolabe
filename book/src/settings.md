# Settings

The last item of the rail, **Settings**. Astrolabe opens on it when no key is saved and no activity is kept yet.

<!-- screenshot: the settings page -->

On a wide window the page has two columns: the account and the maps on the left, the data and
the appearance on the right. On a narrow one they follow each other.

## Garmin account

- **Personal API key** says where the key is kept: the system keyring, "encrypted by the app"
  where there is none (said once, here, with why), or "none".
- Without a key, its field shows: paste the key and click **Check and save**. It is "Checked with
  Garmin before being saved."
- With one, **Change the key** shows the field again, with **Cancel** and **Remove the key**.
- **Synchronisation** says where it stands, with the tracks read and **Synchronise**. See
  [Synchronising](sync.md).

How to get a key: [First steps](first-steps.md). Where it goes: [Your data and backups](data.md).

## Maps

- The regions downloaded for the loops, each with its size and date; the one used says so.
  **Use**, **Update** (or **Download** when it isn't there) and **Remove**, which asks first.
- **Add a region**: type a name, then **Add** one of the regions found. See
  [Loops › Other regions](loops.md#other-regions).
- **Tile address ({z}, {x}, {y})**: OpenStreetMap's tiles by default, kept on disk; another
  server's address, with `{z}`, `{x}`, `{y}`, and **Save** keeps it.

## Data received

How many activities since when, how many FIT files are read, and the folder they are kept in (its
whole path in a tooltip when it is cut). A box says what Astrolabe doesn't read: what the key gives
access to but the app doesn't ask for, and what the personal API doesn't provide.

## Appearance

- **Theme**: **System**, **Light** or **Dark**.
- **Language**: **System**, English or Français.
