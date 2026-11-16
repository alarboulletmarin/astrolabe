#!/usr/bin/env python3
"""Write a made-up athlete's data folder, for screenshots and demos without real health data.

USAGE
    scripts/demo-data.py DIR [--days N] [--seed S] [--today YYYY-MM-DD] [--lang en|fr] [--theme light|dark]

DIR receives `astrolabe/activities.json`, `astrolabe/derived.json`, `astrolabe/settings` and a
FIT file per activity in `astrolabe/fit/`: point the app at it with XDG_DATA_HOME=DIR (Linux).
The activities, their curves, laps and efforts are invented, on routes around Annecy planned
on OpenStreetMap's paths (`demo-routes.json`); the same seed gives the same folder. The app derives the map lines, profiles and efforts from the
FIT files at launch, as it does after an upgrade.

    scripts/demo-data.py /tmp/demo --today 2026-10-25
    XDG_DATA_HOME=/tmp/demo cargo run
"""
import argparse
import bisect
import datetime as dt
import json
import math
import random
import struct
import zipfile
from pathlib import Path

EFFORTS = [400.0, 1000.0, 1609.344, 5000.0, 10_000.0, 15_000.0, 20_000.0, 21_097.5, 42_195.0, 50_000.0, 100_000.0, 1500.0, 40_000.0]
BAND = 5.0  # logic/fit.rs: BAND

# (type key, names, km range without a route, speed in km/h, mean heart rate)
SPORTS = {
    "running": ([], (5, 22), (10.5, 13.5), 148),
    "trail_running": (["Trail", "Ridge loop", "Forest trail"], (8, 28), (7, 9.5), 152),
    "road_biking": (["Ride", "Lake tour", "Hill ride"], (30, 95), (25, 31), 132),
    "lap_swimming": (["Pool"], (1.5, 3.2), (2.6, 3.2), 128),
    "walking": (["Walk"], (3, 8), (4.5, 5.5), 98),
    "hiking": (["Hike", "Summit"], (8, 18), (3.5, 4.5), 118),
}
# A run's name by its length: under 10 km, under 18, longer.
RUN_NAMES = [["Intervals", "Easy run", "Tempo"], ["Morning run", "Lakeside loop", "Tempo"], ["Long run"]]
# The usual week: weekday (0 = Monday) and sport. Some sessions are skipped; one week in five is lighter.
PLAN = [(1, "running"), (2, "lap_swimming"), (3, "running"), (5, "road_biking"), (6, "running"), (6, "walking")]

# The athlete's usual routes, planned on OpenStreetMap's paths from home in Annecy by the app's
# loop planner (`cargo test -- --ignored demo_routes`): {sport: [{"line": [[lat, lon]], "eles": [m]}]}.
ROUTES = json.loads(Path(__file__).with_name("demo-routes.json").read_text())


def ascent(alts):
    """Metres climbed, counting a rise once it passes 3 m (as a device smooths its altimeter)."""
    up, low = 0.0, alts[0] if alts else 0
    for a in alts:
        if a - low >= 3:
            up += a - low
            low = a
        low = min(low, a)
    return up


def bands(rng, seconds, mean):
    spread = rng.uniform(6, 11)
    weights = [math.exp(-(((i + 0.5) * BAND - mean) / spread) ** 2 / 2) for i in range(int(200 / BAND))]
    total = sum(weights)
    return [round(seconds * w / total, 1) for w in weights]


FIT_EPOCH = dt.datetime(1989, 12, 31)
CRC_TABLE = [0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800, 0xB401, 0x5000, 0x9C01, 0x8801, 0x4400]
# (global message, local type, [(field number, struct format, FIT base type)])
FILE_ID = (0, 0, [(0, "B", 0x00), (1, "H", 0x84), (4, "I", 0x86)])
RECORD = (20, 1, [(253, "I", 0x86), (0, "i", 0x85), (1, "i", 0x85), (2, "H", 0x84), (3, "B", 0x02), (5, "I", 0x86), (6, "H", 0x84)])
LAP = (19, 2, [(253, "I", 0x86), (2, "I", 0x86), (7, "I", 0x86), (8, "I", 0x86), (9, "I", 0x86), (15, "B", 0x02), (21, "H", 0x84)])
INVALID = {"i": 0x7FFFFFFF, "I": 0xFFFFFFFF, "H": 0xFFFF, "B": 0xFF}


