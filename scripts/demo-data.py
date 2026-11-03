#!/usr/bin/env python3
"""Write a made-up athlete's data folder, for screenshots and demos without real health data.

USAGE
    scripts/demo-data.py DIR [--days N] [--seed S] [--today YYYY-MM-DD] [--lang en|fr] [--theme light|dark]

DIR receives `astrolabe/activities.json`, `astrolabe/derived.json` and `astrolabe/settings`:
point the app at it with XDG_DATA_HOME=DIR (Linux). The activities, their tracks (loops around
Annecy), heart rates and efforts are invented; the same seed gives the same folder. No file of
activity is written, so an activity's page shows its map but asks Garmin for its curves.

    scripts/demo-data.py /tmp/demo --today 2026-10-25
    XDG_DATA_HOME=/tmp/demo cargo run
"""
import argparse
import datetime as dt
import json
import math
import random
from pathlib import Path

CENTRE = (45.899, 6.129)
EFFORTS = [400.0, 1000.0, 1609.344, 5000.0, 10_000.0, 15_000.0, 20_000.0, 21_097.5, 42_195.0, 50_000.0, 100_000.0, 1500.0, 40_000.0]
VERSION = 2  # logic/fit.rs: VERSION
PROFILE = 48  # logic/fit.rs: PROFILE
BAND = 5.0  # logic/fit.rs: BAND

# (type key, names, km range, speed in km/h, climb per km, mean heart rate)
SPORTS = {
    "running": (["Morning run", "Easy run", "Tempo", "Long run", "Intervals", "Lakeside loop"], (5, 22), (10.5, 13.5), (5, 20), 148),
    "trail_running": (["Trail", "Ridge loop", "Forest trail"], (8, 28), (7, 9.5), (35, 70), 152),
    "road_biking": (["Ride", "Lake tour", "Hill ride"], (30, 95), (25, 31), (8, 20), 132),
    "lap_swimming": (["Pool"], (1.5, 3.2), (2.6, 3.2), (0, 0), 128),
    "walking": (["Walk"], (3, 8), (4.5, 5.5), (2, 10), 98),
    "hiking": (["Hike", "Summit"], (8, 18), (3.5, 4.5), (50, 90), 118),
}
# The usual week: weekday (0 = Monday) and sport. Some sessions are skipped; one week in five is lighter.
PLAN = [(1, "running"), (2, "lap_swimming"), (3, "running"), (5, "road_biking"), (6, "running"), (6, "walking")]

def loop(rng, km):
    """A closed, wobbly loop of about `km` that starts and ends at home, a point every 50 m, as [lat, lon]."""
    radius = km * 1000 / (2 * math.pi) / 1.15
    phi = rng.uniform(0, 2 * math.pi)
    phase = [rng.uniform(0, 2 * math.pi) for _ in range(3)]
    n = max(8, int(km * 20))
    metres_lat = 111_320
    metres_lon = metres_lat * math.cos(math.radians(CENTRE[0]))
    out = []
    for i in range(n + 1):
        a = 2 * math.pi * i / n
        wobble = 1 + 0.25 * math.sin(2 * a + phase[0]) * math.sin(a / 2) + 0.1 * math.sin(5 * a + phase[1]) * math.sin(a / 2)
        r = radius * wobble
        # The circle's centre sits `radius` from home, so the loop's first point is home.
        dy = radius * math.sin(phi) + r * math.sin(phi + math.pi + a)
        dx = radius * math.cos(phi) + r * math.cos(phi + math.pi + a)
        out.append([CENTRE[0] + dy / metres_lat, CENTRE[1] + dx / metres_lon])
    return out


def route(key, idx):
    """One of the few loops a sport keeps coming back to: the same for every outing that takes it."""
    r = random.Random(f"{key}-{idx}")
    km = r.uniform(*SPORTS[key][1])
    return km, loop(r, km)


def profile(rng, climb):
    if climb <= 0:
        return []
    base = rng.uniform(440, 700)
    peaks = rng.randint(1, 3)
    return [round(base + climb / peaks / 2 * (1 - math.cos(2 * math.pi * peaks * i / (PROFILE - 1))) + rng.uniform(-3, 3), 1) for i in range(PROFILE)]


def bands(rng, seconds, mean):
    spread = rng.uniform(6, 11)
    weights = [math.exp(-(((i + 0.5) * BAND - mean) / spread) ** 2 / 2) for i in range(int(200 / BAND))]
    total = sum(weights)
    return [round(seconds * w / total, 1) for w in weights]


def outing(rng, aid, day, key, fitness):
    names, km_range, speed_range, climb_range, hr = SPORTS[key]
    # Five usual loops per sport, ridden a little differently each time (a few metres of GPS noise).
    route_km, line = route(key, rng.randrange(5))
    km = route_km * rng.uniform(0.98, 1.02)
    line = [[round(lat + rng.gauss(0, 0.00004), 6), round(lon + rng.gauss(0, 0.00005), 6)] for lat, lon in line]
    speed = rng.uniform(*speed_range) * fitness
    seconds = km / speed * 3600
    climb = km * rng.uniform(*climb_range)
    mean_hr = hr + rng.uniform(-6, 6)
    hour = rng.choice([7, 7, 8, 12, 18, 18, 19])
    start = dt.datetime.combine(day, dt.time(hour, rng.randrange(0, 60, 5)))
    act = {
        "activityId": aid,
        "activityName": rng.choice(names),
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
        "profile": profile(rng, climb),
        "hr_bands": bands(rng, seconds, mean_hr),
        "max_hr": round(mean_hr + rng.uniform(12, 25)),
        "version": VERSION,
    }
    return act, derived


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
    acts, derived = [], {}
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
            act, d = outing(rng, aid, day, key, fitness)
            acts.append(act)
            derived[str(aid)] = d
    # Hikes in summer.
    for year in sorted({a.today.year - 1, a.today.year}):
        for day in (dt.date(year, 7, 14), dt.date(year, 8, 10)):
            if day <= a.today:
                aid += 7
                act, d = outing(rng, aid, day, "hiking", 1.0)
                acts.append(act)
                derived[str(aid)] = d
    acts.sort(key=lambda x: x["startTimeLocal"], reverse=True)

    out = a.dir / "astrolabe"
    out.mkdir(parents=True, exist_ok=True)
    (out / "activities.json").write_text(json.dumps(acts))
    (out / "derived.json").write_text(json.dumps(derived))
    (out / "settings").write_text(f"language={a.lang}\ntheme={a.theme}\n")
    print(f"{len(acts)} made-up activities in {out}")


if __name__ == "__main__":
    main()
