"""Builds the data the video page and the soundtrack share: the timeline of typed characters,
whooshes and hits (timeline.js / timeline.json), and the Annecy routes projected to metres (routes.js)."""
import json, math, random, pathlib

HERE = pathlib.Path(__file__).parent
FPS, DUR, BPM = 30, 30.0, 120

# Typed texts: (id, text, start, end). Characters land on a slightly human rhythm.
TYPED = [
    ("star",   "take the height of a star", 0.85, 2.45),
    ("coords", "45.8990 · 6.1290",          13.62, 14.30),
    ("binary", "one binary · rust · gpu",   22.52, 22.98),
    ("url",    "alarboulletmarin.github.io/astrolabe", 28.55, 29.35),
]
rng = random.Random(7)
typed = {}
clicks = []
for key, text, a, b in TYPED:
    n = len(text)
    w = [0.7 + rng.random() * 0.6 for _ in range(n)]
    s = sum(w); acc = 0; times = []
    for i in range(n):
        times.append(round(a + (b - a) * acc / s, 4)); acc += w[i]
    typed[key] = {"text": text, "times": times}
    clicks += [{"t": t, "ch": c, "space": c == " "} for t, c in zip(times, text)]

# Feature words flashing over the screenshot wall, one per 16th... per eighth note.
WORDS = ["Today", "Journal", "Calendar", "Activity", "Statistics", "Form", "Records", "Heatmap", "Loops", "3D", "Light", "Dark", "English", "Français"]
words = [{"t": round(23.25 + i * 0.25, 3), "w": w} for i, w in enumerate(WORDS)]

timeline = {
    "fps": FPS, "duration": DUR, "bpm": BPM,
    "typed": typed, "clicks": sorted(clicks, key=lambda c: c["t"]), "words": words,
    # (start, end) of each whoosh: it peaks at its end, on the cut.
    "whooshes": [[2.45, 3.0], [5.55, 6.0], [9.6, 10.0], [13.15, 13.5], [16.95, 17.35], [22.75, 23.0], [26.35, 27.0]],
    "hits": [3.0, 6.0, 10.0, 13.5, 19.0, 23.0, 27.0],
    "chime": 27.5,
    "cuts": [19.0, 19.5, 20.0, 20.5, 21.0, 21.5, 22.0, 22.5],
}
(HERE / "timeline.json").write_text(json.dumps(timeline, indent=1, ensure_ascii=False))
(HERE / "timeline.js").write_text("window.TL = " + json.dumps(timeline, ensure_ascii=False) + ";\n")

# Routes, in metres around a common origin.
routes = json.loads((HERE.parent / "scripts/demo-routes.json").read_text())
lat0, lon0 = 45.899, 6.129
kx, ky = 111_320 * math.cos(math.radians(lat0)), 110_540
def proj(p): return [round((p[1] - lon0) * kx, 1), round(-(p[0] - lat0) * ky, 1)]

hero_src = routes["trail_running"][2]
pts = [proj(p) for p in hero_src["line"]]
ele = hero_src["eles"]
dist = [0.0]
for i in range(1, len(pts)):
    dist.append(dist[-1] + math.dist(pts[i - 1], pts[i]))
# Grade over about 60 m, bucketed as the app's legend: <3, 3-6, 6-10, 10-15, >15 %.
grade = []
for i in range(len(pts)):
    j, k = i, i
    while j > 0 and dist[i] - dist[j] < 30: j -= 1
    while k < len(pts) - 1 and dist[k] - dist[i] < 30: k += 1
    g = abs(ele[k] - ele[j]) / max(dist[k] - dist[j], 1) * 100
    grade.append(0 if g < 3 else 1 if g < 6 else 2 if g < 10 else 3 if g < 15 else 4)
hero = {"pts": pts, "ele": ele, "dist": [round(d, 1) for d in dist], "grade": grade,
        "climb": round(sum(max(0, ele[i] - ele[i - 1]) for i in range(1, len(ele))))}

others = []
for sport, rs in routes.items():
    for r in rs:
        if r is hero_src: continue
        line = r["line"][:: max(1, len(r["line"]) // 400)]
        others.append({"sport": sport, "pts": [proj(p) for p in line]})
(HERE / "routes.js").write_text("window.HERO = " + json.dumps(hero) + ";\nwindow.OTHERS = " + json.dumps(others) + ";\n")
print("hero km", round(dist[-1] / 1000, 2), "climb", hero["climb"], "max", max(ele), "others", len(others), "clicks", len(clicks))