def crc16(data):
    crc = 0
    for byte in data:
        for nibble in (byte & 0xF, byte >> 4):
            tmp = CRC_TABLE[crc & 0xF]
            crc = (crc >> 4) & 0x0FFF
            crc ^= tmp ^ CRC_TABLE[nibble]
    return crc


def fit_file(start, samples, laps):
    """A FIT activity: `samples` are (seconds, lat, lon, alt, hr, metres, m/s), lat and lon None
    without GPS; `laps` are (start s, seconds, metres, mean hr, ascent)."""
    out = bytearray()

    def define(msg):
        num, local, fields = msg
        out.extend(struct.pack("<BBBHB", 0x40 | local, 0, 0, num, len(fields)))
        for n, fmt, base in fields:
            out.extend(struct.pack("<BBB", n, struct.calcsize(fmt), base))

    def write(msg, values):
        _, local, fields = msg
        out.append(local)
        for (_, fmt, _), v in zip(fields, values):
            out.extend(struct.pack("<" + fmt, INVALID[fmt] if v is None else v))

    t0 = int((start - FIT_EPOCH).total_seconds())
    semi = 2**31 / 180
    define(FILE_ID)
    write(FILE_ID, [4, 255, t0])
    define(RECORD)
    for t, lat, lon, alt, hr, d, v in samples:
        write(RECORD, [t0 + round(t), None if lat is None else round(lat * semi), None if lon is None else round(lon * semi),
                       round((alt + 500) * 5), round(hr), round(d * 100), round(v * 1000)])
    define(LAP)
    for s, secs, metres, hr, ascent in laps:
        write(LAP, [t0 + round(s + secs), t0 + round(s), round(secs * 1000), round(secs * 1000), round(metres * 100), round(hr), round(ascent)])
    head = struct.pack("<BBHI4s", 14, 0x20, 2132, len(out), b".FIT")
    head += struct.pack("<H", crc16(head))
    body = head + bytes(out)
    return body + struct.pack("<H", crc16(body))


