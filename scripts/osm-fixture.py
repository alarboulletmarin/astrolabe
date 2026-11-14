#!/usr/bin/env python3
"""Write a small, made-up OpenStreetMap extract (.osm.pbf) for the tests of the loop planner.

USAGE
    scripts/osm-fixture.py tests/fixtures/loops.osm.pbf

No OpenStreetMap data goes in: the network is invented, a 3 km square near 43.70° N, 7.25° E,
so the tests know every length. Streets and paths cross every 250 m, with a node every 50 m
between crossings; a few ways are there to be left out (private, no walkers, a motorway), one
is unreachable (an island), one is a dead end, one is steps. Nodes come in several blocks, as
in a real extract. The file is the same at every run.
"""
import struct
import sys
import zlib

LAT0, LON0 = 43.70, 7.25
STEP = 250.0  # metres between crossings
SUB = 5  # pieces per stretch between two crossings (a node every 50 m)
SIZE = 12  # stretches per side: 3 km
DLAT = 1 / 111_195.0  # degrees per metre north
DLON = 1 / (111_195.0 * 0.7230)  # degrees per metre east at 43.70°


def varint(v):
    out = bytearray()
    while v >= 0x80:
        out.append((v & 0x7F) | 0x80)
        v >>= 7
    out.append(v)
    return bytes(out)


def zz(v):
    return (v << 1) ^ (v >> 63) if v >= 0 else ((-v) << 1) - 1


def key(field, wire):
    return varint(field << 3 | wire)


def uint(field, v):
    return key(field, 0) + varint(v)


def lenf(field, b):
    return key(field, 2) + varint(len(b)) + b


def packed(field, values):
    return lenf(field, b"".join(varint(v) for v in values))


def delta(values):
    out, prev = [], 0
    for v in values:
        out.append(zz(v - prev))
        prev = v
    return out


def blob(kind, raw):
    z = zlib.compress(raw, 9)
    body = uint(2, len(raw)) + lenf(3, z)
    head = lenf(1, kind.encode()) + uint(3, len(body))
    return struct.pack(">I", len(head)) + head + body


def main(path):
    nodes = {}  # id -> (lat, lon) in 1e-7 degrees
    ways = []  # (id, [(k, v)], [refs])

    def node_at(x, y):
        """The node at x, y metres east and north of the corner (made once)."""
        nid = 1 + int(round(y / 50)) * 1000 + int(round(x / 50))
        nodes.setdefault(nid, (round((LAT0 + y * DLAT) * 1e7), round((LON0 + x * DLON) * 1e7)))
        return nid

    def line(points):
        return [node_at(x, y) for x, y in points]

    def straight(x0, y0, x1, y1):
        n = int(round(max(abs(x1 - x0), abs(y1 - y0)) / 50))
        return [(x0 + (x1 - x0) * k / n, y0 + (y1 - y0) * k / n) for k in range(n + 1)]

    side = STEP * SIZE
    wid = 1000
    # Rows west to east: streets on even rows, dirt paths on odd ones.
    for r in range(SIZE + 1):
        y = r * STEP
        tags = [("highway", "residential")] if r % 2 == 0 else [("highway", "path"), ("surface", "ground")]
        ways.append((wid, tags, line(straight(0, y, side, y))))
        wid += 1
    # Columns south to north: tracks, footways, unclassified roads, in turn.
    kinds = [
        [("highway", "track"), ("tracktype", "grade3")],
        [("highway", "footway")],
        [("highway", "unclassified"), ("surface", "asphalt")],
    ]
    for c in range(SIZE + 1):
        x = c * STEP
        ways.append((wid, kinds[c % 3], line(straight(x, 0, x, side))))
        wid += 1
    # Mountain paths on the diagonal of the north-east quarter.
    ways.append((wid, [("highway", "path"), ("sac_scale", "mountain_hiking"), ("trail_visibility", "good")], line(straight(side / 2, side / 2, side, side))))
    wid += 1
    # Left out: a private track, a footway closed to walkers, a motorway.
    ways.append((wid, [("highway", "track"), ("access", "private")], line(straight(0, 0, side / 2, side / 2))))
    wid += 1
    ways.append((wid, [("highway", "footway"), ("foot", "no")], line(straight(side, 0, side / 2, side / 2))))
    wid += 1
    ways.append((wid, [("highway", "motorway")], line(straight(-500, 0, -500, side))))
    wid += 1
    # A dead end, steps going up to it, and an island 1 km off the square.
    ways.append((wid, [("highway", "footway"), ("surface", "dirt")], line(straight(side, side / 2, side + 300, side / 2))))
    wid += 1
    ways.append((wid, [("highway", "steps")], line(straight(side + 300, side / 2, side + 300, side / 2 + 100))))
    wid += 1
    ways.append((wid, [("highway", "path")], line(straight(side + 1000, 0, side + 1000, 500))))
    wid += 1

    strings = [""]

    def s(text):
        if text not in strings:
            strings.append(text)
        return strings.index(text)

    out = bytearray()
    header = lenf(4, b"OsmSchema-V0.6") + lenf(4, b"DenseNodes") + uint(32, 1_760_000_000)
    out += blob("OSMHeader", header)
    ids = sorted(nodes)
    # Nodes in blocks of 400, sorted, as Geofabrik writes them.
    for start in range(0, len(ids), 400):
        chunk = ids[start:start + 400]
        dense = packed(1, delta(chunk)) + packed(8, delta([nodes[i][0] for i in chunk])) + packed(9, delta([nodes[i][1] for i in chunk]))
        group = lenf(2, dense)
        out += blob("OSMData", lenf(1, lenf(1, b"")) + lenf(2, group) + uint(17, 100))
    for start in range(0, len(ways), 10):
        strings[:] = [""]
        group = b""
        for wid_, tags, refs in ways[start:start + 10]:
            keys = [s(k) for k, _ in tags]
            vals = [s(v) for _, v in tags]
            group += lenf(3, uint(1, wid_) + packed(2, keys) + packed(3, vals) + packed(8, delta(refs)))
        table = b"".join(lenf(1, t.encode()) for t in strings)
        out += blob("OSMData", lenf(1, table) + lenf(2, group) + uint(17, 100))
    with open(path, "wb") as f:
        f.write(out)
    print(f"{path}: {len(nodes)} nodes, {len(ways)} ways, {len(out)} bytes")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
