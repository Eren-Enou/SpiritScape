"""Generates the game's sound effects from scratch, so they're all original.

    python tools/make_sounds.py

Everything is synthesised here. It uses a fixed seed, so running it again gives
the same files.
"""

import math
import os
import random
import struct
import wave

RATE = 22050
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "audio")


def write(name, samples):
    os.makedirs(OUT, exist_ok=True)
    peak = max(1e-6, max(abs(s) for s in samples))
    with wave.open(os.path.join(OUT, name + ".wav"), "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(RATE)
        data = b"".join(struct.pack("<h", int(s / peak * 26000)) for s in samples)
        f.writeframes(data)


def bell(freq, seconds, decay=5.0):
    """A struck tone with a few inharmonic partials."""
    out = []
    for i in range(int(seconds * RATE)):
        t = i / RATE
        e = math.exp(-decay * t)
        out.append(
            e
            * (
                math.sin(2 * math.pi * freq * t)
                + 0.45 * math.sin(2 * math.pi * freq * 2.01 * t)
                + 0.2 * math.sin(2 * math.pi * freq * 3.03 * t)
            )
        )
    return out


def mix(parts):
    """Lays (start seconds, samples) pairs over each other."""
    end = max(int(start * RATE) + len(s) for start, s in parts)
    out = [0.0] * end
    for start, s in parts:
        at = int(start * RATE)
        for i, v in enumerate(s):
            out[at + i] += v
    return out


def noise(seconds, decay, smooth, rng):
    """Filtered noise, for wood and stone."""
    out, last = [], 0.0
    for i in range(int(seconds * RATE)):
        t = i / RATE
        last += (rng.uniform(-1, 1) - last) * smooth
        out.append(last * math.exp(-decay * t))
    return out


def thump(freq, seconds, decay):
    return [
        math.exp(-decay * i / RATE) * math.sin(2 * math.pi * freq * (i / RATE))
        for i in range(int(seconds * RATE))
    ]


def main():
    rng = random.Random(7)
    write("chop", mix([(0, noise(0.12, 40, 0.5, rng)), (0, thump(140, 0.14, 30))]))
    write("stone", mix([(0, noise(0.1, 55, 0.9, rng)), (0, thump(320, 0.08, 45))]))
    write("saw", mix([(0, noise(0.12, 18, 0.7, rng)), (0.13, noise(0.12, 18, 0.7, rng))]))
    write("notice", bell(880, 0.35, 11))
    write(
        "level",
        mix([(0.0, bell(523, 0.7)), (0.11, bell(659, 0.7)), (0.22, bell(784, 0.9))]),
    )
    write("built", mix([(0.0, bell(392, 0.9, 4)), (0.2, bell(587, 1.1, 4))]))
    write(
        "season",
        [
            math.sin(math.pi * i / (2.4 * RATE)) ** 2
            * (
                math.sin(2 * math.pi * 196 * i / RATE)
                + 0.6 * math.sin(2 * math.pi * 294 * i / RATE)
            )
            for i in range(int(2.4 * RATE))
        ],
    )
    write(
        "hearth",
        mix(
            [
                (0.0, bell(196, 5.0, 0.7)),
                (0.5, bell(294, 4.5, 0.8)),
                (1.0, bell(392, 4.0, 0.9)),
                (1.5, bell(494, 3.5, 1.0)),
                (2.0, bell(587, 3.0, 1.1)),
            ]
        ),
    )


if __name__ == "__main__":
    main()