def curves(rng, key, name, line, eles, km, seconds, mean_hr):
    """Samples every 3 s along `line`: slower uphill, the heart following the effort with a lag,
    warming up over the first minutes. Returns the samples and the laps."""
    total = km * 1000
    if line:
        steps = [fit_metres(a, b) for a, b in zip(line, line[1:])]
        scale = total / sum(steps)
        cum = [0.0]
        for m in steps:
            cum.append(cum[-1] + m * scale)
    # The route's own altitudes, between its points; flat in a pool.
    known = [e for e in eles if e is not None] or [450]
    eles = [known[0] if e is None else e for e in eles] or [450, 450]
    eles = [e if e is not None else eles[i - 1] for i, e in enumerate(eles)]

    def alt(d):
        if not line:
            return 450
        j = min(bisect.bisect_right(cum, d), len(cum) - 1)
        f = (d - cum[j - 1]) / max(cum[j] - cum[j - 1], 1e-9) if j else 0
        return eles[max(j - 1, 0)] + (eles[j] - eles[max(j - 1, 0)]) * f

    def grade(d):
        # Over 160 m: the altitudes of the route come every 15 m, in whole metres.
        a, b = max(d - 80, 0), min(d + 80, total)
        return (alt(b) - alt(a)) / max(b - a, 1)

    steep = {"road_biking": 8, "lap_swimming": 0, "running": 4}.get(key, 6)
    # Intervals: 400 m fast, 200 m easy, after a 2 km warm-up and before a 1.5 km cool-down.
    fast = lambda d: name == "Intervals" and 2000 < d < total - 1500 and (d - 2000) % 600 < 400

    def weight(d):
        # Downhill helps less than uphill costs: a fifth at most on foot, two fifths on a bike.
        return min(3.0, max(0.6 if key == "road_biking" else 0.82, 1 + steep * grade(d))) * (0.8 if fast(d) else 1.12 if name == "Intervals" else 1)

    mean_w = sum(weight(total * i / 400) for i in range(400)) / 400
    v0 = total / seconds * mean_w
    pos, i, t, d, hr = None, 0, 0.0, 0.0, mean_hr - 35
    samples, wobble = [], 0.0
    while d < total:
        if line:
            while i < len(cum) - 2 and cum[i + 1] < d:
                i += 1
            f = (d - cum[i]) / max(cum[i + 1] - cum[i], 1e-9)
            pos = [line[i][k] + (line[i + 1][k] - line[i][k]) * f for k in (0, 1)]
        wobble = 0.9 * wobble + rng.gauss(0, 0.02)
        v = v0 / weight(d) * (1 + wobble)
        target = mean_hr + max(-12, min(12, 100 * grade(d))) - max(0, 25 * (1 - t / 600)) + (10 if fast(d) else 0)
        hr += (target - hr) * 0.06 + rng.gauss(0, 0.4)
        samples.append((t, pos and pos[0], pos and pos[1], alt(d) + rng.gauss(0, 0.3), hr, d, v))
        t += 3
        d += v * 3
    samples.append((t, line[-1][0] if line else None, line[-1][1] if line else None, alt(total), hr, total, v))
    every = {"road_biking": 5000, "lap_swimming": 500}.get(key, 1000)
    laps, lap = [], [samples[0]]
    for s in samples[1:]:
        lap.append(s)
        if s[5] - lap[0][5] >= every or s is samples[-1]:
            ascent = sum(max(0, b[3] - a[3]) for a, b in zip(lap[::10], lap[10::10]))
            laps.append((lap[0][0], s[0] - lap[0][0], s[5] - lap[0][5], sum(x[4] for x in lap) / len(lap), ascent))
            lap = [s]
    return samples, laps


def fit_metres(a, b):
    lat = math.radians((a[0] + b[0]) / 2)
    return math.hypot((b[0] - a[0]) * 111_320, (b[1] - a[1]) * 111_320 * math.cos(lat))


