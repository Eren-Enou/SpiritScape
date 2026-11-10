#!/usr/bin/env python3
"""Builds the game's own simple models as binary glTF, from nothing but this
file. Run it from the repository root:

    python tools/make_models.py

Everything is flat-shaded blocks of colour, in the same spirit as the rest of
the world. One tile is one unit, y is up, and models sit on y = 0.
"""
import json
import math
import pathlib
import struct

OUT = pathlib.Path("assets/models/own")

BRICK = (0.62, 0.30, 0.22)
BRICK_DARK = (0.50, 0.23, 0.18)
MORTAR = (0.72, 0.66, 0.58)
SOOT = (0.13, 0.11, 0.11)
GLOW = (0.95, 0.55, 0.20)
STILLWOOD = (0.62, 0.72, 0.80)
STILLWOOD_DARK = (0.44, 0.54, 0.62)
PITCH = (0.10, 0.10, 0.12)
LANTERN = (0.98, 0.92, 0.66)
TIMBER = (0.48, 0.34, 0.22)
CLAY = (0.70, 0.50, 0.38)
CLAY_DARK = (0.58, 0.40, 0.30)
CLAY_WET = (0.46, 0.32, 0.25)


class Model:
    """Triangles, grouped by colour so each colour is one primitive."""

    def __init__(self):
        self.groups = {}

    def tri(self, a, b, c, color):
        ux, uy, uz = (b[i] - a[i] for i in range(3))
        vx, vy, vz = (c[i] - a[i] for i in range(3))
        n = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
        length = math.sqrt(sum(x * x for x in n))
        if length < 1e-9:
            return
        n = tuple(x / length for x in n)
        points, normals = self.groups.setdefault(color, ([], []))
        points += [a, b, c]
        normals += [n, n, n]

    def quad(self, a, b, c, d, color):
        self.tri(a, b, c, color)
        self.tri(a, c, d, color)

    def box(self, center, size, color):
        x, y, z = center
        w, h, d = (s / 2 for s in size)
        p = [
            (x - w, y - h, z - d), (x + w, y - h, z - d),
            (x + w, y + h, z - d), (x - w, y + h, z - d),
            (x - w, y - h, z + d), (x + w, y - h, z + d),
            (x + w, y + h, z + d), (x - w, y + h, z + d),
        ]
        for f in [(4, 5, 6, 7), (1, 0, 3, 2), (5, 1, 2, 6),
                  (0, 4, 7, 3), (7, 6, 2, 3), (0, 1, 5, 4)]:
            self.quad(*(p[i] for i in f), color)

    def ring(self, center, radius, sides, squash=1.0):
        x, y, z = center
        return [
            (x + radius * math.cos(2 * math.pi * i / sides),
             y,
             z + squash * radius * math.sin(2 * math.pi * i / sides))
            for i in range(sides)
        ]

    def tower(self, center, rings, sides, color, squash=1.0, cap=True):
        """A stack of (height, radius) rings around a centre on the ground."""
        x, y, z = center
        levels = [self.ring((x, y + h, z), r, sides, squash) for h, r in rings]
        for low, high in zip(levels, levels[1:]):
            for i in range(sides):
                j = (i + 1) % sides
                self.quad(low[j], low[i], high[i], high[j], color)
        if cap and rings[-1][1] > 0.001:
            top = (x, y + rings[-1][0], z)
            for i in range(sides):
                self.tri(top, levels[-1][(i + 1) % sides], levels[-1][i], color)

    def brick_course(self, center, length, height, depth, along_x, row):
        """One row of bricks, alternating so the wall reads as brick."""
        count = max(2, round(length / 0.2))
        each = length / count
        x, y, z = center
        for i in range(count):
            at = -length / 2 + each * (i + 0.5)
            color = BRICK if (i + row) % 2 == 0 else BRICK_DARK
            spot = (x + at, y, z) if along_x else (x, y, z + at)
            size = (each * 0.94, height * 0.9, depth) if along_x else (depth, height * 0.9, each * 0.94)
            self.box(spot, size, color)

    def brick_wall(self, center, length, rows, depth=0.16, along_x=True):
        x, y, z = center
        height = 0.11
        behind = (length, rows * height, depth * 0.8) if along_x else (depth * 0.8, rows * height, length)
        self.box((x, y + rows * height / 2, z), behind, MORTAR)
        for r in range(rows):
            self.brick_course((x, y + (r + 0.5) * height, z), length, height, depth, along_x, r)


