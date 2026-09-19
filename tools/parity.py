"""Confronta il rendering Rust con quello Python (il riferimento), frame per frame.

Uso:  python tools/parity.py            (dopo `cargo build --release`)

Solo i layer deterministici: ripple, starlight e i colori a caso usano generatori diversi
nei due linguaggi e non possono coincidere. Tolleranza di 1 su 255 per gli arrotondamenti.
"""
import json
import logging
import os
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "python"))
logging.basicConfig(level=logging.ERROR)
import chroma_layers as cl
import demonnapse as d

EXE = os.path.join(d.HERE, "target", "release", "demonnapse.exe")
T0, FRAMES, FPS = 1234.5, 600, 30
RANDOM_LAYERS = {"ripple", "starlight"}


class Silent:
    def peak(self):
        return 0.0


def python_frames(spec, cfg):
    fx = d.Effect(spec, cfg)
    return [fx.render(T0 + n / FPS, 1 / FPS, Silent()) for n in range(FRAMES)]


def rust_frames(title, extra):
    out = subprocess.run([EXE, "render", title, str(T0), str(FRAMES), str(FPS)] + extra,
                         capture_output=True, text=True, cwd=d.HERE)
    if out.returncode:
        raise RuntimeError(out.stderr.strip())
    return [[tuple(int(c[k:k + 2], 16) for k in (0, 2, 4)) for c in line.split()] for line in out.stdout.splitlines()]


def main():
    cfg, _ = d.load()
    dump = json.load(open(os.path.join(d.HERE, "chroma_effects_dump.json"), encoding="utf-8"))
    everywhere = [{"leds": [{"row": r, "col": c} for r, c in d.GRID]}]
    cases = [(t, None, dump[t]) for t in dump]          # ogni effetto intero
    for t, e in dump.items():                            # ogni layer da solo, steso su tutti i LED
        for l in e["layers"]:
            if l["name"] in cl.LAYERS:
                cases.append((t, l["name"], dict(e, layers=[dict(l, regions=[dict(l["regions"][0], devices=everywhere)])])))
    failed = 0
    for title, layer, spec in cases:
        names = {l["name"] for l in spec["layers"]}
        rnd = any(r["params"].get("randomColor") for l in spec["layers"] for r in l["regions"])
        label = "%-24s %-10s" % (title, layer or "(intero)")
        if names & RANDOM_LAYERS or rnd:
            print(label, "saltato (contiene casualita')")
            continue
        py = python_frames(spec, cfg)
        rs = rust_frames(title, ["--layer", layer, "--all"] if layer else [])
        worst = max((abs(a - b) for fp, fr in zip(py, rs) for cp, cr in zip(fp, fr) for a, b in zip(cp, cr)), default=0)
        ok = len(py) == len(rs) and worst <= 1
        failed += not ok
        print(label, "OK" if ok else "DIVERSO", "| scarto massimo %d/255 su %d frame" % (worst, len(rs)))
    print("\n%s" % ("tutto coincide" if not failed else "%d casi diversi" % failed))
    return failed


if __name__ == "__main__":
    sys.exit(main())