def outing(rng, aid, day, key, fitness):
    names, km_range, speed_range, hr = SPORTS[key]
    # A usual route, ridden a little differently each time: the GPS wanders by a few metres.
    if key in ROUTES:
        chosen = rng.choice(ROUTES[key])
        line, eles = chosen["line"], chosen["eles"]
        km = sum(fit_metres(a, b) for a, b in zip(line, line[1:])) / 1000 * rng.uniform(0.99, 1.01)
        drift, wander = [0.0, 0.0], []
        for lat, lon in line:
            drift = [0.85 * x + rng.gauss(0, 0.000012) for x in drift]
            wander.append([round(lat + drift[0], 6), round(lon + drift[1] * 1.4, 6)])
        line = wander
    else:
        line, eles, km = [], [], rng.uniform(*km_range)
    climb = ascent([e for e in eles if e is not None])
    speed = rng.uniform(*speed_range) * fitness
    if key == "road_biking":
        # A hilly ride goes slower: 30 m of climb per km takes a quarter off.
        speed /= 1 + climb / km / 100
    seconds = km / speed * 3600
    mean_hr = hr + rng.uniform(-6, 6)
    hour = rng.choice([7, 7, 8, 12, 18, 18, 19])
    start = dt.datetime.combine(day, dt.time(hour, rng.randrange(0, 60, 5)))
    act = {
        "activityId": aid,
        "activityName": rng.choice(RUN_NAMES[(km >= 10) + (km >= 18)] if key == "running" else names),
        "startTimeLocal": start.strftime("%Y-%m-%d %H:%M:%S"),
        "activityType": {"typeKey": key},
        "distance": round(km * 1000, 1),
        "duration": round(seconds, 1),
        "elevationGain": round(climb) if climb else None,
        "averageHR": round(mean_hr),
        "calories": round(seconds / 60 * rng.uniform(9, 13)),
    }
    efforts = []
    if key != "walking":
        for d in EFFORTS:
            if d <= km * 1000:
                # Shorter efforts go a little faster than the whole outing.
                pace = seconds / (km * 1000) * (0.88 + 0.12 * math.log10(1 + 9 * d / (km * 1000)))
                efforts.append({"metres": d, "seconds": round(d * pace, 1)})
    derived = {
        "line": [] if key == "lap_swimming" else line,
        "efforts": efforts,
        "profile": [],
        "hr_bands": bands(rng, seconds, mean_hr),
        "max_hr": round(mean_hr + rng.uniform(12, 25)),
        # Older than the app: derived again from the FIT file at launch.
        "version": 0,
    }
    samples, laps = curves(rng, key, act["activityName"], line, eles, km, seconds, mean_hr)
    act["duration"] = round(samples[-1][0], 1)
    act["averageHR"] = round(sum(x[4] for x in samples) / len(samples))
    return act, derived, fit_file(start, samples, laps)


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("dir", type=Path)
    p.add_argument("--days", type=int, default=730)
    p.add_argument("--seed", type=int, default=7)
    p.add_argument("--today", type=dt.date.fromisoformat, default=dt.date.today())
    p.add_argument("--lang", choices=["en", "fr"], default="en")
    p.add_argument("--theme", choices=["light", "dark"], default="light")
    a = p.parse_args()

    rng = random.Random(a.seed)
    acts, derived, fits = [], {}, {}
    aid = 10_000_000_000
    for back in range(a.days, -1, -1):
        day = a.today - dt.timedelta(days=back)
        # Fitter as the months go, a break in August.
        fitness = 0.93 + 0.1 * (1 - back / max(a.days, 1))
        if day.month == 8 and 5 <= day.day <= 18:
            continue
        week = day.isocalendar()[1]
        light = week % 5 == 0
        for weekday, key in PLAN:
            if day.weekday() != weekday:
                continue
            # Every other Sunday run is a trail.
            if key == "running" and weekday == 6 and week % 2:
                key = "trail_running"
            # The last days keep every session, so the current week is never empty.
            if back > 6 and (rng.random() < (0.45 if light else 0.15) or (key == "walking" and rng.random() < 0.5)):
                continue
            aid += rng.randint(1, 40)
            act, d, fit = outing(rng, aid, day, key, fitness)
            acts.append(act)
            derived[str(aid)] = d
            fits[aid] = fit
        # A run on the last day when the usual week has nothing that day (a Monday).
        if back == 0 and day.weekday() not in {w for w, _ in PLAN}:
            aid += rng.randint(1, 40)
            act, d, fit = outing(rng, aid, day, "running", fitness)
            act["startTimeLocal"] = act["startTimeLocal"][:11] + "07:10:00"
            acts.append(act)
            derived[str(aid)] = d
            fits[aid] = fit
    # Hikes in summer.
    for year in sorted({a.today.year - 1, a.today.year}):
        for day in (dt.date(year, 7, 14), dt.date(year, 8, 10)):
            if day <= a.today:
                aid += 7
                act, d, fit = outing(rng, aid, day, "hiking", 1.0)
                acts.append(act)
                derived[str(aid)] = d
                fits[aid] = fit
    acts.sort(key=lambda x: x["startTimeLocal"], reverse=True)

    out = a.dir / "astrolabe"
    out.mkdir(parents=True, exist_ok=True)
    (out / "activities.json").write_text(json.dumps(acts))
    (out / "derived.json").write_text(json.dumps(derived))
    (out / "fit").mkdir(exist_ok=True)
    for aid, fit in fits.items():
        # Garmin serves the file in a ZIP; the app keeps it so.
        with zipfile.ZipFile(out / "fit" / f"{aid}.zip", "w", zipfile.ZIP_DEFLATED) as z:
            z.writestr(f"{aid}_ACTIVITY.fit", fit)
    (out / "settings").write_text(f"language={a.lang}\ntheme={a.theme}\n")
    print(f"{len(acts)} made-up activities in {out}")


if __name__ == "__main__":
    main()