def kiln():
    m = Model()
    # A squat brick dome, wider than it's tall, on a low base.
    m.box((0, 0.06, 0), (0.92, 0.12, 0.92), MORTAR)
    rings = [(0.10, 0.42), (0.34, 0.42), (0.52, 0.36), (0.66, 0.25), (0.74, 0.10), (0.76, 0.0)]
    m.tower((0, 0, 0), rings, 10, BRICK, cap=False)
    # Courses of darker brick round the dome.
    for h, r in [(0.20, 0.435), (0.40, 0.405)]:
        m.tower((0, h, 0), [(0, r), (0.05, r)], 10, BRICK_DARK, cap=False)
    # The firebox door, with the fire showing.
    m.box((0, 0.22, 0.40), (0.30, 0.26, 0.08), SOOT)
    m.box((0, 0.18, 0.43), (0.20, 0.12, 0.03), GLOW)
    m.box((0, 0.37, 0.41), (0.36, 0.05, 0.10), BRICK_DARK)
    # The flue.
    m.box((0.10, 0.82, -0.12), (0.18, 0.46, 0.18), BRICK_DARK)
    m.box((0.10, 1.06, -0.12), (0.22, 0.05, 0.22), SOOT)
    # Fuel stacked by the door.
    for i, (x, y) in enumerate([(-0.33, 0.16), (-0.25, 0.16), (-0.29, 0.24)]):
        m.box((x, y, 0.40), (0.07, 0.07, 0.24), TIMBER if i % 2 else (0.40, 0.28, 0.18))
    return m


def still_boat():
    m = Model()
    # A narrow hull, lofted from sections along z. Bow and stern come to points.
    sections = [
        (-0.62, 0.00, 0.34, 0.34),
        (-0.46, 0.15, 0.13, 0.30),
        (-0.18, 0.24, 0.07, 0.26),
        (0.14, 0.24, 0.07, 0.26),
        (0.42, 0.15, 0.13, 0.30),
        (0.62, 0.00, 0.36, 0.36),
    ]

    def outline(z, half, keel, rail):
        return [(-half, rail, z), (-half * 0.72, (keel + rail) / 2, z),
                (0.0, keel, z), (half * 0.72, (keel + rail) / 2, z), (half, rail, z)]

    rings = [outline(*s) for s in sections]
    for a, b in zip(rings, rings[1:]):
        for i in range(len(a) - 1):
            color = PITCH if i in (1, 2) else STILLWOOD
            m.quad(a[i], a[i + 1], b[i + 1], b[i], color)
            m.quad(b[i], b[i + 1], a[i + 1], a[i], color)
    # The deck, and a rail along each side.
    for a, b in zip(rings, rings[1:]):
        m.quad(a[0], a[4], b[4], b[0], STILLWOOD_DARK)
        m.quad(b[0], b[4], a[4], a[0], STILLWOOD_DARK)
    for side in (-1, 1):
        m.box((side * 0.235, 0.31, 0), (0.035, 0.05, 0.62), STILLWOOD_DARK)
    # Two thwarts to sit on.
    for z in (-0.16, 0.16):
        m.box((0, 0.27, z), (0.44, 0.03, 0.09), STILLWOOD)
    # The lantern on its post, at the bow.
    m.box((0, 0.56, -0.50), (0.035, 0.58, 0.035), STILLWOOD_DARK)
    m.box((0, 0.84, -0.44), (0.03, 0.03, 0.14), STILLWOOD_DARK)
    m.box((0, 0.76, -0.38), (0.09, 0.12, 0.09), LANTERN)
    m.box((0, 0.83, -0.38), (0.11, 0.02, 0.11), PITCH)
    return m


def warm_wall():
    m = Model()
    # An L of low wall, with a flue at the corner and a little fire at its foot.
    m.brick_wall((0.02, 0, -0.34), 0.84, 6)
    m.brick_wall((-0.34, 0, 0.08), 0.74, 6, along_x=False)
    m.box((-0.34, 0.45, -0.34), (0.22, 0.90, 0.22), BRICK_DARK)
    m.box((-0.34, 0.92, -0.34), (0.27, 0.05, 0.27), SOOT)
    m.box((-0.20, 0.09, -0.21), (0.10, 0.10, 0.03), SOOT)
    m.box((-0.20, 0.08, -0.19), (0.07, 0.06, 0.02), GLOW)
    return m


def chimney():
    m = Model()
    # A tall stack, stepped in as it rises.
    m.box((0, 0.06, 0), (0.74, 0.12, 0.74), MORTAR)
    height, width, row = 0.12, 0.60, 0
    while height < 1.86:
        for dz, along in [(width / 2, True), (-width / 2, True)]:
            m.brick_course((0, height + 0.05, dz), width, 0.11, 0.09, along, row)
        for dx in (width / 2, -width / 2):
            m.brick_course((dx, height + 0.05, 0), width, 0.11, 0.09, False, row + 1)
        height += 0.11
        row += 1
        width = max(0.34, width - 0.016)
    m.box((0, 1.9, 0), (width + 0.12, 0.07, width + 0.12), BRICK_DARK)
    m.box((0, 1.97, 0), (width - 0.08, 0.10, width - 0.08), SOOT)
    m.box((0, 0.95, 0), (width, 1.7, width), SOOT)
    return m


def clay_bank():
    m = Model()
    # A low wet mound, cut away on one side.
    m.tower((0, 0, 0), [(0, 0.46), (0.11, 0.44), (0.21, 0.33), (0.27, 0.16), (0.29, 0.0)],
            9, CLAY, squash=0.82, cap=False)
    m.tower((0.17, 0, 0.12), [(0, 0.22), (0.12, 0.18), (0.18, 0.09), (0.20, 0.0)],
            7, CLAY_DARK, cap=False)
    m.box((-0.16, 0.02, 0.22), (0.26, 0.05, 0.16), CLAY_WET)
    return m


def clay_dug():
    m = Model()
    # What's left once it's been dug: a shallow dark scrape.
    m.tower((0, 0, 0), [(0, 0.42), (0.04, 0.40), (0.06, 0.30), (0.07, 0.0)],
            9, CLAY_WET, squash=0.82, cap=False)
    return m


def write_glb(path, model):
    binary = bytearray()
    views, accessors, primitives, materials = [], [], [], []

    def add_view(data, target):
        while len(binary) % 4:
            binary.append(0)
        views.append({"buffer": 0, "byteOffset": len(binary),
                      "byteLength": len(data), "target": target})
        binary.extend(data)
        return len(views) - 1

    for color, (points, normals) in sorted(model.groups.items()):
        flat = [c for p in points for c in p]
        pos = add_view(struct.pack(f"<{len(flat)}f", *flat), 34962)
        lows = [min(p[i] for p in points) for i in range(3)]
        highs = [max(p[i] for p in points) for i in range(3)]
        accessors.append({"bufferView": pos, "componentType": 5126, "count": len(points),
                          "type": "VEC3", "min": lows, "max": highs})
        flat = [c for n in normals for c in n]
        nor = add_view(struct.pack(f"<{len(flat)}f", *flat), 34962)
        accessors.append({"bufferView": nor, "componentType": 5126, "count": len(normals),
                          "type": "VEC3"})
        materials.append({"pbrMetallicRoughness": {
            "baseColorFactor": [*color, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.9}})
        primitives.append({"attributes": {"POSITION": len(accessors) - 2,
                                          "NORMAL": len(accessors) - 1},
                           "material": len(materials) - 1})
    while len(binary) % 4:
        binary.append(0)
    doc = {
        "asset": {"version": "2.0", "generator": "SpiritScape tools/make_models.py"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": primitives}],
        "buffers": [{"byteLength": len(binary)}],
        "bufferViews": views, "accessors": accessors, "materials": materials,
    }
    text = json.dumps(doc, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    out = struct.pack("<4sII", b"glTF", 2, total)
    out += struct.pack("<I4s", len(text), b"JSON") + text
    out += struct.pack("<I4s", len(binary), b"BIN\0") + bytes(binary)
    path.write_bytes(out)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, build in [("kiln", kiln), ("still_boat", still_boat), ("warm_wall", warm_wall),
                        ("chimney", chimney), ("clay_bank", clay_bank), ("clay_dug", clay_dug)]:
        path = OUT / f"{name}.glb"
        write_glb(path, build())
        print(f"{path}: {path.stat().st_size} bytes")


if __name__ == "__main__":
    main()
